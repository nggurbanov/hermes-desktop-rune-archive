use super::*;
mod dropfiles;
use crate::gateway::terminal::{Frame, HostTerminal};
use std::{
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Clone)]
pub(crate) enum Target {
    Local,
    #[cfg(test)]
    Test(String),
    Ssh {
        config: crate::connections::Ssh,
        windows: bool,
        connection: Connection,
    },
    Remote(Connection),
}
pub(crate) enum Command {
    Input(Vec<u8>),
    Resize(u16, u16),
    Detach,
    Close,
    Drop(Vec<PathBuf>),
}
pub(crate) enum Event {
    Started {
        shell: String,
        cwd: String,
        durable: Option<String>,
    },
    Data(Vec<u8>, bool),
    Reset(bool),
    State(Status),
    Exit(Option<u32>),
    Closed,
    Error(String),
    Warning(String),
    Cwd(String),
}
pub(crate) struct Handle {
    pub send: Sender<Command>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        let _ = self.send.send(Command::Detach);
    }
}
#[derive(Clone)]
pub(crate) struct Start {
    pub target: Target,
    pub profile: String,
    pub profile_intent: crate::source::ProfileIntent,
    pub cwd: String,
    pub rows: u16,
    pub cols: u16,
    pub durable: Option<String>,
}
pub(crate) fn spawn(
    owner: String,
    id: String,
    generation: u64,
    start: Start,
    replies: Sender<Reply>,
    waker: Waker,
) -> Handle {
    let (send, receive) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let stopping = cancel.clone();
    std::thread::spawn(move || {
        let notify = |event| {
            if replies
                .send(Reply::Event {
                    owner: owner.clone(),
                    id: id.clone(),
                    generation,
                    event,
                })
                .is_ok()
            {
                waker();
            }
        };
        let result = match &start.target {
            Target::Remote(connection) => remote(&start, connection, &receive, &stopping, &notify),
            _ => native(&start, &receive, &stopping, &notify),
        };
        if let Err(error) = result {
            notify(Event::Error(error));
        }
    });
    Handle { send, cancel }
}
fn remote(
    start: &Start,
    connection: &Connection,
    commands: &Receiver<Command>,
    cancel: &Arc<AtomicBool>,
    notify: &impl Fn(Event),
) -> Result<(), String> {
    let staged = dropfiles::Stager::new();
    let mut durable = start.durable.clone();
    let mut attempts = 0;
    let mut size = (start.cols, start.rows);
    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        notify(Event::State(if durable.is_some() {
            Status::Reconnecting
        } else {
            Status::Starting
        }));
        let dial = HostTerminal::connect_scoped(
            connection,
            &start.profile,
            start.profile_intent,
            &start.cwd,
            size.0,
            size.1,
            durable.as_deref(),
            false,
        );
        let mut socket = match dial {
            Ok(s) => s,
            Err(error) => {
                if durable.is_none() || attempts >= 5 {
                    return Err(error);
                }
                attempts += 1;
                if pause(commands, cancel, attempts)? {
                    return Ok(());
                }
                continue;
            }
        };
        let attached = durable.clone();
        let mut ready = false;
        let mut replay = false;
        let began = Instant::now();
        let mut close_requested = false;
        let lost = loop {
            if cancel.load(Ordering::Acquire) {
                socket.detach();
                return Ok(());
            }
            for command in commands.try_iter() {
                match command {
                    Command::Input(bytes) => {
                        if ready {
                            socket.write(&bytes)?;
                        } else {
                            return Err("The remote shell is not ready for input".into());
                        }
                    }
                    Command::Resize(cols, rows) => {
                        size = (cols, rows);
                        if ready {
                            socket.resize(cols, rows)?;
                        }
                    }
                    Command::Detach => {
                        socket.detach();
                        return Ok(());
                    }
                    Command::Close => {
                        close_requested = true;
                        break;
                    }
                    Command::Drop(paths) => staged.start(
                        connection.clone(),
                        start.profile.clone(),
                        start.profile_intent,
                        paths,
                        cancel.clone(),
                    ),
                }
            }
            if ready {
                for result in staged.poll() {
                    match result {
                        Ok(paths) => {
                            for path in paths {
                                socket
                                    .write(format!("{} ", input::quote(&path, false)).as_bytes())?;
                            }
                        }
                        Err(error) => notify(Event::Warning(error)),
                    }
                }
            }
            if close_requested {
                socket.detach();
                return close_remote(
                    start,
                    connection,
                    durable
                        .as_deref()
                        .ok_or("The remote shell has no durable id; close cannot be confirmed")?,
                    notify,
                );
            }
            match socket.poll() {
                Ok(Some(Frame::Meta {
                    terminal_id,
                    shell,
                    cwd,
                    truncated,
                    closed,
                })) => {
                    if closed {
                        return Err("The remote shell unexpectedly answered as closed".into());
                    }
                    if shell.trim().is_empty() {
                        return Err("The remote terminal returned no shell identity".into());
                    }
                    if let Some(expected) = &attached {
                        if terminal_id.as_ref() != Some(expected) {
                            return Err(
                                "The remote terminal did not resume the requested shell".into()
                            );
                        }
                    }
                    if let Some(id) = terminal_id {
                        if id.is_empty() || id.len() > 512 {
                            return Err(
                                "The remote terminal returned an invalid durable identity".into()
                            );
                        }
                        durable = Some(id);
                        replay = true;
                    }
                    if attached.is_some() {
                        notify(Event::Reset(truncated));
                        if truncated {
                            socket
                                .resize(size.0, if size.1 > 2 { size.1 - 1 } else { size.1 + 1 })?;
                        }
                        socket.resize(size.0, size.1)?;
                    }
                    notify(Event::Started {
                        shell,
                        cwd: cwd.unwrap_or_else(|| start.cwd.clone()),
                        durable: durable.clone(),
                    });
                    ready = true;
                }
                Ok(Some(Frame::Data(bytes))) => {
                    if !ready {
                        return Err(
                            "The remote terminal sent output before identifying its shell".into(),
                        );
                    }
                    notify(Event::Data(bytes, replay));
                    replay = false;
                }
                Ok(Some(Frame::Closed { code, reason })) => {
                    if code == 4410 && reason == "Process exited" {
                        notify(Event::Exit(None));
                        return Ok(());
                    }
                    if matches!(code, 4401 | 4403 | 4409 | 4410 | 1013) {
                        return Err(format!(
                            "Remote terminal closed ({code}): {}",
                            reason.chars().take(240).collect::<String>()
                        ));
                    }
                    break true;
                }
                Ok(None) => {}
                Err(_) => break true,
            }
            if !ready && began.elapsed() > Duration::from_secs(10) {
                break true;
            }
        };
        socket.detach();
        if !lost {
            return Ok(());
        }
        if durable.is_none() {
            return Err("The remote shell's start outcome is unknown. It will not be restarted automatically".into());
        }
        if began.elapsed() > Duration::from_secs(10) {
            attempts = 0;
        }
        attempts += 1;
        if attempts > 5 {
            return Err(
                "Remote terminal reconnect exhausted; retry attaching this same shell".into(),
            );
        }
        if pause(commands, cancel, attempts)? {
            return Ok(());
        }
    }
}
fn pause(commands: &Receiver<Command>, cancel: &AtomicBool, attempt: u32) -> Result<bool, String> {
    let began = Instant::now();
    let delay = Duration::from_millis((500u64 << attempt.min(4)).min(8000));
    while began.elapsed() < delay {
        if cancel.load(Ordering::Acquire) {
            return Ok(true);
        }
        match commands.recv_timeout(Duration::from_millis(35)) {
            Ok(Command::Close) => return Err(
                "The shell is disconnected; retry its explicit close when the gateway is reachable"
                    .into(),
            ),
            Ok(Command::Detach) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(true),
            Ok(Command::Input(_) | Command::Drop(_)) => {
                return Err("Input was not sent while the remote terminal was disconnected".into());
            }
            _ => {}
        }
    }
    Ok(false)
}
fn close_remote(
    start: &Start,
    connection: &Connection,
    id: &str,
    notify: &impl Fn(Event),
) -> Result<(), String> {
    let mut socket = HostTerminal::connect_scoped(
        connection,
        &start.profile,
        start.profile_intent,
        "",
        start.cols,
        start.rows,
        Some(id),
        true,
    )?;
    let began = Instant::now();
    let mut acknowledged = false;
    while began.elapsed() < Duration::from_secs(10) {
        match socket.poll()? {
            Some(Frame::Meta {
                terminal_id,
                closed,
                ..
            }) => acknowledged = closed && terminal_id.as_deref() == Some(id),
            Some(Frame::Closed { code: 1000, .. }) if acknowledged => {
                notify(Event::Closed);
                return Ok(());
            }
            Some(Frame::Closed { .. }) => break,
            _ => {}
        }
    }
    Err("The remote shell close was not confirmed; its tab is retained for retry".into())
}
fn native(
    start: &Start,
    commands: &Receiver<Command>,
    cancel: &Arc<AtomicBool>,
    notify: &impl Fn(Event),
) -> Result<(), String> {
    let system = portable_pty::native_pty_system();
    let pair = system
        .openpty(portable_pty::PtySize {
            rows: start.rows,
            cols: start.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|_| "A native pseudo-terminal could not be opened")?;
    let (mut command, shell, cwd) = shell_command(start)?;
    for (key, _) in std::env::vars_os() {
        let key_string = key.to_string_lossy();
        if key_string.starts_with("npm_config_")
            || key_string.starts_with("npm_package_")
            || matches!(
                key_string.as_ref(),
                "NO_COLOR" | "FORCE_COLOR" | "COLORFGBG"
            )
        {
            command.env_remove(key);
        }
    }
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "Hermes");
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    command.env("HERMES_DESKTOP_TERMINAL", "1");
    if !cfg!(target_os = "macos") {
        command.env(
            "LC_CTYPE",
            std::env::var("LC_CTYPE")
                .or_else(|_| std::env::var("LANG"))
                .unwrap_or_else(|_| "C.UTF-8".into()),
        );
    }
    let mut child = ChildGuard(
        pair.slave
            .spawn_command(command)
            .map_err(|_| "The interactive shell could not start")?,
    );
    drop(pair.slave);
    let pid = child.process_id();
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|_| "Terminal output could not be attached")?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|_| "Terminal input could not be attached")?;
    #[cfg(target_os = "linux")]
    if let Some(raw) = pair.master.as_raw_fd() {
        use std::os::fd::BorrowedFd;
        // The master owns this fd throughout setup; all cloned readers share
        // its file-status flags, so cancellation can close them without a read.
        let fd = unsafe { BorrowedFd::borrow_raw(raw) };
        let flags = rustix::fs::fcntl_getfl(fd).map_err(|_| "The PTY flags are unavailable")?;
        rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)
            .map_err(|_| "The PTY reader could not be made cancellable")?;
    }
    let staged = dropfiles::Stager::new();
    let reader_cancel = cancel.clone();
    let (raw_send, raw) = mpsc::sync_channel(128);
    std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        loop {
            if reader_cancel.load(Ordering::Acquire) {
                break;
            }
            match reader.read(&mut bytes) {
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if raw_send.send(bytes[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    notify(Event::Started {
        shell,
        cwd,
        durable: None,
    });
    let mut pending = std::collections::VecDeque::new();
    let mut reader_done = false;
    let mut exit = None;
    let mut exit_time = None;
    let mut last_cwd = Instant::now();
    loop {
        let close = cancel.load(Ordering::Acquire);
        if close {
            let _ = child.kill();
            let _ = child.wait();
            notify(Event::Closed);
            return Ok(());
        }
        for command in commands.try_iter() {
            match command {
                Command::Input(bytes) => {
                    if pending.len() + bytes.len() > 4 * 1024 * 1024 {
                        notify(Event::Warning(
                            "The terminal input queue is full; the new input was not sent".into(),
                        ));
                    } else {
                        pending.extend(bytes);
                    }
                }
                Command::Resize(cols, rows) => pair
                    .master
                    .resize(portable_pty::PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .map_err(|_| "The terminal could not resize")?,
                Command::Close | Command::Detach => {
                    let _ = child.kill();
                    let _ = child.wait();
                    notify(Event::Closed);
                    return Ok(());
                }
                Command::Drop(paths) => {
                    if let Target::Ssh { connection, .. } = &start.target {
                        staged.start(
                            connection.clone(),
                            start.profile.clone(),
                            start.profile_intent,
                            paths,
                            cancel.clone(),
                        );
                    } else {
                        for path in paths {
                            pending.extend(
                                format!(
                                    "{} ",
                                    input::quote(
                                        &path.to_string_lossy(),
                                        shell_is_powershell(start)
                                    )
                                )
                                .into_bytes(),
                            );
                        }
                    }
                }
            }
        }
        for result in staged.poll() {
            match result {
                Ok(paths) => {
                    for path in paths {
                        pending.extend(
                            format!("{} ", input::quote(&path, shell_is_powershell(start)))
                                .into_bytes(),
                        );
                    }
                }
                Err(error) => notify(Event::Warning(error)),
            }
        }
        if !pending.is_empty() {
            let bytes = pending.make_contiguous();
            match writer.write(&bytes[..bytes.len().min(65536)]) {
                Ok(0) => return Err("The terminal input stream closed".into()),
                Ok(count) => {
                    pending.drain(..count);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return Err("The terminal input stream failed".into()),
            }
        }
        if reader_done {
            std::thread::sleep(Duration::from_millis(20));
        }
        match raw.recv_timeout(Duration::from_millis(20)) {
            Ok(bytes) => notify(Event::Data(bytes, false)),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                reader_done = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if exit.is_none() {
            if let Some(status) = child
                .try_wait()
                .map_err(|_| "The shell exit state is unavailable")?
            {
                exit = Some(status.exit_code());
                exit_time = Some(Instant::now());
            }
        }
        if reader_done && exit.is_some() {
            notify(Event::Exit(exit));
            return Ok(());
        }
        if exit_time.is_some_and(|time| time.elapsed() > Duration::from_millis(500)) {
            for bytes in raw.try_iter() {
                notify(Event::Data(bytes, false));
            }
            notify(Event::Exit(exit));
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        if matches!(start.target, Target::Local) && last_cwd.elapsed() > Duration::from_secs(2) {
            if let Some(pid) = pid {
                if let Ok(path) = std::fs::read_link(format!("/proc/{pid}/cwd")) {
                    notify(Event::Cwd(path.to_string_lossy().into_owned()));
                }
            }
            last_cwd = Instant::now();
        }
    }
}
fn shell_is_powershell(start: &Start) -> bool {
    matches!(&start.target, Target::Ssh { windows: true, .. }) || cfg!(target_os = "windows")
}
pub(super) fn shell_command(
    start: &Start,
) -> Result<(portable_pty::CommandBuilder, String, String), String> {
    #[cfg(test)]
    if let Target::Test(script) = &start.target {
        let mut command = portable_pty::CommandBuilder::new("/bin/sh");
        command.args(["-c", script]);
        command.cwd(std::env::temp_dir());
        return Ok((
            command,
            "synthetic-shell".into(),
            std::env::temp_dir().to_string_lossy().into_owned(),
        ));
    }
    if let Target::Ssh {
        config, windows, ..
    } = &start.target
    {
        if config.host.is_empty()
            || config.host.starts_with('-')
            || config
                .host
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
        {
            return Err("Invalid SSH terminal host".into());
        }
        let mut command = portable_pty::CommandBuilder::new("ssh");
        for argument in [
            "-tt",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ControlMaster=no",
            "-o",
            "ControlPath=none",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-p",
            &config.port.to_string(),
        ] {
            command.arg(argument);
        }
        if !config.user.is_empty() {
            command.args(["-l", &config.user]);
        }
        if let Some(key) = &config.key_file {
            command.arg("-i");
            command.arg(key);
        }
        command.arg("--");
        command.arg(&config.host);
        if !start.cwd.is_empty() {
            if *windows {
                use base64::Engine;
                let script = format!(
                    "Set-Location -LiteralPath {}",
                    input::quote(&start.cwd, true)
                );
                let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
                command.args([
                    "powershell",
                    "-NoLogo",
                    "-NoExit",
                    "-EncodedCommand",
                    &base64::engine::general_purpose::STANDARD.encode(bytes),
                ]);
            } else {
                command.arg(format!(
                    "cd -- {} && exec \"$SHELL\" -l",
                    input::quote(&start.cwd, false)
                ));
            }
        }
        return Ok((command, "ssh".into(), start.cwd.clone()));
    }
    let configured = std::env::var("HERMES_DESKTOP_SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            if cfg!(windows) {
                None
            } else {
                std::env::var("SHELL").ok()
            }
        });
    let program = configured.unwrap_or_else(|| {
        if cfg!(windows) {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
        } else {
            ["/bin/zsh", "/bin/bash", "/bin/sh"]
                .iter()
                .find(|s| std::path::Path::new(s).is_file())
                .unwrap_or(&"/bin/sh")
                .to_string()
        }
    });
    let shell = std::path::Path::new(&program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("shell")
        .to_owned();
    let mut command = portable_pty::CommandBuilder::new(program);
    if shell.contains("powershell") || shell.contains("pwsh") {
        command.arg("-NoLogo");
    } else if !shell.eq_ignore_ascii_case("cmd.exe") && !shell.eq_ignore_ascii_case("cmd") {
        command.arg(if shell.contains("bash") || shell.contains("zsh") {
            "-il"
        } else {
            "-i"
        });
    }
    let cwd = if start.cwd.is_empty() {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .ok_or("The local home directory is unavailable")?
    } else {
        let path = PathBuf::from(&start.cwd);
        if !path.is_absolute() || !path.is_dir() {
            return Err("Choose an existing absolute working directory for the local shell".into());
        }
        path
    };
    command.cwd(&cwd);
    Ok((command, shell, cwd.to_string_lossy().into_owned()))
}

struct ChildGuard(Box<dyn portable_pty::Child + Send + Sync>);
impl std::ops::Deref for ChildGuard {
    type Target = dyn portable_pty::Child + Send + Sync;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}
impl std::ops::DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.0
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
