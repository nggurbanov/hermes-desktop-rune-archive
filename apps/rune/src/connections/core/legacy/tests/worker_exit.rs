//! Private process-exit contract for connection actor retirement.
use super::*;
use crate::connections::legacy::{Environment, ResolveRequest, SelectionContext};
use std::{
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    process::{Command, Stdio},
    time::Duration,
};

fn request() -> ResolveRequest {
    ResolveRequest {
        door: crate::source::RouteDoor::Legacy,
        desktop_profile: "default".into(),
        context: SelectionContext {
            environment: Environment::captured(None, None),
            primary: None,
            primary_profile: Some("default".into()),
            primary_remote_active: false,
            isolated_backend: false,
            local_home: None,
        },
        primary: None,
        expected: None,
        expected_namespace: None,
        intent: 1,
    }
}
fn process_identity(pid: u64) -> Result<Option<(String, u64, u64)>, String> {
    let path = format!("/proc/{pid}/stat");
    let value = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot inspect fixture process".into()),
    };
    let (_, fields) = value
        .rsplit_once(')')
        .ok_or("Malformed fixture process stat")?;
    let fields: Vec<_> = fields.split_whitespace().collect();
    let state = fields.first().ok_or("Missing process state")?.to_string();
    let pgid = fields
        .get(2)
        .ok_or("Missing process group")?
        .parse()
        .map_err(|_| "Invalid process group")?;
    let ticks = fields
        .get(19)
        .ok_or("Missing process start")?
        .parse()
        .map_err(|_| "Invalid process start")?;
    Ok(Some((state, pgid, ticks)))
}
fn assert_observed_alive(record: &serde_json::Value) {
    let pid = record["pid"].as_u64().unwrap();
    let (state, pgid, ticks) = process_identity(pid).unwrap().unwrap();
    assert_ne!(state, "Z");
    assert_eq!(pgid, record["pgid"].as_u64().unwrap());
    assert_eq!(ticks, record["start_ticks"].as_u64().unwrap());
}
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
struct ForwardFallback(Vec<(u64, u64)>);
impl Drop for ForwardFallback {
    fn drop(&mut self) {
        for (pid, ticks) in &self.0 {
            let _ = Command::new("python3").args([
                "-c", "import os,signal,sys\np=int(sys.argv[1]);t=int(sys.argv[2])\ntry:\n f=os.pidfd_open(p);s=open('/proc/%d/stat'%p).read().rsplit(')',1)[1].split()\n if int(s[19])==t and s[0]!='Z':signal.pidfd_send_signal(f,signal.SIGKILL)\n os.close(f)\nexcept ProcessLookupError:pass",
                &pid.to_string(), &ticks.to_string(),
            ]).status();
        }
    }
}
fn assert_retired(record: &serde_json::Value) {
    let pid = record["pid"].as_u64().unwrap();
    let expected = record["start_ticks"].as_u64().unwrap();
    if let Some((state, _, ticks)) = process_identity(pid).unwrap() {
        assert!(
            ticks != expected || state == "Z",
            "fixture process incarnation is still alive"
        );
    }
}
#[test]
#[ignore = "requires isolated private Secret Service"]
fn application_exit_waits_for_connection_worker_and_owned_ssh_retirement() {
    let home = std::env::var("HOME").unwrap();
    let private_root = std::path::Path::new(&home).parent().unwrap();
    assert!(
        private_root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("rune-private-credentials-")
    );
    assert_eq!(
        std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
        format!("unix:path={}", private_root.join("bus").display())
    );
    if let Ok(registry) = std::env::var("RUNE_PRIVATE_WORKER_REGISTRY") {
        let ssh = std::env::var("RUNE_PRIVATE_WORKER_SSH").unwrap();
        let mut core = Core::system(registry.into());
        core.ssh_binary = ssh.into();
        let (send, requests) = std::sync::mpsc::channel();
        let (replies, _receive) = std::sync::mpsc::channel();
        let (finished, completion) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            crate::connections::worker::run(Ok(core), requests, replies, Arc::new(|| {}));
            let _ = finished.send(());
        });
        let resolver = crate::connections::Resolver::new(send);
        crate::connections::shutdown::track(&resolver, completion);
        let pending = resolver.resolve_legacy(request(), vec![]).unwrap();
        let resolved = pending
            .receiver
            .recv_timeout(Duration::from_secs(20))
            .unwrap()
            .result
            .unwrap();
        let pending_commit = resolver.commit_legacy(resolved, None, 2, vec![]).unwrap();
        let accepted = pending_commit
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .result
            .unwrap();
        let peer = resolver.clone();
        assert!(accepted.lease.is_valid());
        println!("WORKER_READY");
        std::io::stdout().flush().unwrap();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        if std::env::var_os("RUNE_PRIVATE_SKIP_WORKER_DRAIN").is_some() {
            std::process::exit(0);
        }
        crate::connections::drain_workers(Duration::from_secs(20)).unwrap();
        assert!(!accepted.lease.is_valid());
        drop(peer);
        std::process::exit(0);
    }
    let fake = crate::connections::installed_ssh::test_fixture::Fixture::new();
    fs::write(fake.root.join("forward-helper"), b"enabled").unwrap();
    let private = Private::new();
    let mut core = Core::system(private.path.clone());
    let mut ssh = domain::InstalledSsh::new(fake.spec.host.clone());
    ssh.user = fake.spec.user.clone();
    ssh.port = fake.spec.port;
    ssh.remote_hermes_path = fake.spec.remote_hermes_path.clone();
    ssh.remote_profile = fake.spec.remote_profile.clone();
    let scope = Scope::Profile("default".into());
    core.save_legacy(Edit {
        scope,
        expected: absent(),
        value: ValueEdit::Profile(Profile::Ssh(ssh)),
        auth: AuthEdit::default(),
    })
    .unwrap();
    let installation = super::super::installation::load_or_create(&private.path).unwrap();
    let owner = crate::connections::installed_ssh::ownership_id(&installation, "default").unwrap();
    let mut child = ChildGuard(Command::new(std::env::current_exe().unwrap()).args(["connections::core::legacy::tests::worker_exit::application_exit_waits_for_connection_worker_and_owned_ssh_retirement", "--exact", "--ignored", "--nocapture"]).env("RUNE_PRIVATE_WORKER_REGISTRY", &private.path).env("RUNE_PRIVATE_WORKER_SSH", &fake.ssh_binary).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap());
    let mut out = BufReader::new(child.0.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(out.read_line(&mut line).unwrap() > 0);
        if line.contains("WORKER_READY") {
            break;
        }
    }
    let generation = fake.generation(&owner).unwrap();
    let forward: serde_json::Value =
        serde_json::from_slice(&fs::read(fake.root.join("forward.json")).unwrap()).unwrap();
    let _fallback = ForwardFallback(vec![
        (
            forward["pid"].as_u64().unwrap(),
            forward["start_ticks"].as_u64().unwrap(),
        ),
        (
            forward["helper"]["pid"].as_u64().unwrap(),
            forward["helper"]["start_ticks"].as_u64().unwrap(),
        ),
    ]);
    assert_observed_alive(&forward);
    assert_observed_alive(&forward["helper"]);
    let port = u16::try_from(forward["local_port"].as_u64().unwrap()).unwrap();
    assert!(
        TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            Duration::from_millis(200)
        )
        .is_ok()
    );
    child.0.stdin.take().unwrap().write_all(b"exit\n").unwrap();
    assert!(child.0.wait().unwrap().success());
    assert!(generation.files_retired().unwrap());
    assert!(!generation.running().unwrap());
    assert_retired(&forward);
    assert_retired(&forward["helper"]);
    assert!(
        TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            Duration::from_millis(200)
        )
        .is_err()
    );
}
