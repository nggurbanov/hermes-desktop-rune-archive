//! Remote update gate. The Python program is the pinned Electron baseline's
//! marker/checkout-lock judge, executed by the remote system Python.
use super::{Cancellation, Ssh, quote, remote_path};
use std::time::Duration;

const UPDATE_GATE: &str = include_str!("update_gate.py");

pub(super) fn probe_update_gate(
    ssh: &Ssh,
    marker: &str,
    launcher: &str,
    cancel: &Cancellation,
) -> Result<(), String> {
    let cmd = format!(
        "python3 - {} '' {}",
        remote_path(marker)?,
        remote_path(launcher)?
    );
    let output = ssh.exec(
        &cmd,
        Some(UPDATE_GATE.as_bytes()),
        Duration::from_secs(15),
        cancel,
    )?;
    if output.lines().last().map(str::trim) == Some("CLEAR") {
        Ok(())
    } else {
        Err("Remote Hermes update state could not be proved clear".into())
    }
}

/// Run creation while holding the same kernel lock as remote updates. Neither
/// `payload` nor the Python program contains a session token.
pub(super) fn run_under_update_gate(
    ssh: &Ssh,
    marker: &str,
    launcher: &str,
    payload: &str,
    cancel: &Cancellation,
) -> Result<String, String> {
    let cmd = format!(
        "python3 -c {} {} {} {}",
        quote(UPDATE_GATE),
        remote_path(marker)?,
        quote(payload),
        remote_path(launcher)?
    );
    ssh.exec(&cmd, None, Duration::from_secs(35), cancel)
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::Spec;
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn canonical_discovery_filters_ledger_without_mutating_it() {
        let root = std::env::temp_dir().join(format!("rune-ledger-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".hermes")).unwrap();
        let ssh_path = root.join("ssh");
        let script = format!(
            "#!/bin/sh\nHOME={} ; export HOME\nfor arg do last=$arg; done\nexec sh -c \"$last\"\n",
            quote(root.to_str().unwrap())
        );
        std::fs::write(&ssh_path, script).unwrap();
        std::fs::set_permissions(&ssh_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            ssh_path,
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
        let ledger = root.join(".hermes/spawn-ledger.json");
        let data = format!(
            r#"[{{"pid":{},"port":1234,"purpose":"serve","host":"127.0.0.1"}},{{"pid":{},"port":1235,"purpose":"serve","host":"127.0.0.1","isolated":true}},{{"pid":{},"port":1236,"purpose":"serve","host":"example.com"}}]"#,
            std::process::id(),
            std::process::id(),
            std::process::id()
        );
        std::fs::write(&ledger, &data).unwrap();
        assert_eq!(
            canonical_candidates(
                &ssh,
                root.join(".hermes").to_str().unwrap(),
                &Cancellation::default()
            )
            .unwrap(),
            vec![1234]
        );
        assert_eq!(std::fs::read_to_string(&ledger).unwrap(), data);
        std::fs::write(&ledger, "malformed").unwrap();
        assert!(
            canonical_candidates(
                &ssh,
                root.join(".hermes").to_str().unwrap(),
                &Cancellation::default()
            )
            .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn reservation_preserves_foreign_nonce_and_cleans_exact_spawn() {
        let root = std::env::temp_dir().join(format!("rune-spawn-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ssh_path = root.join("ssh");
        std::fs::write(
            &ssh_path,
            "#!/bin/sh\nfor arg do last=$arg; done\nexec sh -c \"$last\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&ssh_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let launcher = root.join("hermes");
        std::fs::write(
            &launcher,
            "#!/usr/bin/env python3\nimport time\ntime.sleep(60)\n",
        )
        .unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            ssh_path,
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
        let owner = root.join("owner");
        let nonce = "0123456789abcdef";
        let token = owner.join(format!("{nonce}.token"));
        let marker = root.join("marker");
        let cancel = Cancellation::default();
        upload_token(&ssh, token.to_str().unwrap(), b"private", &cancel).unwrap();
        let stranded = owner.join(".connect.lock");
        std::fs::create_dir(&stranded).unwrap();
        std::fs::write(stranded.join("owner"), "2000000000:fedcba9876543210").unwrap();
        let pid = spawn_dashboard(
            &ssh,
            marker.to_str().unwrap(),
            launcher.to_str().unwrap(),
            owner.to_str().unwrap(),
            nonce,
            Some("default"),
            root.to_str().unwrap(),
            &cancel,
        )
        .unwrap();
        struct Cleanup<'a> {
            ssh: &'a Ssh,
            owner: &'a str,
            nonce: &'a str,
        }
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let _ = cleanup_dashboard(self.ssh, self.owner, self.nonce);
            }
        }
        let _guard = Cleanup {
            ssh: &ssh,
            owner: owner.to_str().unwrap(),
            nonce,
        };
        assert!(
            !stranded.exists(),
            "dead reservation should be reclaimed under the update mutex"
        );
        let lock = owner.join("backend.lock.json");
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&lock).unwrap()).unwrap();
        assert_eq!(record["pid"], pid);
        assert_eq!(record["profile"], "");
        assert!(cleanup_dashboard(&ssh, owner.to_str().unwrap(), "fedcba9876543210").is_err());
        assert!(lock.exists());
        assert!(
            spawn_dashboard(
                &ssh,
                marker.to_str().unwrap(),
                launcher.to_str().unwrap(),
                owner.to_str().unwrap(),
                nonce,
                None,
                root.to_str().unwrap(),
                &cancel
            )
            .is_err()
        );
        cleanup_dashboard(&ssh, owner.to_str().unwrap(), nonce).unwrap();
        assert!(!lock.exists());
        assert!(!token.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn nonce_token_upload_is_exclusive_private_and_stdin_only() {
        let root = std::env::temp_dir().join(format!("rune-token-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ssh_path = root.join("ssh");
        std::fs::write(&ssh_path, "#!/bin/sh\nfor arg do last=$arg; done\nprintf '%s' \"$last\" > \"$0.args\"\nexec sh -c \"$last\"\n").unwrap();
        std::fs::set_permissions(&ssh_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            ssh_path.clone(),
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
        let path = root.join("owner/0123456789abcdef.token");
        let cancel = Cancellation::default();
        upload_token(
            &ssh,
            path.to_str().unwrap(),
            b"private-fixture-token",
            &cancel,
        )
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"private-fixture-token");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(
            !std::fs::read_to_string(root.join("ssh.args"))
                .unwrap()
                .contains("private-fixture-token")
        );
        assert!(upload_token(&ssh, path.to_str().unwrap(), b"other", &cancel).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"private-fixture-token");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn remote_update_gate_fails_closed_and_executes_clear_payload() {
        let root = std::env::temp_dir().join(format!("rune-gate-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ssh_path = root.join("ssh");
        std::fs::write(
            &ssh_path,
            "#!/bin/sh\nfor arg do last=$arg; done\nexec sh -c \"$last\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&ssh_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            ssh_path,
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
        let marker = root.join(".hermes-update-in-progress");
        let marker = marker.to_str().unwrap();
        let cancel = Cancellation::default();
        probe_update_gate(&ssh, marker, "/bin/true", &cancel).unwrap();
        assert_eq!(
            run_under_update_gate(&ssh, marker, "/bin/true", "printf SAFE", &cancel).unwrap(),
            "SAFE"
        );
        std::fs::write(marker, "malformed").unwrap();
        assert!(probe_update_gate(&ssh, marker, "/bin/true", &cancel).is_err());
        assert!(
            run_under_update_gate(&ssh, marker, "/bin/true", "printf UNSAFE", &cancel).is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn dead_generation_recovery_is_serialized_and_never_signals_live_processes() {
        let root = std::env::temp_dir().join(format!("rune-recovery-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ssh_path = root.join("ssh");
        std::fs::write(
            &ssh_path,
            "#!/bin/sh\nfor arg do last=$arg; done\nexec sh -c \"$last\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&ssh_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let launcher = root.join("hermes");
        let child_file = root.join("fixture-child.pid");
        std::fs::write(&launcher, format!("#!/usr/bin/env python3\nimport os,time\nopen({:?},'w').write(str(os.getpid()))\ntime.sleep(60)\n", child_file.to_str().unwrap())).unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
        let ssh = Ssh::new(
            ssh_path,
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
        let owner = root.join("owner");
        let marker = root.join("marker");
        let nonce = "0123456789abcdef";
        let token = owner.join(format!("{nonce}.token"));
        let cancel = Cancellation::default();
        upload_token(&ssh, token.to_str().unwrap(), b"private", &cancel).unwrap();
        let pid = spawn_dashboard(
            &ssh,
            marker.to_str().unwrap(),
            launcher.to_str().unwrap(),
            owner.to_str().unwrap(),
            nonce,
            None,
            root.to_str().unwrap(),
            &cancel,
        )
        .unwrap();
        struct Guard<'a> {
            ssh: &'a Ssh,
            owner: &'a str,
            nonce: &'a str,
        }
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                let _ = cleanup_dashboard(self.ssh, self.owner, self.nonce);
            }
        }
        let _guard = Guard {
            ssh: &ssh,
            owner: owner.to_str().unwrap(),
            nonce,
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !child_file.exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let child_pid: u32 = std::fs::read_to_string(&child_file)
            .unwrap()
            .parse()
            .unwrap();
        struct ChildGuard(u32);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _=std::process::Command::new("python3").args(["-c","import os,signal,sys;fd=os.pidfd_open(int(sys.argv[1]));signal.pidfd_send_signal(fd,signal.SIGKILL);os.close(fd)",&self.0.to_string()]).stderr(std::process::Stdio::null()).status();
            }
        }
        let _child_guard = ChildGuard(child_pid);
        let lock = owner.join("backend.lock.json");
        // The exact live incarnation remains untouched, even when another
        // route in the same home asks to reclaim it.
        reclaim_dead_generation(
            &ssh,
            marker.to_str().unwrap(),
            launcher.to_str().unwrap(),
            owner.to_str().unwrap(),
            root.to_str().unwrap(),
            None,
            Some(nonce),
            &cancel,
        )
        .unwrap();
        assert!(lock.exists());
        assert!(
            reclaim_dead_generation(
                &ssh,
                marker.to_str().unwrap(),
                launcher.to_str().unwrap(),
                owner.to_str().unwrap(),
                root.to_str().unwrap(),
                Some("other"),
                Some(nonce),
                &cancel
            )
            .is_err()
        );
        assert!(lock.exists());
        let original = std::fs::read(&lock).unwrap();
        let mut foreign: serde_json::Value = serde_json::from_slice(&original).unwrap();
        foreign["spawnNonce"] = "fedcba9876543210".into();
        foreign["logPath"] = format!("{}/fedcba9876543210.log", owner.display()).into();
        std::fs::write(&lock, serde_json::to_vec(&foreign).unwrap()).unwrap();
        assert!(
            reclaim_dead_generation(
                &ssh,
                marker.to_str().unwrap(),
                launcher.to_str().unwrap(),
                owner.to_str().unwrap(),
                root.to_str().unwrap(),
                None,
                Some(nonce),
                &cancel,
            )
            .is_err()
        );
        assert!(lock.exists());
        std::fs::write(&lock, original).unwrap();
        // Simulate abrupt loss of this controlled fixture process via pidfd.
        let killed = std::process::Command::new("python3").args(["-c", "import os,signal,sys;fd=os.pidfd_open(int(sys.argv[1]));signal.pidfd_send_signal(fd,signal.SIGKILL);os.close(fd)", &pid.to_string()]).status().unwrap();
        assert!(killed.success());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let status = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            if status.is_empty()
                || status[status.rfind(')').unwrap() + 2..]
                    .split_whitespace()
                    .next()
                    == Some("Z")
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        // Wrong-nonce recovery cannot signal the live session anchor.
        assert!(
            reclaim_dead_generation(
                &ssh,
                marker.to_str().unwrap(),
                launcher.to_str().unwrap(),
                owner.to_str().unwrap(),
                root.to_str().unwrap(),
                None,
                Some("fedcba9876543210"),
                &cancel
            )
            .is_err()
        );
        assert!(lock.exists());
        let child_state = std::fs::read_to_string(format!("/proc/{child_pid}/stat")).unwrap();
        assert_ne!(
            child_state[child_state.rfind(')').unwrap() + 2..]
                .split_whitespace()
                .next(),
            Some("Z")
        );
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let barrier = barrier.clone();
            let ssh = ssh.clone();
            let marker = marker.to_str().unwrap().to_string();
            let launcher = launcher.to_str().unwrap().to_string();
            let owner = owner.to_str().unwrap().to_string();
            let home = root.to_str().unwrap().to_string();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                reclaim_dead_generation(
                    &ssh,
                    &marker,
                    &launcher,
                    &owner,
                    &home,
                    None,
                    Some("0123456789abcdef"),
                    &Cancellation::default(),
                )
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        assert!(!lock.exists());
        assert!(!token.exists());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let stat =
                std::fs::read_to_string(format!("/proc/{child_pid}/stat")).unwrap_or_default();
            if stat.is_empty()
                || stat[stat.rfind(')').unwrap() + 2..]
                    .split_whitespace()
                    .next()
                    == Some("Z")
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

const TOKEN_UPLOAD: &str = r#"import os,stat,sys
p=os.path.expanduser(sys.argv[1]);d=os.path.dirname(p);n=os.path.basename(p)
os.makedirs(d,mode=0o700,exist_ok=True)
dd=os.open(d,os.O_RDONLY|getattr(os,'O_DIRECTORY',0)|getattr(os,'O_NOFOLLOW',0))
try:
 s=os.fstat(dd)
 if not stat.S_ISDIR(s.st_mode) or (hasattr(os,'getuid') and s.st_uid!=os.getuid()):raise SystemExit(1)
 if (s.st_mode&0o777)!=0o700:os.fchmod(dd,0o700)
 fd=os.open(n,os.O_WRONLY|os.O_CREAT|os.O_EXCL|getattr(os,'O_NOFOLLOW',0),0o600,dir_fd=dd)
 try:
  data=sys.stdin.buffer.read(4097)
  if not data or len(data)>4096:raise ValueError('invalid credential')
  while data:
   count=os.write(fd,data)
   if count<=0:raise OSError('short write')
   data=data[count:]
 except BaseException:
  os.unlink(n,dir_fd=dd)
  raise
 finally:os.close(fd)
finally:os.close(dd)
"#;

/// Writes a one-generation credential through stdin into an exclusive 0600
/// file, never into the SSH argument vector or ownership metadata.
pub(super) fn upload_token(
    ssh: &Ssh,
    token_path: &str,
    token: &[u8],
    cancel: &Cancellation,
) -> Result<(), String> {
    let command = format!(
        "python3 -c {} {}",
        quote(TOKEN_UPLOAD),
        remote_path(token_path)?
    );
    ssh.exec(&command, Some(token), Duration::from_secs(15), cancel)?;
    Ok(())
}

const SPAWN: &str = include_str!("spawn.py");
const CLEANUP: &str = include_str!("cleanup.py");

fn valid_nonce(nonce: &str) -> bool {
    nonce.len() == 16
        && nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The reservation, process creation and generation record are serialized
/// inside the update gate. A pre-existing record is never overwritten.
pub(super) fn spawn_dashboard(
    ssh: &Ssh,
    marker: &str,
    launcher: &str,
    owner: &str,
    nonce: &str,
    profile: Option<&str>,
    home: &str,
    cancel: &Cancellation,
) -> Result<u32, String> {
    if !valid_nonce(nonce) {
        return Err("Invalid SSH generation".into());
    }
    let profile = super::profile_arg(profile)?.unwrap_or_default();
    let payload = format!(
        "python3 -c {} {} {} {} {} {} {}",
        quote(SPAWN),
        quote(launcher),
        quote(owner),
        quote(nonce),
        quote(&profile),
        quote(home),
        quote(include_str!("supervisor.py"))
    );
    let output = run_under_update_gate(ssh, marker, launcher, &payload, cancel)?;
    if output.trim() == "EXISTING" {
        return Err("A remote SSH generation already owns this route".into());
    }
    output
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| "Remote SSH generation did not publish its process".into())
}

/// Cleanup uses Linux pidfds plus a live exact-argv/nonce/start-time check.
/// Unsupported or unprovable process identities fail closed.
pub(super) fn cleanup_dashboard(ssh: &Ssh, owner: &str, nonce: &str) -> Result<(), String> {
    if !valid_nonce(nonce) {
        return Err("Invalid SSH generation".into());
    }
    let command = format!(
        "python3 -c {} {} {}",
        quote(CLEANUP),
        remote_path(owner)?,
        quote(nonce)
    );
    let output = ssh.exec(
        &command,
        None,
        Duration::from_secs(20),
        &Cancellation::default(),
    )?;
    if output.trim() == "CLEAN" {
        Ok(())
    } else {
        Err("Could not prove SSH generation cleanup".into())
    }
}

const DISCOVER: &str = r#"import json,os,sys
p=os.path.join(sys.argv[1],'spawn-ledger.json')
try:
 with open(p,'rb') as f: raw=f.read(1048577)
except FileNotFoundError:print('[]');sys.exit(0)
if len(raw)>1048576:raise SystemExit(2)
rows=json.loads(raw)
if not isinstance(rows,list):raise SystemExit(3)
out=[]
for r in rows:
 if not isinstance(r,dict):continue
 pid=r.get('pid');port=r.get('port');host=r.get('host','')
 if not isinstance(pid,int) or isinstance(pid,bool) or pid<=0 or not isinstance(port,int) or isinstance(port,bool) or not 0<port<65536:continue
 if r.get('purpose') not in ('serve','dashboard') or r.get('isolated') is True or not isinstance(host,str) or host.lower() not in ('','0.0.0.0','127.0.0.1','::','::1','localhost'):continue
 try:os.kill(pid,0)
 except ProcessLookupError:continue
 except PermissionError:pass
 try:
  with open('/proc/%d/stat'%pid) as f:proc=f.read()
  fields=proc[proc.rfind(')')+2:].split()
  if fields[0]=='Z':continue
  claimed=r.get('create_time')
  if isinstance(claimed,(int,float)) and not isinstance(claimed,bool):
   with open('/proc/stat') as f:btime=next(int(line.split()[1]) for line in f if line.startswith('btime '))
   actual=btime+int(fields[19])/os.sysconf('SC_CLK_TCK')
   if abs(actual-claimed)>2.0:continue
 except (OSError,ValueError,IndexError,StopIteration):pass
 out.append(port)
print(json.dumps(out))
"#;

/// Ledger entries are candidates only. Callers must verify the tunneled
/// service and websocket before considering any candidate attachable.
pub(super) fn canonical_candidates(
    ssh: &Ssh,
    machine_home: &str,
    cancel: &Cancellation,
) -> Result<Vec<u16>, String> {
    let cmd = format!(
        "python3 -c {} {}",
        quote(DISCOVER),
        remote_path(machine_home)?
    );
    let output = ssh.exec(&cmd, None, Duration::from_secs(15), cancel)?;
    serde_json::from_str(output.trim())
        .map_err(|_| "Could not parse remote dashboard discovery".into())
}

pub(super) fn publish_dashboard(
    ssh: &Ssh,
    owner: &str,
    nonce: &str,
    port: u16,
    token: &str,
    cancel: &Cancellation,
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    if !valid_nonce(nonce) || port == 0 {
        return Err("Invalid SSH generation".into());
    }
    let fingerprint = format!("{:x}", Sha256::digest(token.as_bytes()))[..32].to_string();
    let command = format!(
        "python3 -c {} {} {} {} {}",
        quote(include_str!("publish.py")),
        remote_path(owner)?,
        quote(nonce),
        port,
        quote(&fingerprint)
    );
    let output = ssh.exec(&command, None, Duration::from_secs(15), cancel)?;
    if output.trim() == "PUBLISHED" {
        Ok(())
    } else {
        Err("Could not publish verified SSH generation".into())
    }
}

pub(super) fn inspect_generation(
    ssh: &Ssh,
    owner: &str,
    cancel: &Cancellation,
) -> Result<Option<serde_json::Value>, String> {
    let cmd = format!(
        "python3 -c {} {}",
        quote(include_str!("inspect.py")),
        remote_path(owner)?
    );
    // Missing ownership directory is equivalent to no record; malformed or
    // unprovable records remain errors and must never be overwritten.
    let script = format!(
        "if [ ! -e {} ]; then printf NONE; else {}; fi",
        remote_path(owner)?,
        cmd
    );
    let out = ssh.exec(&script, None, Duration::from_secs(15), cancel)?;
    if out.trim() == "NONE" {
        return Ok(None);
    }
    serde_json::from_str(out.trim())
        .map(Some)
        .map_err(|_| "Could not inspect remote SSH ownership".into())
}

/// Fences an uncertain spawn under the updater mutex, then cleans only this nonce.
pub(super) fn fence_cleanup(
    ssh: &Ssh,
    marker: &str,
    launcher: &str,
    owner: &str,
    nonce: &str,
) -> Result<(), String> {
    if !valid_nonce(nonce) {
        return Err("Invalid SSH generation".into());
    }
    let payload = format!(
        "if [ ! -d {} ]; then printf CLEAN; else python3 -c {} {} {}; fi",
        remote_path(owner)?,
        quote(CLEANUP),
        remote_path(owner)?,
        quote(nonce)
    );
    let output = run_under_update_gate(ssh, marker, launcher, &payload, &Cancellation::default())?;
    match output.trim() {
        "CLEAN" | "FOREIGN" => Ok(()),
        _ => Err("Could not fence SSH generation cleanup".into()),
    }
}

pub(super) fn require_ownership_capability(
    ssh: &Ssh,
    launcher: &str,
    cancel: &Cancellation,
) -> Result<(), String> {
    let executable = remote_path(launcher)?;
    let command = watchdog(&format!("{executable} serve --help 2>&1"));
    let output = ssh.exec(&command, None, Duration::from_secs(18), cancel)?;
    if output.contains("ssh-session-token-file") && output.contains("ssh-owner-nonce") {
        Ok(())
    } else {
        Err("The remote Hermes installation does not support SSH ownership; update it before connecting".into())
    }
}

/// Reclaim only a record whose original kernel process incarnation is gone.
/// This runs under the same updater mutex as both Desktop implementations.
pub(super) fn reclaim_dead_generation(
    ssh: &Ssh,
    marker: &str,
    launcher: &str,
    owner: &str,
    home: &str,
    profile: Option<&str>,
    expected_nonce: Option<&str>,
    cancel: &Cancellation,
) -> Result<Option<String>, String> {
    let profile = super::profile_arg(profile)?.unwrap_or_default();
    let payload = format!(
        "python3 -c {} {} {} {} {} {}",
        quote(include_str!("reclaim.py")),
        quote(owner),
        quote(launcher),
        quote(home),
        quote(&profile),
        quote(expected_nonce.unwrap_or(""))
    );
    let output = run_under_update_gate(ssh, marker, launcher, &payload, cancel)?;
    match output.trim() {
        "NONE" | "LIVE" | "FOREIGN" => Ok(None),
        value if value.strip_prefix("RECLAIMED:").is_some_and(valid_nonce) => {
            Ok(Some(value[10..].to_string()))
        }
        _ => Err("Could not prove remote SSH recovery".into()),
    }
}

pub(super) fn watchdog(command: &str) -> String {
    format!(
        "[ -n \"${{ZSH_VERSION-}}\" ] || set -m 2>/dev/null; ({command}) </dev/null & probe=$!; set +m 2>/dev/null; (sleep 15 </dev/null >/dev/null 2>&1 & sleeper=$!; trap 'kill $sleeper 2>/dev/null; wait $sleeper 2>/dev/null; exit' TERM; wait $sleeper; kill -9 -- -$probe 2>/dev/null; kill -9 $probe 2>/dev/null) & watchdog=$!; wait $probe; result=$?; kill $watchdog 2>/dev/null; wait $watchdog 2>/dev/null; exit $result"
    )
}
