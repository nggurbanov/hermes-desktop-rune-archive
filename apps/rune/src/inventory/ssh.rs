use crate::{connections::Ssh, gateway::roster::valid_profile};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub(crate) struct Inventory {
    pub profiles: Vec<String>,
    pub install_id: Option<String>,
}
// Adapted from retained remote-lifecycle.ts: inspect the remote environment's
// home, profile directories and existing install identity. No Python imports,
// launcher, dashboard, forward, filesystem write or identity minting occurs.
const PROGRAM: &str = r#"os=$(uname -s 2>/dev/null) || exit 41
case "$os" in Linux|Darwin) ;; *) exit 41 ;; esac
home=${HERMES_HOME:-"$HOME/.hermes"}
case "$home" in /*) ;; *) exit 43 ;; esac
case "$home" in *'/../'*|*'/..') exit 43 ;; esac
[ -d "$home" ] || exit 42
printf 'HERMES_INVENTORY_V1\nP\tdefault\n'
for directory in "$home"/profiles/*; do
  [ -d "$directory" ] || continue
  name=${directory##*/}
  case "$name" in ''|[!A-Za-z0-9]*|*[!A-Za-z0-9_-]*) continue ;; esac
  [ "${#name}" -le 64 ] || continue
  printf 'P\t%s\n' "$name"
done
root=$home
parent=${home%/*}
[ "${parent##*/}" != profiles ] || root=${parent%/*}
if [ -f "$root/install_id" ]; then
  IFS= read -r identity < "$root/install_id" || :
  printf 'I\t%s\n' "$identity"
fi
"#;

pub(crate) fn command(spec: &Ssh) -> Result<Command, String> {
    // Match the saved forward's host/key validation, without its -N/-L flags.
    if spec.host.is_empty()
        || spec.host.starts_with('-')
        || spec.host.len() > 1024
        || spec.host.contains(['@', '/', '\\'])
        || spec
            .host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
        || spec
            .user
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
        || spec.user.len() > 256
        || spec.port == 0
    {
        return Err("The SSH inventory target is invalid".into());
    }
    let mut command = Command::new("ssh");
    command.args([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-o",
        "ControlPersist=no",
        "-o",
        "ConnectTimeout=5",
    ]);
    command.arg("-p").arg(spec.port.to_string());
    if !spec.user.is_empty() {
        command.arg("-l").arg(&spec.user);
    }
    if let Some(key) = &spec.key_file {
        command
            .arg("-i")
            .arg(key)
            .args(["-o", "IdentitiesOnly=yes"]);
    }
    command
        .arg("--")
        .arg(&spec.host)
        .arg("sh -s")
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(command)
}

#[derive(Default)]
pub(crate) struct Children {
    state: std::sync::Mutex<ChildrenState>,
    sequence: std::sync::atomic::AtomicU64,
}
#[derive(Default)]
struct ChildrenState {
    closed: bool,
    children: std::collections::BTreeMap<u64, Arc<std::sync::Mutex<OwnedProcess>>>,
}
struct Registered {
    manager: Arc<Children>,
    key: u64,
    child: Arc<std::sync::Mutex<OwnedProcess>>,
}
struct OwnedProcess {
    child: std::process::Child,
    reaped: Option<std::process::ExitStatus>,
}
impl OwnedProcess {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn exited(&self) -> std::io::Result<bool> {
        if self.reaped.is_some() {
            return Ok(true);
        }
        posix::exited(&self.child)
    }
    fn stop(&mut self) -> std::io::Result<std::process::ExitStatus> {
        if let Some(status) = self.reaped {
            return Ok(status);
        }
        // The leader stays unreaped until its entire owned group is signalled;
        // its PID therefore cannot be reused to address an unrelated group.
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let signalled = posix::kill_group(&self.child);
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let signalled = self.child.kill();
        if signalled.is_err() {
            let _ = self.child.kill();
        }
        let status = self.child.wait()?;
        self.reaped = Some(status);
        signalled?;
        Ok(status)
    }
}
impl Children {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.state.lock().unwrap().children.is_empty()
    }
    pub fn reopen(&self) {
        self.state.lock().unwrap().closed = false;
    }
    fn spawn(
        self: &Arc<Self>,
        command: Command,
        cancelled: &AtomicBool,
    ) -> Result<Registered, String> {
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (command, cancelled);
            return Err("Passive SSH inventory needs owned process-tree support on this platform; Windows Job-object support is not implemented".into());
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::unix::process::CommandExt;
            let mut command = command;
            command.process_group(0);
            let mut state = self.state.lock().unwrap();
            if state.closed || cancelled.load(Ordering::Acquire) {
                return Err("SSH inventory is shutting down".into());
            }
            let child = Arc::new(std::sync::Mutex::new(OwnedProcess {
                child: command
                    .spawn()
                    .map_err(|_| "OpenSSH inventory client is unavailable")?,
                reaped: None,
            }));
            let key = self.sequence.fetch_add(1, Ordering::Relaxed);
            state.children.insert(key, child.clone());
            Ok(Registered {
                manager: self.clone(),
                key,
                child,
            })
        }
    }
    pub fn shutdown(&self) {
        let children = {
            let mut state = self.state.lock().unwrap();
            state.closed = true;
            std::mem::take(&mut state.children)
        };
        for child in children.into_values() {
            let _ = child.lock().unwrap().stop();
        }
    }
}
impl Drop for Registered {
    fn drop(&mut self) {
        let _ = self.child.lock().unwrap().stop();
        self.manager
            .state
            .lock()
            .unwrap()
            .children
            .remove(&self.key);
    }
}
pub(crate) fn read(
    spec: &Ssh,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    children: &Arc<Children>,
) -> Result<Inventory, String> {
    read_command(command(spec)?, deadline, cancelled, children)
}
pub(in crate::fleet) fn read_command(
    command: Command,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    children: &Arc<Children>,
) -> Result<Inventory, String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        read_posix(command, deadline, cancelled, children)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (command, deadline, cancelled, children);
        Err("Passive SSH inventory needs owned process-tree support on this platform; Windows Job-object support is not implemented".into())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_posix(
    command: Command,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    children: &Arc<Children>,
) -> Result<Inventory, String> {
    if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("SSH inventory was cancelled or timed out".into());
    }
    let owned = children.spawn(command, &cancelled)?;
    let (mut stdout, mut stderr, stdin) = {
        let mut owned = owned.child.lock().unwrap();
        (
            owned.child.stdout.take().expect("piped inventory stdout"),
            owned.child.stderr.take().expect("piped inventory stderr"),
            owned.child.stdin.take().expect("piped inventory stdin"),
        )
    };
    posix::nonblocking(&stdout)
        .and_then(|_| posix::nonblocking(&stderr))
        .and_then(|_| posix::nonblocking(&stdin))
        .map_err(|_| "SSH inventory pipes could not be made cancellable")?;
    let mut output = PipeOutput::default();
    let mut errors = PipeOutput::default();
    let mut input = PROGRAM.as_bytes();
    let mut stdin = Some(stdin);
    let status = loop {
        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            owned
                .child
                .lock()
                .unwrap()
                .stop()
                .map_err(|_| "SSH inventory process group could not be stopped")?;
            return Err("SSH inventory was cancelled or timed out".into());
        }
        output.drain(&mut stdout, 8)?;
        errors.drain(&mut stderr, 8)?;
        if let Some(pipe) = &mut stdin {
            match pipe.write(input) {
                Ok(0) => return Err("SSH inventory request could not be written".into()),
                Ok(written) => {
                    input = &input[written..];
                    if input.is_empty() {
                        stdin = None;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => return Err("SSH inventory request could not be written".into()),
            }
        }
        let exited = owned.child.lock().unwrap().exited();
        match exited {
            Ok(true) => {
                let status = owned
                    .child
                    .lock()
                    .unwrap()
                    .stop()
                    .map_err(|_| "SSH inventory process group could not be stopped")?;
                // Never wait for EOF: a helper can inherit a pipe, even after
                // the real leader has finished. Its output is already queued.
                output.drain(&mut stdout, 33)?;
                errors.drain(&mut stderr, 33)?;
                break status;
            }
            Ok(false) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err("SSH inventory process status could not be inspected".into()),
        }
        std::thread::sleep(
            Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
        );
    };
    if !status.success() {
        return Err(match status.code() {
            Some(41) => "Read-only SSH inventory is unavailable for this remote platform",
            Some(42) => "The remote Hermes home is not present",
            Some(43) => "The remote Hermes home is not an absolute safe path",
            _ => "SSH inventory failed; check known_hosts, SSH authentication and remote platform",
        }
        .into());
    }
    parse(&output.bytes)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Default)]
struct PipeOutput {
    bytes: Vec<u8>,
    eof: bool,
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl PipeOutput {
    fn drain(&mut self, stream: &mut impl Read, reads: usize) -> Result<(), String> {
        if self.eof {
            return Ok(());
        }
        let mut buffer = [0u8; 8192];
        // Bound each drain too, so a continuously writing helper cannot starve
        // the cancellation/deadline and process-status checks.
        for _ in 0..reads {
            match stream.read(&mut buffer) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(n) => {
                    if self.bytes.len() + n > 256 * 1024 {
                        return Err("SSH inventory output exceeds the size limit".into());
                    }
                    self.bytes.extend_from_slice(&buffer[..n]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("SSH inventory output could not be read".into()),
            }
        }
        Ok(())
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod posix {
    #[cfg(target_os = "macos")]
    use std::os::fd::AsRawFd;
    use std::{io, os::fd::AsFd, process::Child};
    pub fn nonblocking(pipe: &impl AsFd) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            let flags = rustix::fs::fcntl_getfl(pipe)?;
            rustix::fs::fcntl_setfl(pipe, flags | rustix::fs::OFlags::NONBLOCK)?;
            Ok(())
        }
        #[cfg(target_os = "macos")]
        {
            unsafe extern "C" {
                fn fcntl(fd: i32, command: i32, ...) -> i32;
            }
            // Darwin F_GETFL/F_SETFL and O_NONBLOCK. Preserve all existing flags.
            let flags = unsafe { fcntl(pipe.as_fd().as_raw_fd(), 3) };
            if flags < 0 || unsafe { fcntl(pipe.as_fd().as_raw_fd(), 4, flags | 4) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }
    pub fn exited(child: &Child) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        {
            use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
            Ok(waitid(
                WaitId::Pid(Pid::from_child(child)),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            )?
            .is_some())
        }
        #[cfg(target_os = "macos")]
        {
            #[repr(C)]
            struct SigInfo {
                signal: i32,
                errno: i32,
                code: i32,
                pid: i32,
                uid: u32,
                status: i32,
                address: *mut std::ffi::c_void,
                padding: [usize; 9],
            }
            unsafe extern "C" {
                fn waitid(kind: i32, id: u32, info: *mut SigInfo, options: i32) -> i32;
            }
            let mut info = SigInfo {
                signal: 0,
                errno: 0,
                code: 0,
                pid: 0,
                uid: 0,
                status: 0,
                address: std::ptr::null_mut(),
                padding: [0; 9],
            };
            // Darwin siginfo_t, P_PID, WEXITED|WNOHANG|WNOWAIT. Observation
            // intentionally leaves the leader waitable until group cleanup.
            if unsafe { waitid(1, child.id(), &mut info, 4 | 1 | 0x20) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(info.signal != 0)
        }
    }
    pub fn kill_group(child: &Child) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            rustix::process::kill_process_group(
                rustix::process::Pid::from_child(child),
                rustix::process::Signal::KILL,
            )?;
            Ok(())
        }
        #[cfg(target_os = "macos")]
        {
            unsafe extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            let pid = i32::try_from(child.id())
                .map_err(|_| io::Error::other("invalid owned process group"))?;
            // The unreaped leader pins this exact process-group identity.
            if unsafe { kill(-pid, 9) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }
}
pub(crate) fn parse(bytes: &[u8]) -> Result<Inventory, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "SSH inventory was not UTF-8")?;
    let mut lines = text.lines();
    if lines.next() != Some("HERMES_INVENTORY_V1") {
        return Err("SSH inventory did not prove a Hermes home".into());
    }
    let mut profiles = std::collections::BTreeSet::new();
    let mut install_id = None;
    for line in lines {
        if let Some(name) = line.strip_prefix("P\t") {
            if valid_profile(name) && !name.ends_with(".rollback-old") {
                profiles.insert(name.to_owned());
            }
        } else if let Some(id) = line.strip_prefix("I\t") {
            let id = id.trim().to_ascii_lowercase();
            if id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit()) {
                install_id = Some(id);
            }
        }
    }
    if !profiles.remove("default") {
        return Err("SSH inventory omitted the verified root profile".into());
    }
    let mut names = vec!["default".into()];
    names.extend(profiles);
    Ok(Inventory {
        profiles: names,
        install_id,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_requires_proof_and_never_reinterprets_gateway_path_as_home() {
        assert!(parse(b"connection refused").is_err());
        assert!(parse(b"HERMES_INVENTORY_V1\nP\tresearch\n").is_err());
        let result = parse(b"HERMES_INVENTORY_V1\nP\tdefault\nP\twork\nP\t.hidden\nP\tbad.rollback-old\nI\tABCDEF0123456789ABCDEF0123456789\n").unwrap();
        assert_eq!(result.profiles, ["default", "work"]);
        assert_eq!(
            result.install_id.as_deref(),
            Some("abcdef0123456789abcdef0123456789")
        );
        let spec = Ssh {
            host: "fixture".into(),
            user: String::new(),
            port: 22,
            key_file: None,
            remote_port: 9119,
            local_port: 49151,
            path: "/gateway-prefix-is-not-home".into(),
        };
        let command = command(&spec).unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|v| v.to_string_lossy().to_string())
            .collect();
        assert!(
            !args
                .iter()
                .any(|v| v == "-L" || v == "-N" || v.contains("gateway-prefix"))
        );
        assert_eq!(&args[args.len() - 2..], ["fixture", "sh -s"]);
    }
    #[cfg(unix)]
    #[test]
    fn actual_read_program_only_inspects_a_private_home_and_existing_identity() {
        let root = std::env::temp_dir().join(format!(
            "hermes-ssh-inventory-program-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(root.join("profiles/research")).unwrap();
        std::fs::create_dir_all(root.join("profiles/.hidden")).unwrap();
        std::fs::create_dir_all(root.join("profiles/bad\nP\tintruder")).unwrap();
        std::fs::create_dir_all(root.join("profiles/junk.rollback-old")).unwrap();
        std::fs::write(
            root.join("install_id"),
            "0123456789abcdef0123456789abcdef\n",
        )
        .unwrap();
        let original = std::fs::read(root.join("install_id")).unwrap();
        let mut command = Command::new("sh");
        command
            .arg("-s")
            .env("HOME", &root)
            .env("HERMES_HOME", &root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let children = Arc::new(Children::default());
        let inventory = read_command(
            command,
            Instant::now() + Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
            &children,
        )
        .unwrap();
        assert_eq!(inventory.profiles, ["default", "research"]);
        assert_eq!(
            inventory.install_id.as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(std::fs::read(root.join("install_id")).unwrap(), original);
        assert!(children.is_empty());
        let missing = root.join("missing");
        let mut command = Command::new("sh");
        command
            .arg("-s")
            .env("HOME", &root)
            .env("HERMES_HOME", &missing)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        assert!(
            read_command(
                command,
                Instant::now() + Duration::from_secs(2),
                Arc::new(AtomicBool::new(false)),
                &children
            )
            .is_err()
        );
        assert!(
            !missing.exists(),
            "inventory must never provision an absent home"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "ssh/process_tests.rs"]
mod process_tests;
