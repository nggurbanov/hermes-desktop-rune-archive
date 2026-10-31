//! Native GPT-Live media: real ICE/DTLS/SRTP/SCTP, Opus and native devices.
//! The authenticated gateway exchanges only SDP; API credentials never enter
//! the peer, an executable argument or a WebView.
use super::{
    device::{Capture, Controls},
    dsp::{RenderReference, VoiceDsp},
    live::Event,
    live_protocol::Protocol,
};
use crate::{
    composer::DraftScope,
    gateway::{Connection, audio::AudioGateway},
};
use bytes::Bytes;
use rune::Waker;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc as asynchronous;
use webrtc::{
    api::{
        APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MIME_TYPE_OPUS, MediaEngine},
        setting_engine::SettingEngine,
    },
    data_channel::{
        RTCDataChannel, data_channel_message::DataChannelMessage,
        data_channel_state::RTCDataChannelState,
    },
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTPCodecType},
    track::track_local::{TrackLocal, track_local_static_sample::TrackLocalStaticSample},
};

pub(super) enum Raw {
    Vendor(Value),
    State(RTCPeerConnectionState),
    ChannelOpen,
    ChannelClosed,
    Speaking(bool),
    Level(f32),
    Fatal(String),
}
struct Peer {
    connection: Arc<RTCPeerConnection>,
    track: Arc<TrackLocalStaticSample>,
    channel: Option<Arc<RTCDataChannel>>,
    incoming: asynchronous::UnboundedReceiver<Arc<RTCDataChannel>>,
}
fn data_handlers(channel: &Arc<RTCDataChannel>, events: asynchronous::UnboundedSender<Raw>) {
    let open = events.clone();
    channel.on_open(Box::new(move || {
        let events = open.clone();
        Box::pin(async move {
            let _ = events.send(Raw::ChannelOpen);
        })
    }));
    let closed = events.clone();
    channel.on_close(Box::new(move || {
        let events = closed.clone();
        Box::pin(async move {
            let _ = events.send(Raw::ChannelClosed);
        })
    }));
    channel.on_message(Box::new(move |message: DataChannelMessage| {
        let events = events.clone();
        Box::pin(async move {
            match serde_json::from_slice::<Value>(&message.data) {
                Ok(value) => {
                    let _ = events.send(Raw::Vendor(value));
                }
                Err(_) => {
                    let _ = events.send(Raw::Fatal(
                        "GPT-Live sent an invalid data-channel event".into(),
                    ));
                }
            }
        })
    }));
}
async fn peer(
    events: asynchronous::UnboundedSender<Raw>,
    output: SyncSender<Vec<f32>>,
    offerer: bool,
    loopback: bool,
) -> Result<Peer, String> {
    let mut media = MediaEngine::default();
    media
        .register_default_codecs()
        .map_err(|_| "The native RTC audio codecs could not be registered")?;
    let interceptors = register_default_interceptors(Registry::new(), &mut media)
        .map_err(|_| "The native RTP feedback handlers could not be initialized")?;
    let mut settings = SettingEngine::default();
    if loopback {
        settings.set_include_loopback_candidate(true);
        settings.set_ip_filter(Box::new(|ip| ip.is_loopback()));
        settings.set_network_types(vec![webrtc::ice::network_type::NetworkType::Udp4]);
    }
    let api = APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(interceptors)
        .with_setting_engine(settings)
        .build();
    let connection = Arc::new(
        api.new_peer_connection(RTCConfiguration::default())
            .await
            .map_err(|_| "The native WebRTC peer could not be created")?,
    );
    let state = events.clone();
    connection.on_peer_connection_state_change(Box::new(move |status| {
        let state = state.clone();
        Box::pin(async move {
            let _ = state.send(Raw::State(status));
        })
    }));
    let track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_OPUS.into(),
            clock_rate: 48000,
            channels: 2,
            sdp_fmtp_line: "minptime=10;useinbandfec=1".into(),
            ..Default::default()
        },
        "microphone".into(),
        "hermes-voice".into(),
    ));
    let sender = connection
        .add_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>)
        .await
        .map_err(|_| "The microphone RTP track could not be attached")?;
    tokio::spawn(async move { while sender.read_rtcp().await.is_ok() {} });
    let incoming_events = events.clone();
    connection.on_track(Box::new(move |track, _, _| {
        let output = output.clone();
        let events = incoming_events.clone();
        Box::pin(async move {
            if track.kind() != RTPCodecType::Audio {
                return;
            }
            if !track
                .codec()
                .capability
                .mime_type
                .eq_ignore_ascii_case(MIME_TYPE_OPUS)
            {
                let _ = events.send(Raw::Fatal(
                    "GPT-Live selected an unsupported native audio codec".into(),
                ));
                return;
            }
            let mut decoder = match opus::Decoder::new(48000, opus::Channels::Stereo) {
                Ok(value) => value,
                Err(_) => {
                    let _ = events.send(Raw::Fatal(
                        "The incoming Opus decoder could not start".into(),
                    ));
                    return;
                }
            };
            let mut buffer = vec![0.0f32; 5760 * 2];
            let mut previous = None;
            while let Ok((packet, _)) = track.read_rtp().await {
                let sequence = packet.header.sequence_number;
                if let Some(last) = previous {
                    let distance = sequence.wrapping_sub(last);
                    if distance == 0 || distance > 32768 {
                        continue;
                    }
                    for _ in 1..distance.min(4) {
                        if let Ok(samples) = decoder.decode_float(&[], &mut buffer, false) {
                            let _ = output.try_send(buffer[..samples * 2].to_vec());
                        }
                    }
                }
                previous = Some(sequence);
                match decoder.decode_float(&packet.payload, &mut buffer, false) {
                    Ok(samples) => match output.try_send(buffer[..samples * 2].to_vec()) {
                        Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                        Err(mpsc::TrySendError::Disconnected(_)) => break,
                    },
                    Err(_) => {
                        let _ = events.send(Raw::Fatal(
                            "An incoming GPT-Live Opus packet could not be decoded".into(),
                        ));
                        break;
                    }
                }
            }
        })
    }));
    let (incoming_send, incoming) = asynchronous::unbounded_channel();
    let remote_events = events.clone();
    connection.on_data_channel(Box::new(move |channel| {
        let events = remote_events.clone();
        let send = incoming_send.clone();
        Box::pin(async move {
            data_handlers(&channel, events);
            let _ = send.send(channel);
        })
    }));
    let channel = if offerer {
        let channel = connection
            .create_data_channel("oai-events", None)
            .await
            .map_err(|_| "The GPT-Live data channel could not be opened")?;
        data_handlers(&channel, events);
        Some(channel)
    } else {
        None
    };
    Ok(Peer {
        connection,
        track,
        channel,
        incoming,
    })
}
async fn offer(peer: &Arc<RTCPeerConnection>) -> Result<String, String> {
    let mut gathered = peer.gathering_complete_promise().await;
    let offer = peer
        .create_offer(None)
        .await
        .map_err(|_| "The native peer did not produce an SDP offer")?;
    peer.set_local_description(offer)
        .await
        .map_err(|_| "The native SDP offer could not be committed")?;
    let _ = tokio::time::timeout(Duration::from_secs(10), gathered.recv()).await;
    peer.local_description()
        .await
        .map(|s| s.sdp)
        .filter(|s| !s.trim().is_empty())
        .ok_or("The native peer has no SDP offer".into())
}
struct MediaGuard(Arc<Controls>);
impl Drop for MediaGuard {
    fn drop(&mut self) {
        self.0.stop.store(true, Ordering::Release);
    }
}

pub(super) fn spawn(
    connection: Connection,
    scope: DraftScope,
    history: Value,
    commands: Receiver<Value>,
    events: Sender<Event>,
    waker: Waker,
) -> Arc<Controls> {
    let controls = Arc::new(Controls::default());
    let owner = controls.clone();
    std::thread::spawn(move || {
        let emit = |event| {
            if events.send(event).is_ok() {
                waker();
            }
        };
        let _media_guard = MediaGuard(owner.clone());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build();
        let result = match runtime {
            Ok(runtime) => {
                runtime.block_on(run(connection, scope, history, commands, owner, &emit))
            }
            Err(_) => Err("The native RTC runtime could not start".into()),
        };
        if let Err(error) = result {
            emit(Event::Error(error));
        }
    });
    controls
}
async fn run(
    connection: Connection,
    scope: DraftScope,
    history: Value,
    commands: Receiver<Value>,
    controls: Arc<Controls>,
    emit: &(impl Fn(Event) + Sync),
) -> Result<(), String> {
    let capture = Capture::open_live(controls.clone())?;
    let dsp = VoiceDsp::new(capture.rate)?;
    let (raw_send, mut raw) = asynchronous::unbounded_channel();
    let (output_send, output_receive) = mpsc::sync_channel(128);
    let speaker = output(
        output_receive,
        controls.clone(),
        raw_send.clone(),
        dsp.clone(),
    )?;
    let peer = peer(raw_send.clone(), output_send, true, false).await?;
    let result = run_peer(
        &peer,
        connection,
        scope,
        history,
        commands,
        controls.clone(),
        capture,
        dsp,
        &mut raw,
        raw_send,
        emit,
    )
    .await;
    controls.stop.store(true, Ordering::Release);
    let _ = peer.connection.close().await;
    drop(speaker);
    result
}
async fn run_peer(
    peer: &Peer,
    connection: Connection,
    scope: DraftScope,
    history: Value,
    commands: Receiver<Value>,
    controls: Arc<Controls>,
    capture: Capture,
    dsp: Arc<VoiceDsp>,
    raw: &mut asynchronous::UnboundedReceiver<Raw>,
    raw_send: asynchronous::UnboundedSender<Raw>,
    emit: &(impl Fn(Event) + Sync),
) -> Result<(), String> {
    if controls.stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let sdp = offer(&peer.connection).await?;
    let exchange_controls = controls.clone();
    let mut answer = tokio::task::spawn_blocking(move || {
        let mut gateway =
            AudioGateway::connect_scoped(&connection, &scope.profile, scope.profile_intent)?;
        if exchange_controls.stop.load(Ordering::Acquire) {
            return Err("The voice call was cancelled before the SDP exchange".into());
        }
        gateway.live_answer(&sdp, history)
    });
    loop {
        tokio::select! {
         result=&mut answer=>{let answer=result.map_err(|_|"The gateway SDP exchange worker stopped")??;if controls.stop.load(Ordering::Acquire){return Ok(());}let description=RTCSessionDescription::answer(answer.sdp).map_err(|_|"GPT-Live returned an invalid SDP answer")?;peer.connection.set_remote_description(description).await.map_err(|_|"The native RTC peer rejected GPT-Live's SDP answer")?;break;},
         _=tokio::time::sleep(Duration::from_millis(30))=>{if controls.stop.load(Ordering::Acquire){return Ok(());}for command in commands.try_iter(){if command["type"]=="close"{return Ok(());}if command["type"]=="mute"{controls.muted.store(command["muted"]==true,Ordering::Release);}}}
        }
    }
    let microphone = capture.samples.clone();
    let sender_controls = controls.clone();
    let track = peer.track.clone();
    let state = peer.connection.clone();
    let sender_events = raw_send.clone();
    let sender = tokio::spawn(async move {
        if let Err(error) = send_microphone(
            capture,
            track,
            state,
            sender_controls,
            sender_events.clone(),
            dsp,
        )
        .await
        {
            let _ = sender_events.send(Raw::Fatal(error));
        }
    });
    let channel = peer
        .channel
        .as_ref()
        .ok_or("The native voice peer has no data channel")?;
    let mut protocol = Protocol::default();
    let mut pending = VecDeque::new();
    let mut closing = None;
    let began = Instant::now();
    let mut interval = tokio::time::interval(Duration::from_millis(20));
    let result = 'events: loop {
        tokio::select! {
         message=raw.recv()=>{let Some(message)=message else{break Err("The native RTC event stream ended".into());};match message{
          Raw::Vendor(value)=>{for event in protocol.vendor(&value){emit(event);}if protocol.closed{break Ok(());}},
          Raw::ChannelOpen=>{},Raw::ChannelClosed=>{if closing.is_some(){break Ok(());}else{break Err("The GPT-Live data channel closed unexpectedly".into());}},
          Raw::State(RTCPeerConnectionState::Failed)=>break Err("The GPT-Live ICE/DTLS connection failed".into()),
          Raw::State(RTCPeerConnectionState::Closed)=>break Ok(()),Raw::State(_)=>{},
          Raw::Speaking(speaking)=>emit(Event::Speaking(speaking)),Raw::Level(level)=>emit(Event::Level(level)),Raw::Fatal(error)=>break Err(error),
         }},
         _=interval.tick()=>{
          if closing.is_none(){for command in commands.try_iter(){
           if command["type"]=="mute"{controls.muted.store(command["muted"]==true,Ordering::Release);if let Ok(mut samples)=microphone.lock(){samples.clear();}}
           if command["type"]=="close"{controls.stop.store(true,Ordering::Release);closing=Some(Instant::now());}
           pending.extend(protocol.command(&command));
          }}
          if controls.failed.load(Ordering::Acquire){break Err("A native audio device stopped during the voice call".into());}
          if controls.stop.load(Ordering::Acquire)&&closing.is_none(){closing=Some(Instant::now());pending.extend(protocol.command(&json!({"type":"close"})));}
          if protocol.stop_phrase()&&closing.is_none(){controls.stop.store(true,Ordering::Release);closing=Some(Instant::now());pending.extend(protocol.command(&json!({"type":"close"})));}
          if channel.ready_state()==RTCDataChannelState::Open{while let Some(frame)=pending.pop_front(){match tokio::time::timeout(Duration::from_secs(5),channel.send_text(frame.to_string())).await{Ok(Ok(_))=>{},_=>break 'events Err("The GPT-Live control channel stopped accepting messages".into()),}}}
          if closing.is_some_and(|time|time.elapsed()>Duration::from_secs(15)){break Ok(());}
          if began.elapsed()>Duration::from_secs(45)&&!protocol.started{break Err("GPT-Live did not start its voice session".into());}
          if began.elapsed()>Duration::from_secs(30)&&peer.connection.connection_state()!=RTCPeerConnectionState::Connected{break Err("The GPT-Live media connection did not become ready".into());}
         }
        }
    };
    controls.stop.store(true, Ordering::Release);
    sender.abort();
    if result.is_ok() && !protocol.closed {
        emit(Event::Closed);
    }
    result
}
async fn send_microphone(
    capture: Capture,
    track: Arc<TrackLocalStaticSample>,
    peer: Arc<RTCPeerConnection>,
    controls: Arc<Controls>,
    events: asynchronous::UnboundedSender<Raw>,
    dsp: Arc<VoiceDsp>,
) -> Result<(), String> {
    let mut encoder =
        opus::Encoder::new(capture.rate, opus::Channels::Mono, opus::Application::Voip)
            .map_err(|_| "The native microphone Opus encoder could not start")?;
    encoder
        .set_inband_fec(true)
        .map_err(|_| "The microphone Opus resilience settings were refused")?;
    let frame = (capture.rate / 50) as usize;
    let mut samples = VecDeque::new();
    let mut encoded = vec![0; 4096];
    let mut interval = tokio::time::interval(Duration::from_millis(20));
    let mut connected = false;
    let mut ticks = 0;
    while !controls.stop.load(Ordering::Acquire) {
        interval.tick().await;
        if peer.connection_state() != RTCPeerConnectionState::Connected {
            continue;
        }
        let incoming = capture.pcm_drain()?;
        if !connected {
            connected = true;
            samples.clear();
        } else {
            samples.extend(incoming);
        }
        let mut pcm: Vec<i16> = if controls.muted.load(Ordering::Acquire) {
            samples.clear();
            vec![0; frame]
        } else if samples.len() >= frame {
            samples.drain(..frame).collect()
        } else {
            continue;
        };
        dsp.capture(&mut pcm)?;
        if controls.muted.load(Ordering::Acquire) {
            pcm.fill(0);
        }
        let length = encoder
            .encode(&pcm, &mut encoded)
            .map_err(|_| "The microphone audio could not be encoded as Opus")?;
        track
            .write_sample(&Sample {
                data: Bytes::copy_from_slice(&encoded[..length]),
                duration: Duration::from_millis(20),
                ..Default::default()
            })
            .await
            .map_err(|_| "The native microphone RTP stream failed")?;
        ticks += 1;
        if ticks % 5 == 0 {
            let _ = events.send(Raw::Level(f32::from_bits(
                controls.level.load(Ordering::Relaxed),
            )));
        }
    }
    Ok(())
}

struct Speaker {
    controls: Arc<Controls>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Speaker {
    fn drop(&mut self) {
        self.controls.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
fn output(
    receive: Receiver<Vec<f32>>,
    controls: Arc<Controls>,
    events: asynchronous::UnboundedSender<Raw>,
    dsp: Arc<VoiceDsp>,
) -> Result<Speaker, String> {
    let (ready_send, ready) = mpsc::sync_channel(1);
    let thread_controls = controls.clone();
    let handle = std::thread::spawn(move || {
        let controls = thread_controls;
        let stream = match rodio::OutputStreamBuilder::open_default_stream() {
            Ok(stream) => stream,
            Err(_) => {
                let _ = ready_send.send(Err(
                    "No speaker output is available for the voice call".to_owned()
                ));
                return;
            }
        };
        let sink = rodio::Sink::connect_new(stream.mixer());
        sink.append(SpeakerSource {
            receive,
            controls: controls.clone(),
            events,
            pending: VecDeque::new(),
            started: false,
            waited: 0,
            frame: 0,
            peak: 0.0,
            quiet: 0,
            speaking: false,
            reference: RenderReference::new(dsp),
        });
        if ready_send.send(Ok(())).is_err() {
            return;
        }
        while !controls.stop.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(20));
        }
        sink.stop();
        drop(stream);
    });
    ready
        .recv_timeout(Duration::from_secs(15))
        .map_err(|_| "The voice speakers did not become ready")??;
    Ok(Speaker {
        controls,
        handle: Some(handle),
    })
}
struct SpeakerSource {
    receive: Receiver<Vec<f32>>,
    controls: Arc<Controls>,
    events: asynchronous::UnboundedSender<Raw>,
    pending: VecDeque<f32>,
    started: bool,
    waited: u64,
    frame: u64,
    peak: f32,
    quiet: u32,
    speaking: bool,
    reference: RenderReference,
}
impl Iterator for SpeakerSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.controls.stop.load(Ordering::Acquire) {
            return None;
        }
        if self.frame % 960 == 0 {
            for samples in self.receive.try_iter() {
                self.pending.extend(samples);
            }
            if self.pending.len() > 48000 * 2 * 3 {
                self.pending.drain(..self.pending.len() - 48000 * 2);
            }
        }
        if !self.started {
            self.waited += 1;
            if self.pending.len() >= 3 * 960 * 2 || (self.waited > 9600 && !self.pending.is_empty())
            {
                self.started = true;
            }
        }
        let sample = if self.started {
            self.pending.pop_front().unwrap_or(0.0)
        } else {
            0.0
        };
        if let Err(error) = self.reference.sample(sample) {
            self.controls.failed.store(true, Ordering::Release);
            self.controls.stop.store(true, Ordering::Release);
            let _ = self.events.send(Raw::Fatal(error));
            return None;
        }
        self.peak = self.peak.max(sample.abs());
        self.frame += 1;
        if self.frame % 960 == 0 {
            self.quiet = if self.peak > 0.01 { 0 } else { self.quiet + 1 };
            let speaking = self.peak > 0.01 || (self.speaking && self.quiet < 40);
            if speaking != self.speaking {
                self.speaking = speaking;
                let _ = self.events.send(Raw::Speaking(speaking));
            }
            if self.frame % 9600 == 0 {
                let _ = self.events.send(Raw::Level(self.peak));
            }
            self.peak = 0.0;
        }
        Some(sample)
    }
}
impl rodio::Source for SpeakerSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        48000
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests;
