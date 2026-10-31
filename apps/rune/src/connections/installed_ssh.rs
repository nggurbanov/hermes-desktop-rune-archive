//! Installed-Hermes SSH: command execution and launcher discovery.
//! This module is deliberately independent of saved-port forwarding and storage.
#[path = "installed_ssh/engine.rs"]
mod engine;
#[path = "installed_ssh/http.rs"]
mod http;
#[path = "installed_ssh/pool.rs"]
mod pool;
pub(crate) use engine::{
    CredentialState, CredentialStore, LifetimeLease, Provenance, Ready, Resource, StoredCredential,
    ownership_id,
};
pub(crate) use pool::Pool;
#[path = "installed_ssh/remote.rs"]
mod remote;
#[cfg(all(test, target_os = "linux"))]
#[path = "installed_ssh/test_fixture.rs"]
pub(crate) mod test_fixture;

use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(crate) struct Spec {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub key_path: Option<PathBuf>,
    pub remote_hermes_path: Option<String>,
    pub remote_profile: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct Cancellation {
    cancelled: Arc<AtomicBool>,
    live: Option<Arc<AtomicBool>>,
}
impl Cancellation {
    pub(crate) fn from_live(live: Arc<AtomicBool>) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            live: Some(live),
        }
    }
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self
                .live
                .as_ref()
                .is_some_and(|live| !live.load(Ordering::Acquire))
    }
}

#[derive(Clone)]
pub(crate) struct Ssh {
    binary: PathBuf,
    spec: Spec,
}
impl Ssh {
    pub(crate) fn new(binary: PathBuf, spec: Spec) -> Result<Self, String> {
        if spec.host.is_empty()
            || spec.host.starts_with('-')
            || spec.host.len() > 1024
            || spec
                .host
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || "@/\\".contains(c))
            || spec.user.len() > 256
            || spec
                .user
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            || spec.port == 0
        {
            return Err("Invalid SSH connection address".into());
        }
        if spec.user.starts_with('-') || spec.user.contains('@') {
            return Err("Invalid SSH connection address".into());
        }
        if let Some(key) = &spec.key_path {
            let text = key.to_str().ok_or("Invalid SSH key path")?;
            if text.starts_with('-') || text.chars().any(char::is_control) {
                return Err("Invalid SSH key path".into());
            }
        }
        if let Some(path) = &spec.remote_hermes_path {
            remote_path(path)?;
        }
        profile_arg(spec.remote_profile.as_deref())?;
        Ok(Self { binary, spec })
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.binary);
        cmd.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ControlMaster=no",
            "-o",
            "ControlPath=none",
            "-o",
            "ControlPersist=no",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "TCPKeepAlive=yes",
        ]);
        if self.spec.port != 22 {
            cmd.arg("-p").arg(self.spec.port.to_string());
        }
        if !self.spec.user.is_empty() {
            cmd.arg("-l").arg(&self.spec.user);
        }
        if let Some(key) = &self.spec.key_path {
            cmd.arg("-i").arg(key);
        }
        cmd.env("LC_ALL", "C");
        cmd
    }
    /// Effective OpenSSH configuration is an opaque identity; do not log or persist it.
    pub(crate) fn effective_identity(&self, cancel: &Cancellation) -> Result<String, String> {
        let mut cmd = self.command();
        cmd.arg("-G").arg("--").arg(&self.spec.host);
        self.run(cmd, None, Duration::from_secs(15), cancel)
    }
    pub(crate) fn exec(
        &self,
        script: &str,
        stdin: Option<&[u8]>,
        timeout: Duration,
        cancel: &Cancellation,
    ) -> Result<String, String> {
        let mut cmd = self.command();
        cmd.args(["-T", "--"]).arg(&self.spec.host).arg(script);
        self.run(cmd, stdin, timeout, cancel)
    }
    fn run(
        &self,
        mut cmd: Command,
        input: Option<&[u8]>,
        timeout: Duration,
        cancel: &Cancellation,
    ) -> Result<String, String> {
        if cancel.cancelled() {
            return Err("SSH operation cancelled".into());
        }
        cmd.stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        // The local SSH invocation and its helpers are an app-owned process group.
        // Killing only ssh leaves inherited pipes and helper processes behind.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd
            .spawn()
            .map_err(|_| "Could not start OpenSSH".to_string())?;
        let (send, receive) = mpsc::channel();
        let writer = input.map(|bytes| {
            let bytes = bytes.to_vec();
            let mut pipe = child.stdin.take().expect("piped stdin");
            let send = send.clone();
            std::thread::spawn(move || {
                let _ = send.send((
                    0,
                    pipe.write_all(&bytes)
                        .map(|_| Vec::new())
                        .map_err(|_| "SSH input failed".to_string()),
                ));
            });
        });
        let out = child.stdout.take().expect("piped stdout");
        let err = child.stderr.take().expect("piped stderr");
        let output_send = send.clone();
        std::thread::spawn(move || {
            let _ = output_send.send((1, read_bounded(out)));
        });
        std::thread::spawn(move || {
            let _ = send.send((2, read_bounded(err)));
        });
        let start = Instant::now();
        let result = loop {
            if cancel.cancelled() || start.elapsed() >= timeout {
                kill_owned_group(&mut child);
                let _ = child.wait();
                break Err(if cancel.cancelled() {
                    "SSH operation cancelled"
                } else {
                    "SSH operation timed out"
                }
                .to_string());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    break if status.success() {
                        Ok(())
                    } else {
                        Err("SSH command failed; check host trust and authentication".into())
                    };
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err("Could not wait for OpenSSH".into());
                }
            }
        };
        result?;
        let mut remaining = 2 + usize::from(writer.is_some());
        let mut output = None;
        while remaining > 0 {
            if cancel.cancelled() || start.elapsed() >= timeout {
                // A descendant may have inherited a pipe after ssh exited. Its
                // process group is still alive while it retains that pipe.
                kill_owned_group(&mut child);
                return Err(if cancel.cancelled() {
                    "SSH operation cancelled"
                } else {
                    "SSH operation timed out"
                }
                .into());
            }
            match receive.recv_timeout(Duration::from_millis(20)) {
                Ok((kind, value)) => {
                    let value = match value {
                        Ok(value) => value,
                        Err(error) => {
                            kill_owned_group(&mut child);
                            return Err(error);
                        }
                    };
                    if kind == 1 {
                        output = Some(value);
                    }
                    remaining -= 1;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err("SSH output failed".into()),
            }
        }
        String::from_utf8(output.unwrap_or_default()).map_err(|_| "SSH output was not UTF-8".into())
    }
    /// Open an SSH-owned loopback forward. The caller must authenticate the
    /// service through it before publishing the endpoint: TCP readiness alone
    /// cannot rule out another process winning the ephemeral-port race.
    pub(crate) fn forward(
        &self,
        remote_port: u16,
        timeout: Duration,
        cancel: &Cancellation,
    ) -> Result<Forward, String> {
        if remote_port == 0 || cancel.cancelled() {
            return Err("Invalid or cancelled SSH forward".into());
        }
        let local_port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|socket| socket.local_addr())
            .map_err(|_| "Could not reserve a local SSH port")?
            .port();
        let mut cmd = self.command();
        cmd.args(["-v", "-o", "ExitOnForwardFailure=yes", "-N", "-L"])
            .arg(format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}"))
            .arg("--")
            .arg(&self.spec.host)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().map_err(|_| "Could not start SSH forward")?;
        let stderr = child.stderr.take().expect("piped SSH diagnostic stream");
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let expected = format!("Local forwarding listening on 127.0.0.1 port {local_port}.");
            let mut line = Vec::new();
            let mut stream = stderr;
            let mut chunk = [0u8; 1024];
            loop {
                let n = match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                for byte in &chunk[..n] {
                    if *byte == b'\n' {
                        if String::from_utf8_lossy(&line).contains(&expected) {
                            let _ = send.send(());
                        }
                        line.clear();
                    } else if line.len() < 4096 {
                        line.push(*byte);
                    }
                }
            }
        });
        let mut forward = Forward {
            child: Some(child),
            local_port,
        };
        let start = Instant::now();
        let mut bound = false;
        loop {
            if cancel.cancelled() || start.elapsed() >= timeout {
                forward.close();
                return Err(if cancel.cancelled() {
                    "SSH operation cancelled"
                } else {
                    "SSH forward timed out"
                }
                .into());
            }
            if let Some(error) = forward.exited() {
                return Err(error);
            }
            bound |= receive.try_recv().is_ok();
            if bound
                && TcpStream::connect_timeout(
                    &SocketAddrV4::new(Ipv4Addr::LOCALHOST, local_port).into(),
                    Duration::from_millis(100),
                )
                .is_ok()
            {
                return Ok(forward);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    pub(crate) fn locate_hermes(&self, cancel: &Cancellation) -> Result<String, String> {
        if let Some(path) = &self.spec.remote_hermes_path {
            if self.executable(path, cancel)? {
                return Ok(path.clone());
            }
            return Err("The configured remote Hermes launcher is not executable".into());
        }
        let mut candidates = Vec::new();
        if let Ok(found) = self.exec(
            "bash -lc 'command -v hermes'",
            None,
            Duration::from_secs(15),
            cancel,
        ) {
            if let Some(last) = found.lines().last() {
                candidates.push(last.trim().to_owned());
            }
        }
        candidates.extend(
            ["~/.local/bin/hermes", "/usr/local/bin/hermes"]
                .into_iter()
                .map(str::to_owned),
        );
        // An explicitly configured remote home is authority for this rung;
        // never infer an install root by searching unrelated directories.
        if let Ok(home) = self.exec(
            "printf '%s' \"${HERMES_HOME-}\"",
            None,
            Duration::from_secs(15),
            cancel,
        ) {
            let home = home.trim_end_matches('/');
            if !home.is_empty()
                && !home.chars().any(char::is_control)
                && !home.split('/').any(|part| part == "." || part == "..")
                && remote_path(home).is_ok()
            {
                let root = match home.rsplit_once("/profiles/") {
                    Some((root, profile)) if !profile.is_empty() && !profile.contains('/') => root,
                    _ => home,
                };
                candidates.push(format!("{root}/hermes-agent/venv/bin/hermes"));
            }
        }
        for path in candidates {
            if cancel.cancelled() {
                return Err("SSH operation cancelled".into());
            }
            if remote_path(&path).is_ok() && self.executable(&path, cancel)? {
                return Ok(path);
            }
        }
        Err("No remote Hermes launcher was found; configure its path or expose hermes in the remote PATH".into())
    }
    fn executable(&self, path: &str, cancel: &Cancellation) -> Result<bool, String> {
        let script = format!("[ -x {} ] && printf OK || true", remote_path(path)?);
        Ok(self
            .exec(&script, None, Duration::from_secs(15), cancel)?
            .trim()
            == "OK")
    }
}
/// The local SSH process group is owned by this handle; the remote service is not.
pub(crate) struct Forward {
    child: Option<Child>,
    local_port: u16,
}
impl Forward {
    pub(crate) fn local_port(&self) -> u16 {
        self.local_port
    }
    pub(crate) fn exited(&mut self) -> Option<String> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(None) => None,
            Ok(Some(_)) | Err(_) => {
                self.close();
                Some("SSH forward exited".into())
            }
        }
    }
    pub(crate) fn close(&mut self) {
        if let Some(mut child) = self.child.take() {
            kill_owned_group(&mut child);
            let _ = child.wait();
        }
    }
}
impl Drop for Forward {
    fn drop(&mut self) {
        self.close();
    }
}
fn kill_owned_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        // process_group(0) established this group for this particular child.
        unsafe {
            kill(-(child.id() as i32), 9);
        }
    }
    let _ = child.kill();
}
fn read_bounded(mut stream: impl Read) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let n = stream
            .read(&mut buf)
            .map_err(|_| "SSH output read failed")?;
        if n == 0 {
            break;
        }
        if result.len() + n > 1024 * 1024 {
            return Err("SSH output exceeded limit".into());
        }
        result.extend_from_slice(&buf[..n]);
    }
    Ok(result)
}
fn quote(s: &str) -> String {
    let mut result = String::from("'");
    for ch in s.chars() {
        match ch {
            '\'' => result.push_str("'\\''"),
            '\\' => result.push_str("'\\\\'"),
            _ => result.push(ch),
        }
    }
    result.push('\'');
    result
}
pub(crate) fn remote_path(path: &str) -> Result<String, String> {
    if path.is_empty() || path.contains(['\0', '\n', '\r']) {
        return Err("Invalid remote path".into());
    }
    if path == "~" {
        Ok("\"$HOME\"".into())
    } else if let Some(rest) = path.strip_prefix("~/") {
        Ok(format!("\"$HOME\"{}", quote(&format!("/{rest}"))))
    } else if path.starts_with('/') {
        Ok(quote(path))
    } else {
        Err("Remote path must be absolute or start with ~/".into())
    }
}
pub(crate) fn profile_arg(profile: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = profile.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let value = value.to_ascii_lowercase();
    if value == "default" {
        return Ok(None);
    }
    if value.len() > 64
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err("Invalid remote profile".into());
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_and_profile_are_safe() {
        assert_eq!(remote_path("~/a'b").unwrap(), "\"$HOME\"'/a'\\''b'");
        assert!(remote_path("relative").is_err());
        assert!(remote_path("/a\nb").is_err());
        assert_eq!(profile_arg(Some(" Default ")).unwrap(), None);
        assert_eq!(
            profile_arg(Some(" My_Profile ")).unwrap(),
            Some("my_profile".into())
        );
        assert!(profile_arg(Some("bad profile")).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn remote_shell_quote_round_trips_quotes_and_backslashes() {
        let value = "space 'quoted' \\ path";
        let output = Command::new("sh")
            .arg("-c")
            .arg(format!("printf %s {}", quote(value)))
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), value);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn inherited_output_pipes_are_bounded_and_owned_helpers_are_killed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rune-ssh-pipes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("ssh");
        std::fs::write(
            &bin,
            "#!/bin/sh\nsleep 30 &\nprintf '%s' $! > \"$0.pid\"\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            bin,
            Spec {
                host: "example".into(),
                user: "".into(),
                port: 22,
                key_path: None,
                remote_hermes_path: None,
                remote_profile: None,
            },
        )
        .unwrap();
        let start = Instant::now();
        let error = ssh
            .exec(
                "ignored",
                None,
                Duration::from_millis(200),
                &Cancellation::default(),
            )
            .unwrap_err();
        assert_eq!(error, "SSH operation timed out");
        assert!(start.elapsed() < Duration::from_secs(3));
        let pid: i32 = std::fs::read_to_string(dir.join("ssh.pid"))
            .unwrap()
            .parse()
            .unwrap();
        // A killed orphan may briefly remain as a zombie until its parent reaps it.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            if status.is_empty() || status.split_whitespace().nth(2) == Some("Z") {
                break;
            }
            assert!(Instant::now() < deadline, "owned helper survived cleanup");
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn fake_forward_is_bounded_and_drop_reaps_its_process_group() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rune-ssh-forward-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("ssh");
        std::fs::write(
            &bin,
            r##"#!/usr/bin/env python3
import os, socket, sys, time
args = sys.argv
spec = args[args.index('-L') + 1]
port = int(spec.split(':')[1])
s = socket.socket()
s.bind(('127.0.0.1', port))
s.listen(4)
print('debug1: Local forwarding listening on 127.0.0.1 port %s.' % port, file=sys.stderr, flush=True)
open(sys.argv[0] + '.pid', 'w').write(str(os.getpid()))
while True:
    conn, _ = s.accept()
    conn.close()
"##,
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            bin.clone(),
            Spec {
                host: "example".into(),
                user: "".into(),
                port: 22,
                key_path: None,
                remote_hermes_path: None,
                remote_profile: None,
            },
        )
        .unwrap();
        let cancel = Cancellation::default();
        let mut forward = ssh.forward(9000, Duration::from_secs(3), &cancel).unwrap();
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, forward.local_port())).is_ok());
        let pid: i32 = std::fs::read_to_string(dir.join("ssh.pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(forward.exited().is_none());
        forward.close();
        forward.close();
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        cancel.cancel();
        assert!(ssh.forward(9000, Duration::from_secs(3), &cancel).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn listening_port_without_ssh_bind_confirmation_is_not_ready() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rune-ssh-unconfirmed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("ssh");
        std::fs::write(&bin, r#"#!/usr/bin/env python3
import socket,sys,time
a=sys.argv;port=int(a[a.index('-L')+1].split(':')[1]);s=socket.socket();s.bind(('127.0.0.1',port));s.listen(1);time.sleep(30)
"#).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            bin,
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
            ssh.forward(9000, Duration::from_millis(300), &Cancellation::default())
                .err()
                .as_deref(),
            Some("SSH forward timed out")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn cancellation_during_forward_reaps_ssh_and_its_children() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rune-ssh-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("ssh");
        std::fs::write(
            &bin,
            r#"#!/bin/sh
sleep 30 &
printf '%s' $! > "$0.pid"
wait
"#,
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            bin,
            Spec {
                host: "example".into(),
                user: "".into(),
                port: 22,
                key_path: None,
                remote_hermes_path: None,
                remote_profile: None,
            },
        )
        .unwrap();
        let cancel = Cancellation::default();
        let other = cancel.clone();
        let pid_file = dir.join("ssh.pid");
        let worker =
            std::thread::spawn(move || ssh.forward(9000, Duration::from_secs(5), &other).err());
        let deadline = Instant::now() + Duration::from_secs(2);
        while !pid_file.exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        cancel.cancel();
        assert_eq!(
            worker.join().unwrap().as_deref(),
            Some("SSH operation cancelled")
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            if status.is_empty() || status.split_whitespace().nth(2) == Some("Z") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "forward helper survived cancellation"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn launcher_fallback_uses_only_explicit_remote_home() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("rune-launcher-home-{}", std::process::id()));
        let install = root.join("custom home");
        let launcher = install.join("hermes-agent/venv/bin/hermes");
        std::fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        std::fs::write(&launcher, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
        let bin = root.join("ssh");
        let home = install.join("profiles/research");
        let script = format!(
            "#!/bin/sh\nHOME={}; HERMES_HOME={}; export HOME HERMES_HOME\nfor arg do last=$arg; done\ncase \"$last\" in \"bash -lc \"*) exit 1;; esac\nexec sh -c \"$last\"\n",
            quote(root.to_str().unwrap()),
            quote(home.to_str().unwrap())
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            bin,
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
            ssh.locate_hermes(&Cancellation::default()).unwrap(),
            launcher.to_str().unwrap()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn fake_ssh_discovers_explicit_launcher_and_respects_cancellation() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "rune-ssh-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("ssh");
        std::fs::write(&bin, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\ncase \"$*\" in *-G*) printf 'hostname effective.example\\nuser remote\\n';; *'[ -x '*) printf OK;; esac\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            bin.clone(),
            Spec {
                host: "alias".into(),
                user: "remote".into(),
                port: 2222,
                key_path: Some("/tmp/key path".into()),
                remote_hermes_path: Some("~/bin/hermes".into()),
                remote_profile: Some("default".into()),
            },
        )
        .unwrap();
        let c = Cancellation::default();
        assert!(
            ssh.effective_identity(&c)
                .unwrap()
                .contains("hostname effective.example")
        );
        assert_eq!(ssh.locate_hermes(&c).unwrap(), "~/bin/hermes");
        let args = std::fs::read_to_string(dir.join("ssh.args")).unwrap();
        assert!(args.contains("StrictHostKeyChecking=accept-new\n"));
        assert!(args.contains("/tmp/key path\n"));
        assert!(args.contains("2222\n"));
        c.cancel();
        assert!(ssh.locate_hermes(&c).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
