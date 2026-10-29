use super::*;
use rodio::Source;
use sha2::{Digest, Sha256};
use std::{
    io::Cursor,
    sync::Mutex,
    time::{Duration, Instant},
};
#[derive(Clone)]
struct FakePlayer(Arc<Mutex<Vec<u8>>>);
impl worker::Player for FakePlayer {
    fn play(&mut self, preset: u8) -> Result<(), String> {
        self.0.lock().unwrap().push(preset);
        Ok(())
    }
}
fn register(
    mixer: &mut worker::Mixer<FakePlayer>,
    client: u64,
    key: &str,
    prefs: Preferences,
) -> (mpsc::Receiver<Reply>, Arc<()>) {
    let (send, receive) = mpsc::channel();
    let alive = Arc::new(());
    mixer.command(
        worker::Command::Register {
            client,
            key: key.into(),
            prefs,
            path: None,
            writable: true,
            replies: send,
            waker: Arc::new(|| {}),
            alive: Arc::downgrade(&alive),
        },
        Instant::now(),
    );
    (receive, alive)
}
fn cue(
    mixer: &mut worker::Mixer<FakePlayer>,
    client: u64,
    key: &str,
    preview: Option<u8>,
    now: Instant,
) {
    mixer.command(
        worker::Command::Cue {
            client,
            key: key.into(),
            preview,
        },
        now,
    );
}
#[test]
fn all_original_assets_decode_as_distinct_finite_stereo_audio_with_provenance() {
    let manifest: Value =
        serde_json::from_str(include_str!("../../assets/completion-sounds/manifest.json")).unwrap();
    let mut hashes = std::collections::HashSet::new();
    assert_eq!(manifest["presets"].as_array().unwrap().len(), 14);
    for (id, name) in PRESETS.iter().copied().skip(1) {
        let bytes = bank::bytes(id).unwrap();
        let decoder = rodio::Decoder::try_from(Cursor::new(bytes)).unwrap();
        assert_eq!(decoder.channels(), 2);
        assert_eq!(decoder.sample_rate(), 48000);
        let samples = decoder.collect::<Vec<_>>();
        assert!(samples.len() > 4800 && samples.len() <= 48000 * 4 * 2);
        assert!(samples.iter().all(|x| x.is_finite() && x.abs() < 1.));
        assert!(samples.iter().any(|x| x.abs() > 0.001));
        let hash = format!("{:x}", Sha256::digest(bytes));
        assert!(hashes.insert(hash.clone()));
        let record = &manifest["presets"][id as usize - 1];
        assert_eq!(record["id"], id);
        assert_eq!(record["name"], name);
        assert_eq!(record["sha256"], hash);
        assert_eq!(
            record["frames"].as_u64().unwrap() as usize,
            samples.len() / 2
        );
    }
    assert!(bank::bytes(0).is_none());
    assert!(bank::bytes(15).is_none());
}
#[test]
fn muted_or_none_window_does_not_steal_the_shared_cue_from_an_audible_peer() {
    let played = Arc::new(Mutex::new(vec![]));
    let mut mixer = worker::Mixer::new(FakePlayer(played.clone()));
    let now = Instant::now();
    let (_quiet, _a) = register(
        &mut mixer,
        1,
        "quiet",
        Preferences {
            muted: true,
            ..Default::default()
        },
    );
    let (_loud, _b) = register(&mut mixer, 2, "audible", Preferences::default());
    cue(&mut mixer, 1, "turn-one", None, now);
    cue(&mut mixer, 2, "turn-one", None, now);
    mixer.flush();
    assert_eq!(*played.lock().unwrap(), vec![1]);
    mixer.command(
        worker::Command::Patch {
            client: 1,
            patch: Patch::Mute(false),
            persist: false,
        },
        now,
    );
    mixer.command(
        worker::Command::Patch {
            client: 1,
            patch: Patch::Preset(0),
            persist: false,
        },
        now,
    );
    cue(&mut mixer, 1, "turn-two", None, now);
    cue(&mut mixer, 2, "turn-two", None, now);
    mixer.flush();
    assert_eq!(*played.lock().unwrap(), vec![1, 1]);
}
#[test]
fn async_claim_rechecks_current_mute_and_selection_and_preview_bypasses_mute() {
    let played = Arc::new(Mutex::new(vec![]));
    let mut mixer = worker::Mixer::new(FakePlayer(played.clone()));
    let now = Instant::now();
    let (_rx, _life) = register(&mut mixer, 1, "device", Preferences::default());
    cue(&mut mixer, 1, "pending", None, now);
    mixer.command(
        worker::Command::Patch {
            client: 1,
            patch: Patch::Mute(true),
            persist: false,
        },
        now,
    );
    mixer.flush();
    assert!(played.lock().unwrap().is_empty());
    cue(&mut mixer, 1, "preview", Some(4), now);
    mixer.flush();
    assert_eq!(*played.lock().unwrap(), vec![4]);
    mixer.command(
        worker::Command::Patch {
            client: 1,
            patch: Patch::Mute(false),
            persist: false,
        },
        now,
    );
    cue(&mut mixer, 1, "route", None, now);
    mixer.command(
        worker::Command::Patch {
            client: 1,
            patch: Patch::Preset(9),
            persist: false,
        },
        now,
    );
    mixer.flush();
    assert_eq!(*played.lock().unwrap(), vec![4, 9]);
    cue(&mut mixer, 1, "none", Some(0), now);
    mixer.flush();
    assert_eq!(*played.lock().unwrap(), vec![4, 9]);
}
#[test]
fn device_patches_merge_across_windows_and_sound_claim_expires_after_one_second() {
    let played = Arc::new(Mutex::new(vec![]));
    let mut mixer = worker::Mixer::new(FakePlayer(played.clone()));
    let now = Instant::now();
    let (rx1, _a) = register(&mut mixer, 1, "same-device", Preferences::default());
    let (rx2, _b) = register(&mut mixer, 2, "same-device", Preferences::default());
    mixer.command(
        worker::Command::Patch {
            client: 1,
            patch: Patch::Preset(3),
            persist: false,
        },
        now,
    );
    mixer.command(
        worker::Command::Patch {
            client: 2,
            patch: Patch::Mute(true),
            persist: false,
        },
        now,
    );
    for rx in [rx1, rx2] {
        let prefs = rx
            .try_iter()
            .filter_map(|reply| {
                if let Reply::Preferences { prefs: p, .. } = reply {
                    Some(p)
                } else {
                    None
                }
            })
            .last()
            .unwrap();
        assert_eq!(prefs.preset, 3);
        assert!(prefs.muted);
    }
    mixer.command(
        worker::Command::Patch {
            client: 2,
            patch: Patch::Mute(false),
            persist: false,
        },
        now,
    );
    cue(&mut mixer, 1, "same-key", None, now);
    cue(&mut mixer, 2, "same-key", None, now);
    mixer.flush();
    assert_eq!(*played.lock().unwrap(), vec![3]);
    cue(
        &mut mixer,
        2,
        "same-key",
        None,
        now + Duration::from_secs(1),
    );
    mixer.flush();
    assert_eq!(*played.lock().unwrap(), vec![3, 3]);
}
#[test]
fn private_sound_preferences_preserve_future_fields_and_bad_files_and_queue_saturation_is_truthful()
{
    let root = std::env::temp_dir().join(format!("hermes-sounds-{}", uuid::Uuid::new_v4()));
    let path = root.join("sounds.json");
    crate::client::private_write(&path, br#"{"version":1,"preset":99,"future":"keep"}"#).unwrap();
    let mut sounds = CompletionSounds::new();
    sounds.load_at(path.clone());
    assert_eq!(sounds.prefs.preset, 1);
    sounds.act(Action::Mute(true), true);
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["muted"], true);
    assert_eq!(saved["future"], "keep");
    std::fs::write(&path, b"broken").unwrap();
    let mut invalid = CompletionSounds::new();
    invalid.load_at(path.clone());
    invalid.act(Action::Preset(3), true);
    assert_eq!(std::fs::read(&path).unwrap(), b"broken");
    assert!(invalid.error.is_some());
    let (send, receive) = mpsc::sync_channel(32);
    for id in 0..32 {
        send.try_send(worker::Command::Unregister(id)).unwrap();
    }
    sounds.native = Some(send.clone());
    sounds.init(Arc::new(|| {}));
    assert!(!sounds.initialized);
    assert!(sounds.error.as_deref().unwrap().contains("busy"));
    for _ in 0..32 {
        receive.try_recv().unwrap();
    }
    sounds.init(Arc::new(|| {}));
    assert!(sounds.initialized);
    receive.try_recv().unwrap();
    for id in 0..32 {
        send.try_send(worker::Command::Unregister(id)).unwrap();
    }
    sounds.act(Action::Preview, false);
    assert!(
        !sounds.previewing,
        "a full queue must not leave a preview stuck pending"
    );
    let previous = sounds.prefs.clone();
    sounds.act(Action::Mute(!previous.muted), false);
    assert_eq!(
        sounds.prefs.muted, previous.muted,
        "A failed queue submission must not claim that mute changed"
    );
    sounds.act(Action::Preset(14), false);
    assert_eq!(
        sounds.prefs.preset, previous.preset,
        "The displayed preset must remain the actor's acknowledged value"
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn sdk_and_native_cues_play_once_with_current_native_prefs_and_speech_keeps_its_own_ttl() {
    let played = Arc::new(Mutex::new(vec![]));
    let mut mixer = worker::Mixer::new(FakePlayer(played.clone()));
    let now = Instant::now();
    let (rx, _life) = register(&mut mixer, 1, "device", Preferences::default());
    let key = serde_json::to_string(&("principal", "work", "sound:runtime-one")).unwrap();
    mixer.command(
        worker::Command::Claim {
            client: 1,
            id: 91,
            key: key.clone(),
            speech: false,
        },
        now,
    );
    cue(&mut mixer, 1, &key, None, now);
    mixer.command(
        worker::Command::Patch {
            client: 1,
            patch: Patch::Preset(4),
            persist: false,
        },
        now,
    );
    mixer.flush();
    assert_eq!(*played.lock().unwrap(), vec![4]);
    assert!(rx.try_iter().any(|reply| matches!(
        reply,
        Reply::Claimed {
            id: 91,
            owns: false
        }
    )));
    let other = serde_json::to_string(&("principal", "work", "sound:runtime-two")).unwrap();
    cue(&mut mixer, 1, &other, None, now);
    mixer.command(
        worker::Command::Claim {
            client: 1,
            id: 92,
            key: other,
            speech: false,
        },
        now,
    );
    mixer.command(
        worker::Command::Patch {
            client: 1,
            patch: Patch::Preset(9),
            persist: false,
        },
        now,
    );
    mixer.flush();
    assert_eq!(*played.lock().unwrap(), vec![4, 9]);
    mixer.command(
        worker::Command::Claim {
            client: 1,
            id: 93,
            key: "sound:pending-muted".into(),
            speech: false,
        },
        now,
    );
    mixer.command(
        worker::Command::Patch {
            client: 1,
            patch: Patch::Mute(true),
            persist: false,
        },
        now,
    );
    mixer.flush();
    assert_eq!(
        *played.lock().unwrap(),
        vec![4, 9],
        "native mute applied after SDK request must suppress pending audio"
    );
    mixer.command(
        worker::Command::Claim {
            client: 1,
            id: 94,
            key: "speech-key".into(),
            speech: true,
        },
        now,
    );
    mixer.command(
        worker::Command::Claim {
            client: 1,
            id: 95,
            key: "speech-key".into(),
            speech: true,
        },
        now + Duration::from_secs(2),
    );
    mixer.command(
        worker::Command::Claim {
            client: 1,
            id: 96,
            key: "speech-key".into(),
            speech: true,
        },
        now + Duration::from_secs(600),
    );
    let claims = rx
        .try_iter()
        .filter_map(|reply| {
            if let Reply::Claimed { id, owns } = reply {
                Some((id, owns))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        claims,
        vec![
            (92, false),
            (93, false),
            (94, true),
            (95, false),
            (96, true)
        ]
    );
}
