#[path = "external_tests.rs"]
mod external_tests;
#[path = "virtual_peer.rs"]
mod virtual_peer;
use super::*;
use std::{
    io::Cursor,
    time::{Duration, Instant},
};
use symphonia::core::io::MediaSource;
fn tone(rate: u32, channels: u16, seconds: f64) -> Vec<f32> {
    (0..(rate as f64 * seconds) as usize)
        .flat_map(|i| {
            let t = i as f32 / rate as f32;
            let left = (t * std::f32::consts::TAU * 220.).sin() * 0.2;
            let right = (t * std::f32::consts::TAU * 330.).sin() * 0.15;
            (0..channels).map(move |c| if c == 0 { left } else { right })
        })
        .collect()
}
fn wav(channels: u16, seconds: f64) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(
            &mut out,
            hound::WavSpec {
                channels,
                sample_rate: 48000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for value in tone(48000, channels, seconds) {
            writer.write_sample((value * 32767.) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }
    out.into_inner()
}
fn decoder(bytes: Vec<u8>, label: &str) -> decode::AudioDecoder {
    decode::AudioDecoder::open(Box::new(Cursor::new(bytes)) as Box<dyn MediaSource>, label).unwrap()
}
fn decoded(d: &mut decode::AudioDecoder) -> Vec<f32> {
    let mut out = vec![];
    while let Some(samples) = d.next().unwrap() {
        out.extend(samples);
    }
    out
}
fn tempo(samples: &[f32], channels: u16, speed: f32) -> Vec<f32> {
    let mut t = tempo::Tempo::new(48000, channels, speed).unwrap();
    let mut out = vec![];
    for batch in samples.chunks(1024 * channels as usize) {
        t.write(batch).unwrap();
        loop {
            let chunk = t.read();
            if chunk.is_empty() {
                break;
            }
            out.extend(chunk);
        }
    }
    t.finish().unwrap();
    loop {
        let chunk = t.read();
        if chunk.is_empty() {
            break;
        }
        out.extend(chunk);
    }
    assert!(t.read().is_empty());
    out
}
fn frequency(samples: &[f32], channels: usize, channel: usize, expected: f64) -> f64 {
    let frames = samples.len() / channels;
    let start = frames.min(4800);
    let end = frames.saturating_sub(start);
    ((expected as i32 - 8)..=(expected as i32 + 8))
        .map(|hz| {
            let (mut re, mut im) = (0., 0.);
            for frame in start..end {
                let phase = std::f64::consts::TAU * hz as f64 * frame as f64 / 48000.;
                let value = samples[frame * channels + channel] as f64;
                re += value * phase.cos();
                im += value * phase.sin();
            }
            (hz as f64, re * re + im * im)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap()
        .0
}
#[test]
fn real_wav_decode_seek_and_pitch_preserving_tempo_keep_stereo_and_duration() {
    let mut d = decoder(wav(2, 2.), "tone.wav");
    assert_eq!(
        (d.rate, d.channels, d.duration, d.seekable),
        (48000, 2, Some(2.), true)
    );
    let samples = decoded(&mut d);
    assert_eq!(samples.len(), 192000);
    for speed in [0.5, 1., 1.5, 2., 4.] {
        let output = tempo(&samples, 2, speed);
        let expected = samples.len() as f64 / speed as f64;
        assert!(
            (output.len() as f64 - expected).abs() < 4800.,
            "tempo {speed}: {} vs {expected}",
            output.len()
        );
        assert!((frequency(&output, 2, 0, 220.) - 220.).abs() <= 1.);
        assert!((frequency(&output, 2, 1, 330.) - 330.).abs() <= 1.);
    }
    d.seek(1.25).unwrap();
    let rest = decoded(&mut d);
    assert_eq!(rest.len(), 72000);
    assert!((frequency(&rest, 2, 0, 220.) - 220.).abs() <= 1.);
}
fn page(payload: &[u8], granule: u64, sequence: u32, flags: u8) -> Vec<u8> {
    let mut out = b"OggS\0".to_vec();
    out.push(flags);
    out.extend(granule.to_le_bytes());
    out.extend(42u32.to_le_bytes());
    out.extend(sequence.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    let count = payload.len() / 255 + 1;
    out.push(count as u8);
    out.extend(std::iter::repeat_n(255, payload.len() / 255));
    out.push((payload.len() % 255) as u8);
    out.extend(payload);
    let mut crc = 0u32;
    for byte in &out {
        crc ^= (*byte as u32) << 24;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 0x80000000 != 0 { 0x04c11db7 } else { 0 };
        }
    }
    out[22..26].copy_from_slice(&crc.to_le_bytes());
    out
}
fn ogg_opus(gain: i16) -> Vec<u8> {
    let mut encoder =
        opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Audio).unwrap();
    let skip = encoder.get_lookahead().unwrap() as u16;
    let mut head = b"OpusHead\x01\x01".to_vec();
    head.extend(skip.to_le_bytes());
    head.extend(48000u32.to_le_bytes());
    head.extend(gain.to_le_bytes());
    head.push(0);
    let mut out = page(&head, 0, 0, 2);
    let mut tags = b"OpusTags".to_vec();
    tags.extend(0u32.to_le_bytes());
    tags.extend(0u32.to_le_bytes());
    out.extend(page(&tags, 0, 1, 0));
    let samples = tone(48000, 1, 1.);
    let packets = (samples.len() + skip as usize).div_ceil(960);
    for i in 0..packets {
        let mut input = [0.; 960];
        let begin = i * 960;
        let count = samples.len().saturating_sub(begin).min(960);
        input[..count]
            .copy_from_slice(&samples[begin.min(samples.len())..begin.min(samples.len()) + count]);
        let mut encoded = [0u8; 4000];
        let n = encoder.encode_float(&input, &mut encoded).unwrap();
        let last = i + 1 == packets;
        let granule = if last {
            48000 + skip as u64
        } else {
            ((i + 1) * 960) as u64
        };
        out.extend(page(
            &encoded[..n],
            granule,
            (i + 2) as u32,
            if last { 4 } else { 0 },
        ));
    }
    out
}
#[test]
fn actual_ogg_opus_demux_preskip_endtrim_gain_and_seek_are_verified() {
    let mut d = decoder(ogg_opus(0), "tone.opus");
    assert_eq!((d.rate, d.channels, d.duration), (48000, 1, Some(1.)));
    let samples = decoded(&mut d);
    assert_eq!(samples.len(), 48000);
    assert!((frequency(&samples, 1, 0, 220.) - 220.).abs() <= 1.);
    let mut boosted = decoder(ogg_opus(6 * 256), "tone.opus");
    let loud = decoded(&mut boosted);
    let energy = |v: &[f32]| v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
    assert!((energy(&loud) / energy(&samples) - 10f64.powf(0.6)).abs() < 0.02);
    d.seek(0.6).unwrap();
    let rest = decoded(&mut d);
    assert!(
        (rest.len() as i64 - 19200).abs() <= 1,
        "seek frames {}",
        rest.len()
    );
    assert!((frequency(&rest, 1, 0, 220.) - 220.).abs() <= 1.);
    let mut malformed = ogg_opus(0);
    malformed[28 + 18] = 1;
    assert!(decode::AudioDecoder::open(Box::new(Cursor::new(malformed)), "bad.opus").is_err());
}
fn target(bytes: Vec<u8>) -> Target {
    Target {
        owner: DraftScope {
            connection: 4,
            profile: "sample".into(),
            session: Some("owned".into()),
        },
        source: crate::preview::Source::Inline {
            mime: "audio/wav".into(),
            bytes: bytes.into(),
        },
        label: "tone.wav".into(),
        kind: crate::preview::Kind::Audio,
    }
}
fn connection() -> Connection {
    Connection::Url {
        endpoint: "http://127.0.0.1:1".into(),
        token: None,
    }
}
fn wait(players: &mut MediaPlayers, id: PlayerId, predicate: impl Fn(&Snapshot) -> bool) {
    let start = Instant::now();
    loop {
        players.poll();
        let snapshot = players.get(id).unwrap();
        assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
        if predicate(snapshot) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "audio wait: {:?} at {}",
            snapshot.state,
            snapshot.position
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn metadata_never_autoplays_and_retired_owners_are_cancelled() {
    let mut players = MediaPlayers::new();
    players.init(Arc::new(|| {}));
    let id = players.open(connection(), target(wav(1, 1.))).unwrap();
    wait(&mut players, id, |s| s.state == State::Paused);
    assert_eq!(players.get(id).unwrap().position, 0.);
    players.act(id, Action::Seek(0.5));
    wait(&mut players, id, |s| {
        s.position >= 0.5 && s.state == State::Paused
    });
    assert_eq!(players.get(id).unwrap().state, State::Paused);
    let shared = players.players[&id].shared.clone();
    players.close_connection(3);
    assert!(players.get(id).is_some());
    players.close_profile(4, "sample");
    assert!(players.get(id).is_none());
    assert!(shared.cancel.cancelled());
}
#[test]
fn bounded_output_ring_keeps_media_clock_fixed_during_buffering_and_pause() {
    let mut r = output::Ring::new();
    r.reset(1., 48000, 2);
    r.paused = false;
    assert_eq!(output::consume(&mut r, 200), vec![0.; 200]);
    assert_eq!(r.position, 1.);
    assert!(r.underrun);
    r.push(vec![0.2; 960], 2.);
    output::consume(&mut r, 480);
    assert!((r.position - 1.01).abs() < 1e-9);
    r.paused = true;
    output::consume(&mut r, 100);
    assert!((r.position - 1.01).abs() < 1e-9);
    assert_eq!(r.queued, 480);
    r.reset(7., 48000, 2);
    r.paused = false;
    assert_eq!(output::consume(&mut r, 1), vec![0.]);
    assert_eq!(r.position, 7.);
    assert_eq!(r.queued, 0);
}
/// Called only inside the existing strict private Pulse/ALSA child harness.
pub(crate) fn private_device_playback() -> Result<(), String> {
    assert_eq!(
        std::env::var("HERMES_RUNE_VIRTUAL_AUDIO_CHILD").as_deref(),
        Ok("1")
    );
    assert_eq!(
        std::env::var("PULSE_SINK").as_deref(),
        Ok("hermes_rune_test")
    );
    assert_eq!(
        std::env::var("PULSE_SOURCE").as_deref(),
        Ok("hermes_rune_test.monitor")
    );
    let mut players = MediaPlayers::new();
    players.init(Arc::new(|| {}));
    let id = players.open(connection(), target(wav(1, 3.))).unwrap();
    wait(&mut players, id, |s| s.state == State::Paused);
    players.act(id, Action::Rate(2.));
    players.act(id, Action::Play);
    wait(&mut players, id, |s| s.position >= 0.5);
    players.act(id, Action::Pause);
    wait(&mut players, id, |s| s.state == State::Paused);
    let paused = players.get(id).unwrap().position;
    std::thread::sleep(Duration::from_millis(150));
    players.poll();
    assert!((players.get(id).unwrap().position - paused).abs() < 0.03);
    players.act(id, Action::Seek(2.));
    wait(&mut players, id, |s| s.position >= 2.);
    players.act(id, Action::Play);
    wait(&mut players, id, |s| s.state == State::Ended);
    assert_eq!(players.get(id).unwrap().position, 3.);
    players.close(id);
    virtual_peer::seek_cancels_stalled_http_body();
    Ok(())
}
#[test]
fn actual_compressed_mp3_vorbis_flac_and_aac_stereo_containers_decode_without_external_programs() {
    for (label, bytes) in [
        (
            "tone.mp3",
            include_bytes!("../../assets/test-audio/tone.mp3").as_slice(),
        ),
        (
            "tone.ogg",
            include_bytes!("../../assets/test-audio/tone.ogg").as_slice(),
        ),
        (
            "tone.flac",
            include_bytes!("../../assets/test-audio/tone.flac").as_slice(),
        ),
        (
            "tone.m4a",
            include_bytes!("../../assets/test-audio/tone.m4a").as_slice(),
        ),
    ] {
        let mut d = decoder(bytes.to_vec(), label);
        assert_eq!((d.rate, d.channels), (48000, 2), "{label}");
        let samples = decoded(&mut d);
        assert!(
            (samples.len() as i64 - 192000).abs() < 8192,
            "{label}: {} samples",
            samples.len()
        );
        assert!(
            (frequency(&samples, 2, 0, 220.) - 220.).abs() <= 1.,
            "{label}"
        );
        assert!(
            (frequency(&samples, 2, 1, 330.) - 330.).abs() <= 1.,
            "{label}"
        );
        d.seek(1.).unwrap();
        let rest = decoded(&mut d);
        assert!(
            (rest.len() as i64 - 96000).abs() < 8192,
            "{label}: seek {}",
            rest.len()
        );
    }
}
#[test]
fn save_retains_exact_original_bytes_and_cancelled_save_never_replaces_destination() {
    let directory =
        std::env::temp_dir().join(format!("hermes-audio-original-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let destination = directory.join("original.wav");
    let bytes = wav(1, 1.);
    let mut players = MediaPlayers::new();
    players.init(Arc::new(|| {}));
    let id = players.open(connection(), target(bytes.clone())).unwrap();
    players.act(id, Action::SaveOriginal(destination.clone()));
    let start = Instant::now();
    loop {
        players.poll();
        let s = players.get(id).unwrap();
        assert!(s.save_error.is_none(), "{:?}", s.save_error);
        if !s.saving && s.notice.is_some() {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    let shared = players.players[&id].shared.clone();
    shared.cancel.cancel();
    assert!(save::original(connection(), target(vec![1, 2, 3]), &destination, shared).is_err());
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn malformed_wav_metadata_is_rejected_before_the_upstream_parser_allocates() {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend(0x7fff_ffffu32.to_le_bytes());
    bytes.extend(b"WAVELIST");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(b"INFOINAM");
    bytes.extend(0x4000_0000u32.to_le_bytes());
    bytes.extend([0; 4]);
    let result = decode::AudioDecoder::open(Box::new(Cursor::new(bytes)), "hostile.wav");
    assert!(matches!(result, Err(_)));
    let mut bytes = wav(1, 1.);
    bytes[32..34].copy_from_slice(&65535u16.to_le_bytes());
    assert!(matches!(
        decode::AudioDecoder::open(Box::new(Cursor::new(bytes)), "bad-align.wav"),
        Err(_)
    ));
    let mut bytes = wav(1, 1.);
    bytes.truncate(bytes.len() / 2);
    let mut decoder = decoder(bytes, "truncated.wav");
    let mut rejected = false;
    for _ in 0..1000 {
        match decoder.next() {
            Err(error) => {
                assert!(error.contains("truncated"));
                rejected = true;
                break;
            }
            Ok(None) => break,
            Ok(Some(_)) => {}
        }
    }
    assert!(rejected, "Declared WAV tail silently became successful EOF");
}
#[test]
fn paused_volume_and_mute_commands_are_applied_without_another_playback_command() {
    let mut players = MediaPlayers::new();
    players.init(Arc::new(|| {}));
    let id = players.open(connection(), target(wav(1, 1.))).unwrap();
    wait(&mut players, id, |s| s.state == State::Paused);
    for i in 0..32 {
        let volume = if i % 2 == 0 { 0.3 } else { 0.7 };
        let muted = i % 2 == 0;
        players.act(id, Action::Volume(volume));
        players.act(id, Action::Mute(muted));
        wait(&mut players, id, |s| s.volume == volume && s.muted == muted);
        assert_eq!(players.get(id).unwrap().position, 0.);
    }
}
