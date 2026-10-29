//! Explicit device acceptance proof. The parent launches an empty private Pulse
//! daemon with only a null sink, then a child process whose ALSA default routes
//! solely to that daemon. No hardware module or existing user server is used.
use super::*;
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
};
const SOURCE: &str = "hermes_rune_test.monitor";
const SINK: &str = "hermes_rune_test";
struct Server {
    child: Child,
    directory: PathBuf,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}
#[test]
#[ignore = "Explicit virtual-device acceptance: HERMES_RUNE_VIRTUAL_AUDIO=1; owns a private Pulse null sink, never hardware"]
fn native_virtual_device_roundtrip() {
    assert_eq!(
        std::env::var("HERMES_RUNE_VIRTUAL_AUDIO").as_deref(),
        Ok("1"),
        "This device test requires an explicit opt-in"
    );
    if std::env::var("HERMES_RUNE_VIRTUAL_AUDIO_CHILD").as_deref() == Ok("1") {
        assert_eq!(std::env::var("PULSE_SOURCE").as_deref(), Ok(SOURCE));
        assert_eq!(std::env::var("PULSE_SINK").as_deref(), Ok(SINK));
        let server = std::env::var("PULSE_SERVER").unwrap();
        let config = std::env::var("ALSA_CONFIG_PATH").unwrap();
        assert!(
            server.starts_with("unix:/tmp/hermes-rune-virtual-") && server.ends_with("/pulse.sock")
        );
        assert!(
            config.starts_with("/tmp/hermes-rune-virtual-") && config.ends_with("/asound.conf")
        );
        let controls = Arc::new(Controls::default());
        let capture =
            Capture::open(controls.clone(), false).expect("Open private virtual CPAL capture");
        assert!(!controls.failed.load(Ordering::Acquire));
        std::thread::sleep(Duration::from_millis(150));
        let samples: Vec<i16> = (0..16000)
            .map(|i| {
                let time = i as f32 / 16000.;
                ((time * std::f32::consts::TAU * 440.).sin() * 12000.
                    + (time * std::f32::consts::TAU * 660.).sin() * 4000.) as i16
            })
            .collect();
        let audio = encode_wav(16000, &samples).unwrap();
        play(audio, &Arc::new(Controls::default()))
            .expect("Play through private virtual Rodio output");
        std::thread::sleep(Duration::from_millis(250));
        controls.stop.store(true, Ordering::Release);
        capture.wait_stopped().unwrap();
        let samples = capture.pcm_drain().unwrap();
        assert!(
            samples.len() >= capture.rate as usize / 2,
            "Native capture returned too little PCM"
        );
        let peak = samples
            .iter()
            .map(|v| i32::from(*v).abs())
            .max()
            .unwrap_or(0);
        assert!(
            peak > 3000,
            "No generated speaker audio reached the private monitor: peak={peak}"
        );
        let power = samples
            .iter()
            .map(|v| (f64::from(*v) / 32768.).powi(2))
            .sum::<f64>()
            / samples.len() as f64;
        assert!(
            power > 0.001,
            "Virtual speaker/capture roundtrip was silent: {power}"
        );
        assert!(!controls.failed.load(Ordering::Acquire));
        eprintln!(
            "Private CPAL/Rodio roundtrip: {} samples @{}Hz, peak={peak}, mean power={power:.5}",
            samples.len(),
            capture.rate
        );
        return;
    }
    let directory = PathBuf::from(format!("/tmp/hermes-rune-virtual-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.join("pulse.sock");
    let alsa = directory.join("asound.conf");
    fs::write(&alsa,format!("pcm.!default {{ type pulse server \"unix:{}\" }}\nctl.!default {{ type pulse server \"unix:{}\" }}\n",socket.display(),socket.display())).unwrap();
    let log = fs::File::create(directory.join("pulse.log")).unwrap();
    let child = Command::new("pulseaudio")
        .args([
            "--daemonize=no",
            "--exit-idle-time=-1",
            "--use-pid-file=no",
            "-n",
            "--log-target=stderr",
        ])
        .arg(format!(
            "--load=module-native-protocol-unix socket={} auth-anonymous=1",
            socket.display()
        ))
        .arg(format!(
            "--load=module-null-sink sink_name={SINK} rate=48000 channels=2"
        ))
        .env("XDG_RUNTIME_DIR", &directory)
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}/unused-bus", directory.display()),
        )
        .env_remove("DISPLAY")
        .env_remove("PULSE_SERVER")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .expect("Install pulseaudio to run this explicit virtual-device test");
    let mut server = Server { child, directory };
    let start = Instant::now();
    while !socket.exists() {
        assert!(
            server.child.try_wait().unwrap().is_none(),
            "Private Pulse daemon failed: {}",
            fs::read_to_string(server.directory.join("pulse.log")).unwrap_or_default()
        );
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "Private Pulse daemon did not become ready"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "audio::device::virtual_tests::native_virtual_device_roundtrip",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HERMES_RUNE_VIRTUAL_AUDIO_CHILD", "1")
        .env("PULSE_SERVER", format!("unix:{}", socket.display()))
        .env("PULSE_SOURCE", SOURCE)
        .env("PULSE_SINK", SINK)
        .env("ALSA_CONFIG_PATH", &alsa)
        .env("XDG_RUNTIME_DIR", &server.directory)
        .status()
        .unwrap();
    assert!(status.success(), "Private virtual-device child failed");
}
