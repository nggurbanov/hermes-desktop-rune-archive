//! Real native devices. No device is reported ready until CPAL has opened and
//! started it; cancellation releases the microphone independently of HTTP work.
use cpal::Sample;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Default)]
pub struct Controls {
    pub stop: AtomicBool,
    pub cancelled: AtomicBool,
    pub failed: AtomicBool,
    pub muted: AtomicBool,
    pub level: AtomicU32,
}
pub struct Capture {
    pub controls: Arc<Controls>,
    pub samples: Arc<Mutex<Vec<i16>>>,
    pub rate: u32,
    done: Arc<AtomicBool>,
}
impl Capture {
    pub fn open(controls: Arc<Controls>, voice: bool) -> Result<Self, String> {
        Self::open_mode(controls, voice, false)
    }
    pub fn open_live(controls: Arc<Controls>) -> Result<Self, String> {
        Self::open_mode(controls, false, true)
    }
    fn open_mode(controls: Arc<Controls>, voice: bool, continuous: bool) -> Result<Self, String> {
        let (samples_send, samples_receive) = std::sync::mpsc::sync_channel(1);
        let shared = Arc::new(Mutex::new(Vec::<i16>::new()));
        let output = shared.clone();
        let thread_controls = controls.clone();
        let done = Arc::new(AtomicBool::new(false));
        let completed = done.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let host = cpal::default_host();
                let device = host.default_input_device().ok_or(
                    "No microphone is available. Choose an input device in system settings",
                )?;
                let configs = device
                    .supported_input_configs()
                    .map_err(|_| "The microphone formats could not be read")?;
                let supported = configs
                    .filter(|c| {
                        c.channels() > 0
                            && c.min_sample_rate().0 <= 48000
                            && c.max_sample_rate().0 >= 8000
                            && matches!(
                                c.sample_format(),
                                cpal::SampleFormat::F32
                                    | cpal::SampleFormat::I16
                                    | cpal::SampleFormat::U16
                            )
                    })
                    .min_by_key(|c| c.channels())
                    .ok_or("This microphone has no supported speech format")?;
                let rate = 16000u32
                    .min(supported.max_sample_rate().0)
                    .max(supported.min_sample_rate().0);
                let format = supported.sample_format();
                let config = supported.with_sample_rate(cpal::SampleRate(rate)).config();
                let channels = config.channels as usize;
                let mut converter = Downsample::new(rate);
                let output_rate = converter.output_rate;
                let samples = output.clone();
                let state = thread_controls.clone();
                let error_controls = thread_controls.clone();
                let on_error = move |_: cpal::StreamError| {
                    error_controls.failed.store(true, Ordering::Release);
                    error_controls.cancelled.store(true, Ordering::Release);
                    error_controls.stop.store(true, Ordering::Release);
                };
                let stream = match format {
                    cpal::SampleFormat::F32 => device.build_input_stream(
                        &config,
                        move |data: &[f32], _| {
                            push(data, channels, &samples, &state, &mut converter)
                        },
                        on_error,
                        None,
                    ),
                    cpal::SampleFormat::I16 => device.build_input_stream(
                        &config,
                        move |data: &[i16], _| {
                            push(data, channels, &samples, &state, &mut converter)
                        },
                        on_error,
                        None,
                    ),
                    cpal::SampleFormat::U16 => device.build_input_stream(
                        &config,
                        move |data: &[u16], _| {
                            push(data, channels, &samples, &state, &mut converter)
                        },
                        on_error,
                        None,
                    ),
                    _ => return Err("This microphone sample format is not supported".into()),
                }
                .map_err(
                    |_| "The microphone could not be opened. Check system microphone permission",
                )?;
                stream
                    .play()
                    .map_err(|_| "The microphone could not start recording")?;
                if samples_send.send(Ok(output_rate)).is_err() {
                    return Ok(());
                }
                let started = Instant::now();
                let mut heard = false;
                let mut last_loud = Instant::now();
                while !thread_controls.stop.load(Ordering::Acquire) {
                    let level = f32::from_bits(thread_controls.level.load(Ordering::Relaxed));
                    if thread_controls.muted.load(Ordering::Relaxed) {
                        last_loud = Instant::now();
                    }
                    if level > 0.014 {
                        heard = true;
                        last_loud = Instant::now();
                    }
                    if !continuous && started.elapsed() > Duration::from_secs(600)
                        || (voice && heard && last_loud.elapsed() > Duration::from_millis(850))
                    {
                        thread_controls.stop.store(true, Ordering::Release);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                drop(stream);
                Ok(())
            })();
            completed.store(true, Ordering::Release);
            if let Err(error) = result {
                let _ = samples_send.send(Err(error));
            }
        });
        let rate = samples_receive
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| "The microphone did not become ready")??;
        Ok(Self {
            controls,
            samples: shared,
            rate,
            done,
        })
    }
    pub fn pcm_from(&self, cursor: &mut usize) -> Result<Vec<u8>, String> {
        let samples = self
            .samples
            .lock()
            .map_err(|_| "The microphone buffer is unavailable")?;
        let end = samples.len();
        let bytes = samples
            .get(*cursor..end)
            .unwrap_or_default()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        *cursor = end;
        Ok(bytes)
    }
    pub fn wait_stopped(&self) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !self.done.load(Ordering::Acquire) {
            if Instant::now() >= deadline {
                return Err("The microphone did not finish releasing its recording".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
    pub fn pcm_drain(&self) -> Result<Vec<i16>, String> {
        let mut samples = self
            .samples
            .lock()
            .map_err(|_| "The microphone buffer is unavailable")?;
        Ok(std::mem::take(&mut *samples))
    }
    pub fn wav(&self) -> Result<Vec<u8>, String> {
        self.wait_stopped()?;
        let samples = self
            .samples
            .lock()
            .map_err(|_| "The microphone buffer is unavailable")?;
        encode_wav(self.rate, &samples)
    }
}

fn push<T: cpal::Sample>(
    data: &[T],
    channels: usize,
    samples: &Arc<Mutex<Vec<i16>>>,
    controls: &Arc<Controls>,
    converter: &mut Downsample,
) where
    f32: cpal::FromSample<T>,
{
    if controls.stop.load(Ordering::Relaxed) || controls.muted.load(Ordering::Relaxed) {
        controls.level.store(0f32.to_bits(), Ordering::Relaxed);
        return;
    }
    let mut peak = 0f32;
    let mut converted = Vec::with_capacity(data.len() / channels);
    for frame in data.chunks(channels) {
        let value = frame.iter().map(|v| f32::from_sample(*v)).sum::<f32>() / frame.len() as f32;
        peak = peak.max(value.abs());
        if let Some(sample) = converter.push(value) {
            converted.push((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        }
    }
    controls.level.store(peak.to_bits(), Ordering::Relaxed);
    if let Ok(mut samples) = samples.lock() {
        if samples.len() + converted.len() <= 12_500_000 {
            samples.extend(converted);
        } else {
            controls.stop.store(true, Ordering::Release);
        }
    }
}
struct Downsample {
    input_rate: u32,
    output_rate: u32,
    phase: u64,
    sum: f32,
    count: u32,
}
impl Downsample {
    fn new(input_rate: u32) -> Self {
        Self {
            input_rate,
            output_rate: if input_rate >= 16000 { 16000 } else { 8000 },
            phase: 0,
            sum: 0.0,
            count: 0,
        }
    }
    fn push(&mut self, sample: f32) -> Option<f32> {
        self.sum += sample;
        self.count += 1;
        self.phase += self.output_rate as u64;
        if self.phase >= self.input_rate as u64 {
            self.phase -= self.input_rate as u64;
            let value = self.sum / self.count as f32;
            self.sum = 0.0;
            self.count = 0;
            Some(value)
        } else {
            None
        }
    }
}
fn encode_wav(rate: u32, samples: &[i16]) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|_| "The recording could not be encoded")?;
        for sample in samples {
            writer
                .write_sample(*sample)
                .map_err(|_| "The recording could not be encoded")?;
        }
        writer
            .finalize()
            .map_err(|_| "The recording could not be finalized")?;
    }
    Ok(cursor.into_inner())
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.controls.stop.store(true, Ordering::Release);
    }
}

pub fn play(bytes: Vec<u8>, controls: &Arc<Controls>) -> Result<(), String> {
    let stream = rodio::OutputStreamBuilder::open_default_stream()
        .map_err(|_| "No audio output is available. Choose speakers in system settings")?;
    let sink = rodio::Sink::connect_new(stream.mixer());
    let source = rodio::Decoder::try_from(Cursor::new(bytes))
        .map_err(|_| "The synthesized audio format could not be decoded")?;
    sink.append(source);
    while !sink.empty() && !controls.stop.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(20));
    }
    sink.stop();
    drop(stream);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn microphone_resampling_preserves_duration_and_supported_codec_rates() {
        for input in [8000, 11025, 16000, 22050, 44100, 48000] {
            let mut converter = Downsample::new(input);
            let output: Vec<_> = (0..input).filter_map(|_| converter.push(0.25)).collect();
            assert!(matches!(converter.output_rate, 8000 | 16000));
            assert_eq!(output.len(), converter.output_rate as usize);
            assert!(output.iter().all(|sample| (*sample - 0.25).abs() < 1e-6));
        }
    }
    #[test]
    fn native_recording_encoder_preserves_mono_rate_and_signed_samples() {
        let data = encode_wav(16000, &[i16::MIN, 0, i16::MAX]).unwrap();
        let mut reader = hound::WavReader::new(Cursor::new(data)).unwrap();
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 16000);
        assert_eq!(
            reader
                .samples::<i16>()
                .map(Result::unwrap)
                .collect::<Vec<_>>(),
            vec![i16::MIN, 0, i16::MAX]
        );
    }
}

pub struct Playback {
    _stream: rodio::OutputStream,
    sink: rodio::Sink,
}
impl Playback {
    pub fn open() -> Result<Self, String> {
        let stream = rodio::OutputStreamBuilder::open_default_stream()
            .map_err(|_| "No audio output is available. Choose speakers in system settings")?;
        let sink = rodio::Sink::connect_new(stream.mixer());
        Ok(Self {
            _stream: stream,
            sink,
        })
    }
    pub fn append_pcm(&mut self, rate: u32, channels: u16, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() % 2 != 0 || channels == 0 {
            return Err("The speech stream returned invalid PCM audio".into());
        }
        let samples: Vec<f32> = bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect();
        self.sink
            .append(rodio::buffer::SamplesBuffer::new(channels, rate, samples));
        Ok(())
    }
    pub fn wait(&mut self, controls: &Arc<Controls>) {
        while !self.sink.empty() && !controls.stop.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(20));
        }
        if controls.stop.load(Ordering::Acquire) {
            self.sink.stop();
        }
    }
    pub fn stop(&mut self) {
        self.sink.stop();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod virtual_tests;
