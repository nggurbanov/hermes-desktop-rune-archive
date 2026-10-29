//! OpenSSH forwards only. No remote command, installation or agent lifecycle.
use super::{
    Ssh,
    core::{Tunnel, Tunnels},
};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};
pub(super) struct OpenSsh;
pub(super) fn validate(spec: &Ssh) -> Result<(), String> {
    if spec.host.is_empty()
        || spec.host.len() > 1024
        || spec.host.starts_with('-')
        || spec.host.contains(['@', '/', '\\'])
        || spec.host.chars().any(char::is_whitespace)
        || spec.host.chars().any(char::is_control)
    {
        return Err(
            "Enter an SSH hostname, IP address or SSH config alias without user@ or a command"
                .into(),
        );
    }
    if spec.user.len() > 256
        || spec.user.chars().any(char::is_whitespace)
        || spec.user.chars().any(char::is_control)
    {
        return Err("The SSH username is invalid".into());
    }
    if spec.port == 0 || spec.remote_port == 0 {
        return Err("SSH and gateway ports must be between 1 and 65535".into());
    }
    Ok(())
}
fn command(spec: &Ssh) -> Command {
    let mut command = Command::new("ssh");
    command.args([
        "-N",
        "-T",
        "-v",
        "-o",
        "BatchMode=yes",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "StrictHostKeyChecking=yes",
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
        .arg("-L")
        .arg(format!(
            "127.0.0.1:{}:127.0.0.1:{}",
            spec.local_port, spec.remote_port
        ))
        .arg("--")
        .arg(&spec.host)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}
struct Process {
    child: Child,
    closed: bool,
}
impl Tunnel for Process {
    fn exited(&mut self) -> Option<String> {
        if self.closed {
            return None;
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.closed = true;
                Some(format!(
                    "The SSH tunnel ended ({status}). Reconnect to retry the same saved port"
                ))
            }
            Ok(None) => None,
            Err(_) => Some("The SSH tunnel status could not be read".into()),
        }
    }
    fn close(&mut self) {
        if !self.closed {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.closed = true;
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.close();
    }
}
impl Tunnels for OpenSsh {
    fn open(&mut self, spec: &Ssh) -> Result<Box<dyn Tunnel>, String> {
        validate(spec)?;
        if spec.local_port == 0 {
            return Err("Save this SSH connection before opening its fixed local port".into());
        }
        let reservation=std::net::TcpListener::bind(("127.0.0.1",spec.local_port)).map_err(|_|format!("Saved SSH port {} is already in use. Hermes will not connect through an unowned listener or choose a different port",spec.local_port))?;
        drop(reservation);
        let mut child=command(spec).spawn().map_err(|_|"OpenSSH could not be started. Install an SSH client and configure passwordless access first")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("OpenSSH did not provide its connection status")?;
        let (send, receive) = mpsc::channel();
        let port = spec.local_port;
        std::thread::spawn(move || {
            let mut lines = BufReader::new(stderr);
            let mut count = 0;
            loop {
                let mut line = String::new();
                match lines.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        count += line.len();
                        if count > 256 * 1024 {
                            let _ = std::io::copy(&mut lines, &mut std::io::sink());
                            break;
                        }
                        if line.contains("Local forwarding listening on 127.0.0.1 port")
                            && line.contains(&format!("port {port}."))
                        {
                            let _ = send.send(Ok(()));
                        } else if line.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
                            || line.contains("Host key verification failed")
                        {
                            let _=send.send(Err("The SSH host key is not trusted. Verify it using your OpenSSH known_hosts setup before reconnecting".to_owned()));
                        } else if line.contains("Permission denied") {
                            let _=send.send(Err("SSH authentication was refused. Check your SSH agent, username and key".to_owned()));
                        } else if line.contains("Address already in use") {
                            let _=send.send(Err(format!("Saved SSH port {port} was taken while connecting; no alternate listener was used")));
                        }
                    }
                }
            }
        });
        let mut process = Process {
            child,
            closed: false,
        };
        match receive.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => {
                if let Some(error) = process.exited() {
                    return Err(error);
                }
                Ok(Box::new(process))
            }
            Ok(Err(error)) => {
                process.close();
                Err(error)
            }
            Err(_) => {
                process.close();
                Err("The SSH tunnel did not become ready. Check the host, known_hosts and passwordless SSH access".into())
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssh_forward_never_constructs_a_remote_command_or_changes_its_owned_port() {
        let spec = Ssh {
            host: "agent-alias".into(),
            user: "ubuntu".into(),
            port: 2222,
            key_file: None,
            remote_port: 9119,
            local_port: 49151,
            path: String::new(),
        };
        let command = command(&spec);
        let args: Vec<_> = command
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"-N".to_owned()));
        assert!(args.contains(&"BatchMode=yes".to_owned()));
        assert!(args.contains(&"StrictHostKeyChecking=yes".to_owned()));
        let marker = args.iter().position(|s| s == "-L").unwrap();
        assert_eq!(args[marker + 1], "127.0.0.1:49151:127.0.0.1:9119");
        assert_eq!(&args[args.len() - 2..], &["--", "agent-alias"]);
        let mut bad = spec.clone();
        bad.host = "-oProxyCommand=anything".into();
        assert!(validate(&bad).is_err());
        bad.host = "user@host".into();
        assert!(validate(&bad).is_err());
    }
}
