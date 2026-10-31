use super::super::{Spec, pool::Pool};
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpListener},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    thread,
};

struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    joins: Option<thread::JoinHandle<()>>,
    requests: Arc<AtomicUsize>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.joins.take() {
            join.join().unwrap();
        }
    }
}
fn server(home: PathBuf) -> Server {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let requests = Arc::new(AtomicUsize::new(0));
    let (signal, counter) = (stop.clone(), requests.clone());
    let join = thread::spawn(move || {
        while !signal.load(Ordering::Relaxed) {
            let (mut stream, _) = match listener.accept() {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut peek = [0; 4096];
            let n = stream.peek(&mut peek).unwrap_or(0);
            if n == 0 {
                continue;
            }
            let initial = String::from_utf8_lossy(&peek[..n]);
            if initial.starts_with("GET /api/ws?") {
                let mut ws = tungstenite::accept_hdr(
                    stream,
                    |request: &tungstenite::handshake::server::Request,
                     response: tungstenite::handshake::server::Response| {
                        assert_eq!(request.uri().query(), Some("token=served-token"));
                        Ok(response)
                    },
                )
                .unwrap();
                ws.send(tungstenite::Message::Text("ready".into())).unwrap();
                continue;
            }
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                request.push_str(&line);
            }
            let body = if request.starts_with("GET /api/ssh/ownership") {
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("x-hermes-session-token: served-token")
                );
                let dir = home.join(".hermes/desktop-ssh");
                let record = std::fs::read_dir(dir)
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path()
                    .join("backend.lock.json");
                let lock: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
                format!(
                    r#"{{"ok":true,"sshOwnerNonce":"{}","protocolVersion":1}}"#,
                    lock["spawnNonce"].as_str().unwrap()
                )
            } else {
                counter.fetch_add(1, Ordering::Relaxed);
                r#"window.__HERMES_SESSION_TOKEN__ = "served-token""#.into()
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    Server {
        port,
        stop,
        joins: Some(join),
        requests,
    }
}
fn fixture(label: &str) -> (PathBuf, Ssh, Server) {
    let home = std::env::temp_dir().join(format!("rune-engine-{label}-{}", std::process::id()));
    std::fs::create_dir_all(home.join(".hermes")).unwrap();
    let server = server(home.clone());
    let launcher = home.join("hermes");
    std::fs::write(&launcher,format!("#!/usr/bin/env python3\nimport os,sys,time\nif 'config' in sys.argv: print(os.path.join(os.environ['HOME'],'.hermes','config.yaml'));sys.exit(0)\nif '--help' in sys.argv: print('--ssh-session-token-file --ssh-owner-nonce');sys.exit(0)\nimport subprocess\nchild=subprocess.Popen(['sleep','60'])\nopen(os.path.join(os.environ['HOME'],'descendant.pid'),'w').write(str(child.pid))\nprint('HERMES_BACKEND_READY port={}',flush=True)\ntime.sleep(60)\n",server.port)).unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
    let ssh = home.join("ssh");
    let script = format!(
        r#"#!/usr/bin/env python3
import os,socket,sys,threading,time
os.environ['HOME']={home:?}
os.environ['HERMES_HOME']={hermes_home:?}
a=sys.argv[1:]
if '-G' in a:
 print('hostname fixture.example\nuser remote\nidentityfile /fixture/key');sys.exit(0)
if '-L' in a:
 spec=a[a.index('-L')+1].split(':');local=int(spec[1]);remote=int(spec[3])
 if os.path.exists(os.path.join(os.environ['HOME'],'delay_forward')):
  open(os.path.join(os.environ['HOME'],'forward_pending'),'w').close();time.sleep(10)
 s=socket.socket();s.bind(('127.0.0.1',local));s.listen(10)
 print('debug1: Local forwarding listening on 127.0.0.1 port %s.' % local,file=sys.stderr,flush=True)
 def pump(a,b):
  try:
   while True:
    data=a.recv(8192)
    if not data:break
    b.sendall(data)
  except OSError:pass
  finally:
   try:b.shutdown(socket.SHUT_WR)
   except OSError:pass
 def conn(c):
  try:r=socket.create_connection(('127.0.0.1',remote))
  except OSError:c.close();return
  threading.Thread(target=pump,args=(c,r),daemon=True).start();pump(r,c);c.close();r.close()
 while True:
  c,_=s.accept();threading.Thread(target=conn,args=(c,),daemon=True).start()
if os.path.exists(os.path.join(os.environ['HOME'],'delay_spawn')) and 'subprocess.Popen' in a[-1]:
 open(os.path.join(os.environ['HOME'],'spawn_pending'),'w').close();time.sleep(1)
os.execv('/bin/sh',['sh','-c',a[-1]])
"#,
        home = home.to_str().unwrap(),
        hermes_home = home.join(".hermes").to_str().unwrap()
    );
    std::fs::write(&ssh, script).unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let spec = Spec {
        host: "fixture".into(),
        user: "remote".into(),
        port: 2222,
        key_path: Some(home.join("key")),
        remote_hermes_path: Some(launcher.to_str().unwrap().into()),
        remote_profile: Some("default".into()),
    };
    (home, Ssh::new(ssh, spec).unwrap(), server)
}
fn fixture_owner(ssh: &Ssh, account_home: &Path, owner_id: &str) -> String {
    let launcher = ssh.spec.remote_hermes_path.as_deref().unwrap();
    let (_, remote_home) = remote_homes(ssh, launcher, &Cancellation::default()).unwrap();
    let relative = Path::new(&remote_home)
        .strip_prefix(account_home)
        .unwrap()
        .to_str()
        .unwrap();
    format!("~/{relative}/desktop-ssh/{owner_id}")
}
fn remove(home: &Path) {
    std::fs::remove_dir_all(home).unwrap();
}
#[test]
fn attaches_existing_canonical_and_leaves_it_alive() {
    let (home, ssh, server) = fixture("canonical");
    let ledger = home.join(".hermes/spawn-ledger.json");
    std::fs::write(
        &ledger,
        format!(
            r#"[{{"pid":{},"port":{},"purpose":"serve","host":"127.0.0.1"}}]"#,
            std::process::id(),
            server.port
        ),
    )
    .unwrap();
    let ready = connect(
        ssh,
        "0123456789abcdef0123456789abcdef",
        &Cancellation::default(),
    )
    .unwrap();
    assert!(ready.provenance == Provenance::Canonical);
    assert_eq!(ready.token, "served-token");
    drop(ready);
    assert!(server.requests.load(Ordering::Relaxed) >= 1);
    assert!(ledger.exists());
    drop(server);
    remove(&home);
}
#[test]
fn starts_and_cleans_exact_isolated_generation() {
    let (home, mut ssh, server) = fixture("owned");
    ssh.spec.remote_profile = Some("Research".into());
    let store = test_store();
    let mut ready = connect_with_store(
        ssh.clone(),
        "0123456789abcdef0123456789abcdef",
        store.clone(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(matches!(&ready.provenance, Provenance::Owned { .. }));
    assert_eq!(ready.token, "served-token");
    let owner = std::fs::read_dir(home.join(".hermes/desktop-ssh"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(owner.join("backend.lock.json")).unwrap()).unwrap();
    assert_eq!(record["port"], server.port);
    assert_eq!(record["profile"], "research");
    let pid = record["pid"].as_u64().unwrap();
    let argv = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap();
    let args: Vec<_> = argv.split(|b| *b == 0).collect();
    let profile = args.iter().position(|arg| *arg == b"--profile").unwrap();
    assert_eq!(args[profile + 1], b"research");
    assert_ne!(record["tokenFingerprint"], "");
    assert!(
        connect_with_store(
            ssh.clone(),
            "0123456789abcdef0123456789abcdef",
            test_store(),
            &Cancellation::default()
        )
        .is_err()
    );
    let reused = connect_with_store(
        ssh,
        "0123456789abcdef0123456789abcdef",
        store.clone(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(matches!(
        reused.provenance,
        Provenance::ReusedExternal { .. }
    ));
    drop(reused);
    assert!(owner.join("backend.lock.json").exists());
    let descendant: u32 = std::fs::read_to_string(home.join("descendant.pid"))
        .unwrap()
        .parse()
        .unwrap();
    ready.resource.close().unwrap();
    ready.resource.close().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stat = std::fs::read_to_string(format!("/proc/{descendant}/stat")).unwrap_or_default();
        if stat.is_empty()
            || stat[stat.rfind(')').unwrap() + 2..]
                .split_whitespace()
                .next()
                == Some("Z")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "owned remote descendant survived cleanup"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!owner.join("backend.lock.json").exists());
    drop(ready);
    drop(server);
    remove(&home);
}
#[test]
fn same_identity_shares_lifetime_and_distinct_scope_does_not() {
    let (home, ssh, server) = fixture("pool");
    std::fs::write(
        home.join(".hermes/spawn-ledger.json"),
        format!(
            r#"[{{"pid":{},"port":{},"purpose":"serve","host":"127.0.0.1"}}]"#,
            std::process::id(),
            server.port
        ),
    )
    .unwrap();
    let pool = Pool::default();
    let mut threads = Vec::new();
    for _ in 0..2 {
        let pool = pool.clone();
        let ssh = ssh.clone();
        threads.push(thread::spawn(move || {
            pool.connect(
                ssh,
                "0123456789abcdef0123456789abcdef",
                &Cancellation::default(),
            )
            .unwrap()
        }));
    }
    let a = threads.remove(0).join().unwrap();
    let b = threads.remove(0).join().unwrap();
    assert_eq!(a.endpoint, b.endpoint);
    drop(a);
    let c = pool
        .connect(
            ssh,
            "1123456789abcdef0123456789abcdef",
            &Cancellation::default(),
        )
        .unwrap();
    assert_ne!(b.endpoint, c.endpoint);
    drop(b);
    drop(c);
    drop(server);
    remove(&home);
}
#[test]
fn cancelled_bootstrap_fences_and_removes_its_generation_before_return() {
    let (home, ssh, server) = fixture("cancel");
    let launcher = home.join("hermes");
    std::fs::write(
        &launcher,
        "#!/usr/bin/env python3\nimport os,sys,time\nif 'config' in sys.argv: print(os.path.join(os.environ['HOME'],'.hermes','config.yaml'));sys.exit(0)\nif '--help' in sys.argv: print('--ssh-session-token-file --ssh-owner-nonce');sys.exit(0)\ntime.sleep(15)\n",
    )
    .unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
    let live = Arc::new(AtomicBool::new(true));
    let worker_cancel = Cancellation::from_live(live.clone());
    let pool = Pool::default();
    let worker = thread::spawn(move || {
        pool.connect(ssh, "0123456789abcdef0123456789abcdef", &worker_cancel)
            .err()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let directory = home.join(".hermes/desktop-ssh");
    let owner = loop {
        let candidate = std::fs::read_dir(&directory)
            .ok()
            .and_then(|mut rows| rows.next())
            .and_then(Result::ok)
            .map(|r| r.path());
        if let Some(owner) = candidate {
            if owner.join("backend.lock.json").exists() {
                break owner;
            }
        }
        assert!(
            Instant::now() < deadline,
            "owned generation was not published"
        );
        thread::sleep(Duration::from_millis(10));
    };
    live.store(false, Ordering::Release);
    assert_eq!(
        worker.join().unwrap().as_deref(),
        Some("SSH operation cancelled")
    );
    assert!(!owner.join("backend.lock.json").exists());
    assert!(
        !std::fs::read_dir(&owner)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().ends_with(".token"))
    );
    drop(server);
    remove(&home);
}
#[test]
fn cancelling_one_coalesced_waiter_keeps_other_waiter_alive() {
    let (home, ssh, server) = fixture("partial-cancel");
    let launcher = home.join("hermes");
    std::fs::write(&launcher,format!("#!/usr/bin/env python3\nimport os,sys,time\nif 'config' in sys.argv: print(os.path.join(os.environ['HOME'],'.hermes','config.yaml'));sys.exit(0)\nif '--help' in sys.argv: print('--ssh-session-token-file --ssh-owner-nonce');sys.exit(0)\ntime.sleep(1)\nprint('HERMES_BACKEND_READY port={}',flush=True)\ntime.sleep(15)\n",server.port)).unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
    let first_cancel = Cancellation::default();
    let first_signal = first_cancel.clone();
    let pool = Pool::default();
    let first_pool = pool.clone();
    let second_pool = pool.clone();
    let second_ssh = ssh.clone();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let first_barrier = barrier.clone();
    let second_barrier = barrier.clone();
    let first = thread::spawn(move || {
        first_barrier.wait();
        first_pool
            .connect(ssh, "0123456789abcdef0123456789abcdef", &first_signal)
            .err()
    });
    let second = thread::spawn(move || {
        second_barrier.wait();
        second_pool.connect(
            second_ssh,
            "0123456789abcdef0123456789abcdef",
            &Cancellation::default(),
        )
    });
    barrier.wait();
    let deadline = Instant::now() + Duration::from_secs(10);
    let directory = home.join(".hermes/desktop-ssh");
    let owner = loop {
        let candidate = std::fs::read_dir(&directory)
            .ok()
            .and_then(|mut rows| rows.next())
            .and_then(Result::ok)
            .map(|r| r.path());
        if let Some(owner) = candidate {
            if owner.join("backend.lock.json").exists() {
                break owner;
            }
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    };
    first_cancel.cancel();
    assert_eq!(
        first.join().unwrap().as_deref(),
        Some("SSH operation cancelled")
    );
    let ready = second.join().unwrap().unwrap();
    assert!(owner.join("backend.lock.json").exists());
    drop(ready);
    assert!(!owner.join("backend.lock.json").exists());
    drop(server);
    remove(&home);
}
#[test]
fn cancellation_during_tunnel_reaps_forward_and_owned_dashboard() {
    let (home, ssh, server) = fixture("cancel-tunnel");
    std::fs::write(home.join("delay_forward"), "").unwrap();
    let cancellation = Cancellation::default();
    let other = cancellation.clone();
    let worker =
        thread::spawn(move || connect(ssh, "0123456789abcdef0123456789abcdef", &other).err());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !home.join("forward_pending").exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let owner = std::fs::read_dir(home.join(".hermes/desktop-ssh"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(owner.join("backend.lock.json").exists());
    cancellation.cancel();
    assert_eq!(
        worker.join().unwrap().as_deref(),
        Some("SSH operation cancelled")
    );
    assert!(!owner.join("backend.lock.json").exists());
    drop(server);
    remove(&home);
}
#[test]
fn cancel_during_spawn_publication_waits_for_exact_fence() {
    let (home, ssh, server) = fixture("cancel-publication");
    std::fs::write(home.join("delay_spawn"), "").unwrap();
    let cancellation = Cancellation::default();
    let other = cancellation.clone();
    let pool = Pool::default();
    let worker = thread::spawn(move || {
        pool.connect(ssh, "0123456789abcdef0123456789abcdef", &other)
            .err()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !home.join("spawn_pending").exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    cancellation.cancel();
    assert_eq!(
        worker.join().unwrap().as_deref(),
        Some("SSH operation cancelled")
    );
    let owner = std::fs::read_dir(home.join(".hermes/desktop-ssh"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(!owner.join("backend.lock.json").exists());
    assert!(
        !std::fs::read_dir(&owner)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().ends_with(".token"))
    );
    drop(server);
    remove(&home);
}
#[test]
fn changing_effective_ssh_config_cannot_publish_under_old_identity() {
    let home = std::env::temp_dir().join(format!("rune-engine-identity-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let binary = home.join("ssh");
    std::fs::write(&binary,"#!/bin/sh\nif [ -e \"$0.once\" ]; then printf 'hostname new\\n'; else : > \"$0.once\"; printf 'hostname old\\n'; fi\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let ssh = Ssh::new(
        binary,
        Spec {
            host: "fixture".into(),
            user: "".into(),
            port: 22,
            key_path: None,
            remote_hermes_path: None,
            remote_profile: None,
        },
    )
    .unwrap();
    assert_eq!(
        Pool::default()
            .connect(
                ssh,
                "0123456789abcdef0123456789abcdef",
                &Cancellation::default()
            )
            .err()
            .as_deref(),
        Some("Effective SSH configuration changed while connecting")
    );
    remove(&home);
}
#[test]
fn stable_owner_id_matches_pinned_desktop_installation_scope() {
    assert_eq!(
        ownership_id("123e4567-e89b-42d3-a456-426614174000", "worker").unwrap(),
        "1b7a1f553f93d4860453410ceca1c63c"
    );
    assert!(ownership_id("123e4567-e89b-12d3-a456-426614174000", "worker").is_err());
    assert_ne!(
        ownership_id("123e4567-e89b-42d3-a456-426614174000", "worker").unwrap(),
        ownership_id("123e4567-e89b-42d3-a456-426614174000", "").unwrap()
    );
}
#[test]
fn home_discovery_ignores_sticky_profile_and_preserves_explicit_unicode_home() {
    let root = std::env::temp_dir().join(format!("rune home 漢字-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let launcher = root.join("hermes");
    std::fs::write(&launcher, "#!/usr/bin/env python3\nimport os,sys\nroot=os.path.join(os.environ['HOME'],'.hermes')\nif sys.argv[1:]==['--profile','default','config','path']: print(root+'/config.yaml')\nelse: print(root+'/profiles/sticky/config.yaml')\n").unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
    let custom = root.join("custom foo..bar 漢字");
    let binary = root.join("ssh");
    let script = format!(
        "#!/bin/sh\nHOME={}; HERMES_HOME={}; export HOME HERMES_HOME\nfor arg do last=$arg; done\nexec sh -c \"$last\"\n",
        super::super::quote(root.to_str().unwrap()),
        super::super::quote(custom.to_str().unwrap())
    );
    std::fs::write(&binary, script).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let ssh = Ssh::new(
        binary,
        Spec {
            host: "fixture".into(),
            user: "".into(),
            port: 22,
            key_path: None,
            remote_hermes_path: None,
            remote_profile: None,
        },
    )
    .unwrap();
    let (launch, machine) =
        remote_homes(&ssh, launcher.to_str().unwrap(), &Cancellation::default()).unwrap();
    assert_eq!(launch, custom.to_str().unwrap());
    assert_eq!(machine, root.join(".hermes").to_str().unwrap());
    assert!(checked_home("/valid/../escape").is_err());
    let binary = root.join("ssh-default");
    let script = format!(
        "#!/bin/sh\nHOME={}; export HOME; unset HERMES_HOME\nfor arg do last=$arg; done\nexec sh -c \"$last\"\n",
        super::super::quote(root.to_str().unwrap())
    );
    std::fs::write(&binary, script).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let default_ssh = Ssh::new(
        binary,
        Spec {
            host: "fixture".into(),
            user: "".into(),
            port: 22,
            key_path: None,
            remote_hermes_path: None,
            remote_profile: None,
        },
    )
    .unwrap();
    let (launch, machine) = remote_homes(
        &default_ssh,
        launcher.to_str().unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    assert_eq!(launch, root.join(".hermes").to_str().unwrap());
    assert_eq!(launch, machine);
    remove(&root);
}
struct FailingStore {
    inner: TestStore,
    fail_reserve: bool,
    fail_reconcile: bool,
}
impl CredentialStore for FailingStore {
    fn load(&self, owner: &str) -> Result<CredentialState, String> {
        self.inner.load(owner)
    }
    fn reserve(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        if self.fail_reserve {
            return Err(format!("sensitive {token}"));
        }
        self.inner.reserve(owner, nonce, token)
    }
    fn claim(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        self.inner.claim(owner, nonce, token)
    }
    fn finish(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        self.inner.finish(owner, nonce, token)
    }
    fn activate(
        &self,
        owner: &str,
        nonce: &str,
        token: &str,
    ) -> Result<Box<dyn LifetimeLease>, String> {
        self.inner.activate(owner, nonce, token)
    }
    fn reconcile(&self, owner: &str, nonce: &str, token: &str, served: &str) -> Result<(), String> {
        if self.fail_reconcile {
            return Err(format!("sensitive {token}"));
        }
        self.inner.reconcile(owner, nonce, token, served)
    }
    fn abort(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        self.inner.abort(owner, nonce, token)
    }
}
#[test]
fn secret_store_reconcile_failure_reaps_generation_and_redacts_error() {
    let (home, ssh, server) = fixture("reconcile-fails");
    let store = Arc::new(FailingStore {
        inner: TestStore::default(),
        fail_reserve: false,
        fail_reconcile: true,
    });
    let id = "0123456789abcdef0123456789abcdef";
    let error = connect_with_store(ssh, id, store.clone(), &Cancellation::default())
        .err()
        .unwrap();
    assert_eq!(error, "Could not reconcile SSH credentials");
    assert!(matches!(store.load(id).unwrap(), CredentialState::Missing));
    let owner = home.join(".hermes/desktop-ssh").join(id);
    assert!(!owner.join("backend.lock.json").exists());
    assert!(
        !std::fs::read_dir(owner)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().ends_with(".token"))
    );
    drop(server);
    remove(&home);
}
#[test]
fn stale_scope_rejects_before_remote_spawn_and_redacts_store_error() {
    let (home, ssh, server) = fixture("stale-scope");
    let store = Arc::new(FailingStore {
        inner: TestStore::default(),
        fail_reserve: true,
        fail_reconcile: false,
    });
    let error = connect_with_store(
        ssh,
        "0123456789abcdef0123456789abcdef",
        store,
        &Cancellation::default(),
    )
    .err()
    .unwrap();
    assert_eq!(error, "Could not reserve SSH credentials");
    assert!(!home.join(".hermes/desktop-ssh").exists());
    drop(server);
    remove(&home);
}
#[test]
fn crash_before_spawn_recovers_only_matching_secret_reservation() {
    let (home, ssh, server) = fixture("crash-before");
    let store = Arc::new(TestStore::default());
    let id = "0123456789abcdef0123456789abcdef";
    store
        .reserve(id, "0123456789abcdef", "old-private-token")
        .unwrap();
    let old_token = format!("{}/0123456789abcdef.token", fixture_owner(&ssh, &home, id));
    remote::upload_token(
        &ssh,
        &old_token,
        b"old-private-token",
        &Cancellation::default(),
    )
    .unwrap();
    store.0.lock().unwrap().get_mut(id).unwrap().1 = false;
    let mut ready = connect_with_store(ssh, id, store.clone(), &Cancellation::default()).unwrap();
    assert!(
        !home
            .join(".hermes/desktop-ssh")
            .join(id)
            .join("0123456789abcdef.token")
            .exists()
    );
    let saved = match store.load(id).unwrap() {
        CredentialState::Ready(saved) => saved,
        _ => panic!("missing fixture credential"),
    };
    assert_ne!(saved.nonce, "0123456789abcdef");
    assert_eq!(saved.served_token.as_deref(), Some("served-token"));
    ready.resource.close().unwrap();
    assert!(matches!(store.load(id).unwrap(), CredentialState::Missing));
    drop(ready);
    drop(server);
    remove(&home);
}
#[test]
fn crash_after_spawn_recovers_port_zero_and_published_generation() {
    for (label, published) in [("crash-port-zero", false), ("crash-published", true)] {
        let (home, ssh, server) = fixture(label);
        let store = Arc::new(TestStore::default());
        let id = "0123456789abcdef0123456789abcdef";
        let nonce = "0123456789abcdef";
        let owner = fixture_owner(&ssh, &home, id);
        let marker = home.join(".hermes/.hermes-update-in-progress");
        let launcher = home.join("hermes");
        let home_path = home.join(".hermes");
        let token_path = format!("{owner}/{nonce}.token");
        let cancel = Cancellation::default();
        store.reserve(id, nonce, "spawn-token").unwrap();
        remote::upload_token(&ssh, &token_path, b"spawn-token", &cancel).unwrap();
        remote::spawn_dashboard(
            &ssh,
            marker.to_str().unwrap(),
            launcher.to_str().unwrap(),
            &owner,
            nonce,
            None,
            home_path.to_str().unwrap(),
            &cancel,
        )
        .unwrap();
        if published {
            store
                .reconcile(id, nonce, "spawn-token", "served-token")
                .unwrap();
            remote::publish_dashboard(&ssh, &owner, nonce, server.port, "served-token", &cancel)
                .unwrap();
        }
        store.0.lock().unwrap().get_mut(id).unwrap().1 = false;
        let reused = connect_with_store(ssh.clone(), id, store.clone(), &cancel).unwrap();
        assert!(matches!(
            reused.provenance,
            Provenance::ReusedExternal { .. }
        ));
        assert_eq!(reused.token, "served-token");
        let record: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                home.join(".hermes/desktop-ssh")
                    .join(id)
                    .join("backend.lock.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(record["port"], server.port);
        drop(reused);
        assert!(
            !home
                .join(".hermes/desktop-ssh")
                .join(id)
                .join("backend.lock.json")
                .exists()
        );
        assert!(matches!(store.load(id).unwrap(), CredentialState::Missing));
        drop(server);
        remove(&home);
    }
}
#[test]
fn peer_os_process_shared_lease_fences_last_cleanup_and_recovers_after_crash() {
    use std::process::{Command, Stdio};
    let (home, ssh, server) = fixture("peer-lease");
    let store = Arc::new(TestStore::default());
    let id = "0123456789abcdef0123456789abcdef";
    let mut owner =
        connect_with_store(ssh.clone(), id, store.clone(), &Cancellation::default()).unwrap();
    let owner_dir = home.join(".hermes/desktop-ssh").join(id);
    let code = "import fcntl,sys\nf=open(sys.argv[1],'a+')\nfcntl.flock(f,fcntl.LOCK_SH)\nprint('READY',flush=True)\nsys.stdin.read()\n";
    let mut peer = Command::new("python3")
        .args(["-c", code, store.1.join(id).to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(peer.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "READY");
    owner.resource.close().unwrap();
    assert!(owner_dir.join("backend.lock.json").exists());
    assert!(matches!(store.load(id).unwrap(), CredentialState::Ready(_)));
    // OS releases the crashed peer's shared file description automatically.
    peer.kill().unwrap();
    peer.wait().unwrap();
    let mut successor =
        connect_with_store(ssh, id, store.clone(), &Cancellation::default()).unwrap();
    assert!(matches!(
        successor.provenance,
        Provenance::ReusedExternal { .. }
    ));
    successor.resource.close().unwrap();
    assert!(!owner_dir.join("backend.lock.json").exists());
    assert!(matches!(store.load(id).unwrap(), CredentialState::Missing));
    drop(owner);
    drop(successor);
    drop(server);
    remove(&home);
}
#[test]
fn independent_pools_keep_generation_until_the_last_shared_holder() {
    let (home, ssh, server) = fixture("independent-pools");
    let store = Arc::new(TestStore::default());
    let id = "0123456789abcdef0123456789abcdef";
    let pool_a = Pool::default();
    let pool_b = Pool::default();
    let mut a = pool_a
        .connect_with_store(ssh.clone(), id, store.clone(), &Cancellation::default())
        .unwrap();
    let mut b = pool_b
        .connect_with_store(ssh, id, store.clone(), &Cancellation::default())
        .unwrap();
    let lock = home
        .join(".hermes/desktop-ssh")
        .join(id)
        .join("backend.lock.json");
    assert_ne!(a.endpoint, b.endpoint);
    a.resource.close().unwrap();
    assert!(lock.exists());
    b.resource.close().unwrap();
    assert!(!lock.exists());
    assert!(matches!(store.load(id).unwrap(), CredentialState::Missing));
    drop(a);
    drop(b);
    drop(server);
    remove(&home);
}
#[test]
fn legacy_external_owner_is_never_killed_without_supervisor_proof() {
    let (home, ssh, server) = fixture("legacy-external");
    let store = Arc::new(TestStore::default());
    let id = "0123456789abcdef0123456789abcdef";
    let mut owner =
        connect_with_store(ssh.clone(), id, store.clone(), &Cancellation::default()).unwrap();
    let record_path = home
        .join(".hermes/desktop-ssh")
        .join(id)
        .join("backend.lock.json");
    let original = std::fs::read(&record_path).unwrap();
    let mut record: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let nonce = record["spawnNonce"].as_str().unwrap().to_string();
    record.as_object_mut().unwrap().remove("supervisorProtocol");
    record.as_object_mut().unwrap().remove("startTicks");
    record.as_object_mut().unwrap().remove("creationTime");
    std::fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    struct Guard<'a> {
        path: &'a Path,
        bytes: &'a [u8],
        ssh: &'a Ssh,
        owner: String,
        nonce: String,
    }
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            if self.path.exists() {
                let _ = std::fs::write(self.path, self.bytes);
                let _ = remote::cleanup_dashboard(self.ssh, &self.owner, &self.nonce);
            }
        }
    }
    let owner_path = fixture_owner(&ssh, &home, id);
    let guard = Guard {
        path: &record_path,
        bytes: &original,
        ssh: &ssh,
        owner: owner_path.clone(),
        nonce: nonce.clone(),
    };
    let mut reused =
        connect_with_store(ssh.clone(), id, store.clone(), &Cancellation::default()).unwrap();
    owner.resource.close().unwrap();
    assert!(record_path.exists());
    reused.resource.close().unwrap();
    assert!(record_path.exists());
    std::fs::write(&record_path, &original).unwrap();
    remote::cleanup_dashboard(&ssh, &owner_path, &nonce).unwrap();
    let saved = match store.load(id).unwrap() {
        CredentialState::Ready(saved) => saved,
        _ => panic!("missing fixture credential"),
    };
    store.abort(id, &nonce, &saved.spawn_token).unwrap();
    drop(guard);
    drop(owner);
    drop(reused);
    drop(server);
    remove(&home);
}
#[test]
fn last_holder_recovers_owned_remote_tree_after_supervisor_is_killed() {
    use std::process::{Command, Stdio};
    let (home, ssh, server) = fixture("supervisor-loss");
    let store = Arc::new(TestStore::default());
    let id = "0123456789abcdef0123456789abcdef";
    let cleanup_ssh = ssh.clone();
    let mut ready = connect_with_store(ssh, id, store.clone(), &Cancellation::default()).unwrap();
    let owner = home.join(".hermes/desktop-ssh").join(id);
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(owner.join("backend.lock.json")).unwrap()).unwrap();
    struct RecoveryGuard {
        ssh: Ssh,
        marker: String,
        launcher: String,
        owner: String,
        home: String,
        nonce: String,
    }
    impl Drop for RecoveryGuard {
        fn drop(&mut self) {
            let _ = remote::reclaim_dead_generation(
                &self.ssh,
                &self.marker,
                &self.launcher,
                &self.owner,
                &self.home,
                None,
                Some(&self.nonce),
                &Cancellation::default(),
            );
        }
    }
    let owner_path = fixture_owner(&cleanup_ssh, &home, id);
    let _guard = RecoveryGuard {
        ssh: cleanup_ssh,
        marker: home
            .join(".hermes/.hermes-update-in-progress")
            .to_str()
            .unwrap()
            .into(),
        launcher: home.join("hermes").to_str().unwrap().into(),
        owner: owner_path,
        home: home.join(".hermes").to_str().unwrap().into(),
        nonce: record["spawnNonce"].as_str().unwrap().into(),
    };
    let supervisor = record["pid"].as_u64().unwrap();
    let descendant: u32 = std::fs::read_to_string(home.join("descendant.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let killed=Command::new("python3").args(["-c","import os,signal,sys;fd=os.pidfd_open(int(sys.argv[1]));signal.pidfd_send_signal(fd,signal.SIGKILL);os.close(fd)",&supervisor.to_string()]).stderr(Stdio::null()).status().unwrap();
    assert!(killed.success());
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stat = std::fs::read_to_string(format!("/proc/{supervisor}/stat")).unwrap_or_default();
        if stat.is_empty()
            || stat[stat.rfind(')').unwrap() + 2..]
                .split_whitespace()
                .next()
                == Some("Z")
        {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    ready.resource.close().unwrap();
    assert!(!owner.join("backend.lock.json").exists());
    assert!(matches!(store.load(id).unwrap(), CredentialState::Missing));
    let stat = std::fs::read_to_string(format!("/proc/{descendant}/stat")).unwrap_or_default();
    assert!(
        stat.is_empty()
            || stat[stat.rfind(')').unwrap() + 2..]
                .split_whitespace()
                .next()
                == Some("Z")
    );
    drop(ready);
    drop(server);
    remove(&home);
}
struct PrewriteLockStore {
    inner: TestStore,
    lock_path: PathBuf,
    loads: AtomicUsize,
}
impl CredentialStore for PrewriteLockStore {
    fn load(&self, owner: &str) -> Result<CredentialState, String> {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        self.loads.fetch_add(1, Ordering::Relaxed);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.lock_path)
            .map_err(|_| "lock open")?;
        if unsafe { flock(file.as_raw_fd(), 2 | 4) } != 0 {
            return if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                Ok(CredentialState::Pending)
            } else {
                Err("lock probe".into())
            };
        }
        if unsafe { flock(file.as_raw_fd(), 8) } != 0 {
            return Err("unlock".into());
        }
        self.inner.load(owner)
    }
    fn reserve(&self, o: &str, n: &str, t: &str) -> Result<(), String> {
        self.inner.reserve(o, n, t)
    }
    fn claim(&self, o: &str, n: &str, t: &str) -> Result<(), String> {
        self.inner.claim(o, n, t)
    }
    fn finish(&self, o: &str, n: &str, t: &str) -> Result<(), String> {
        self.inner.finish(o, n, t)
    }
    fn activate(&self, o: &str, n: &str, t: &str) -> Result<Box<dyn LifetimeLease>, String> {
        self.inner.activate(o, n, t)
    }
    fn reconcile(&self, o: &str, n: &str, t: &str, s: &str) -> Result<(), String> {
        self.inner.reconcile(o, n, t, s)
    }
    fn abort(&self, o: &str, n: &str, t: &str) -> Result<(), String> {
        self.inner.abort(o, n, t)
    }
}
#[test]
fn peer_bootstrap_lock_before_first_secret_is_pending_not_missing() {
    use std::process::{Command, Stdio};
    let (home, ssh, server) = fixture("prewrite-lock");
    let lock_path = home.join("bootstrap.lock");
    std::fs::write(&lock_path, "").unwrap();
    let store = Arc::new(PrewriteLockStore {
        inner: TestStore::default(),
        lock_path: lock_path.clone(),
        loads: AtomicUsize::new(0),
    });
    let code = "import fcntl,sys\nf=open(sys.argv[1],'r+')\nfcntl.flock(f,fcntl.LOCK_EX)\nprint('READY',flush=True)\nsys.stdin.read()\n";
    let mut peer = Command::new("python3")
        .args(["-c", code, lock_path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(peer.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "READY");
    assert!(matches!(
        store.load("0123456789abcdef0123456789abcdef").unwrap(),
        CredentialState::Pending
    ));
    let cancel = Cancellation::default();
    let other = cancel.clone();
    let pool = Pool::default();
    let worker_pool = pool.clone();
    let worker_store = store.clone();
    let worker_ssh = ssh.clone();
    let worker = thread::spawn(move || {
        worker_pool
            .connect_with_store(
                worker_ssh,
                "0123456789abcdef0123456789abcdef",
                worker_store,
                &other,
            )
            .err()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while store.loads.load(Ordering::Relaxed) < 3 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!home.join(".hermes/desktop-ssh").exists());
    cancel.cancel();
    assert_eq!(
        worker.join().unwrap().as_deref(),
        Some("SSH operation cancelled")
    );
    assert!(!home.join(".hermes/desktop-ssh").exists());
    drop(peer.stdin.take());
    assert!(peer.wait().unwrap().success());
    assert!(matches!(
        store.load("0123456789abcdef0123456789abcdef").unwrap(),
        CredentialState::Missing
    ));
    let mut ready = pool
        .connect_with_store(
            ssh,
            "0123456789abcdef0123456789abcdef",
            store.clone(),
            &Cancellation::default(),
        )
        .unwrap();
    ready.resource.close().unwrap();
    drop(ready);
    drop(server);
    remove(&home);
}
