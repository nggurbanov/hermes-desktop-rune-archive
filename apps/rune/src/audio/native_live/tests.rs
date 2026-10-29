//! Actual loopback ICE/DTLS/SRTP/SCTP and Opus, with synthetic samples and no
//! device, provider or agent. The media seam is the production peer builder.
use super::*;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_peer_negotiates_data_and_decodes_real_opus_rtp_without_a_browser() {
    let (raw_a, mut events_a) = asynchronous::unbounded_channel();
    let (raw_b, mut events_b) = asynchronous::unbounded_channel();
    let (out_a, _audio_a) = mpsc::sync_channel(128);
    let (out_b, audio_b) = mpsc::sync_channel(128);
    let a = peer(raw_a, out_a, true, true).await.unwrap();
    let mut b = peer(raw_b, out_b, false, true).await.unwrap();
    let local = offer(&a.connection).await.unwrap();
    assert!(local.contains("m=audio"));
    assert!(local.contains("opus/48000"));
    assert!(local.contains("m=application"));
    b.connection
        .set_remote_description(RTCSessionDescription::offer(local).unwrap())
        .await
        .unwrap();
    let mut gathered = b.connection.gathering_complete_promise().await;
    let answer = b.connection.create_answer(None).await.unwrap();
    b.connection.set_local_description(answer).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), gathered.recv())
        .await
        .unwrap();
    let answer = b.connection.local_description().await.unwrap();
    a.connection
        .set_remote_description(RTCSessionDescription::answer(answer.sdp).unwrap())
        .await
        .unwrap();
    let incoming = tokio::time::timeout(Duration::from_secs(5), b.incoming.recv())
        .await
        .unwrap()
        .unwrap();
    let local = a.channel.as_ref().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while local.ready_state() != RTCDataChannelState::Open
        || incoming.ready_state() != RTCDataChannelState::Open
    {
        assert!(
            Instant::now() < deadline,
            "The actual SCTP data channel did not open"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    incoming
        .send_text(json!({"type":"session.started","session":{"id":"synthetic-live"}}).to_string())
        .await
        .unwrap();
    let mut protocol = Protocol::default();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut ready = false;
    while Instant::now() < deadline && !ready {
        if let Ok(Some(Raw::Vendor(value))) =
            tokio::time::timeout(Duration::from_millis(100), events_a.recv()).await
        {
            ready = protocol
                .vendor(&value)
                .iter()
                .any(|e| matches!(e, Event::Ready));
        }
    }
    assert!(ready);
    incoming
        .send_text(
            json!({"type":"session.input_transcript.delta","delta":"Check Hermes","end_ms":100})
                .to_string(),
        )
        .await
        .unwrap();
    incoming
        .send_text(
            json!({"type":"session.delegation.created","delegation":{"id":"delegation-1"}})
                .to_string(),
        )
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut delegated = false;
    while Instant::now() < deadline && !delegated {
        if let Ok(Some(Raw::Vendor(value))) =
            tokio::time::timeout(Duration::from_millis(100), events_a.recv()).await
        {
            delegated = protocol
                .vendor(&value)
                .iter()
                .any(|e| matches!(e,Event::Delegation{text,..}if text=="Check Hermes"));
        }
    }
    assert!(delegated);
    for command in protocol.command(&json!({"type":"speak","text":"Hermes is ready"})) {
        local.send_text(command.to_string()).await.unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut heard = false;
    while Instant::now() < deadline && !heard {
        if let Ok(Some(Raw::Vendor(value))) =
            tokio::time::timeout(Duration::from_millis(100), events_b.recv()).await
        {
            heard = value["type"] == "session.commentary.append"
                && value["delegation_id"] == "delegation-1"
                && value["content"] == "Hermes is ready";
        }
    }
    assert!(heard);
    let mut encoder =
        opus::Encoder::new(16000, opus::Channels::Mono, opus::Application::Voip).unwrap();
    let mut encoded = vec![0; 4096];
    for frame in 0..10 {
        let samples: Vec<i16> = (0..320)
            .map(|n| {
                (((frame * 320 + n) as f32 * std::f32::consts::TAU * 440.0 / 16000.0).sin() * 5000.)
                    as i16
            })
            .collect();
        let count = encoder.encode(&samples, &mut encoded).unwrap();
        a.track
            .write_sample(&Sample {
                data: Bytes::copy_from_slice(&encoded[..count]),
                duration: Duration::from_millis(20),
                ..Default::default()
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut decoded = vec![];
    while decoded.len() < 48000 * 2 / 10 && Instant::now() < deadline {
        for frame in audio_b.try_iter() {
            decoded.extend(frame);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        decoded.len() >= 48000 * 2 / 10,
        "Real Opus RTP must produce stereo PCM at 48 kHz"
    );
    assert!(
        decoded.iter().any(|value| value.abs() > 0.01),
        "Decoded synthetic speech audio must not be silence"
    );
    a.connection.close().await.unwrap();
    b.connection.close().await.unwrap();
    assert_eq!(
        a.connection.connection_state(),
        RTCPeerConnectionState::Closed
    );
}
