use super::*;
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const OUTPUT: u64 = 16 * 1024 * 1024;
pub struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}
pub struct Git {
    pub cwd: PathBuf,
    pub binary: PathBuf,
    pub is_repo: bool,
}
pub fn binary(name: &str) -> Result<PathBuf, String> {
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    };
    let mut candidates = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(&file))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for folder in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        candidates.push(Path::new(folder).join(&file));
    }
    #[cfg(windows)]
    for key in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(key) {
            for rel in [
                format!("Git/cmd/{file}"),
                format!("GitHub CLI/{file}"),
                format!("Programs/Git/cmd/{file}"),
            ] {
                candidates.push(PathBuf::from(&base).join(rel));
            }
        }
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
        .ok_or_else(|| format!("{name} is not installed or is unavailable to this desktop process"))
}
pub fn run_program(program: &Path, args: &[&str], cwd: &Path) -> Result<Output, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if name.starts_with("GIT_")
            && !matches!(
                name.as_str(),
                "GIT_SSH" | "GIT_SSH_COMMAND" | "GIT_ASKPASS" | "GIT_TERMINAL_PROMPT"
            )
            || matches!(name.as_str(), "EDITOR" | "VISUAL" | "PAGER" | "PREFIX")
        {
            cmd.env_remove(key);
        }
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0").env("GIT_PAGER", "cat");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("The Git command could not start: {e}"))?;
    let out = child.stdout.take().ok_or("Git output pipe is missing")?;
    let err = child.stderr.take().ok_or("Git error pipe is missing")?;
    let output = std::thread::spawn(move || {
        let mut bytes = vec![];
        out.take(OUTPUT + 1).read_to_end(&mut bytes).map(|_| bytes)
    });
    let errors = std::thread::spawn(move || {
        let mut bytes = vec![];
        err.take(OUTPUT + 1).read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                timed_out = true;
                #[cfg(unix)]
                {
                    let _ = rustix::process::kill_process_group(
                        rustix::process::Pid::from_child(&child),
                        rustix::process::Signal::KILL,
                    );
                }
                let _ = child.kill();
                break child
                    .wait()
                    .map_err(|_| "The Git process could not be reaped")?;
            }
        }
    };
    let stdout = output
        .join()
        .map_err(|_| "Git output reader stopped")?
        .map_err(|_| "Git output could not be read")?;
    let stderr = errors
        .join()
        .map_err(|_| "Git error reader stopped")?
        .map_err(|_| "Git errors could not be read")?;
    if timed_out {
        return Err("The Git operation exceeded its 30-second timeout. Its result must be checked before retrying a write.".into());
    }
    if stdout.len() as u64 > OUTPUT || stderr.len() as u64 > OUTPUT {
        return Err("The Git output exceeded the desktop's 16 MB safe-load limit".into());
    }
    Ok(Output {
        code: status.code().unwrap_or(-1),
        stdout,
        stderr: String::from_utf8_lossy(&stderr).trim().into(),
    })
}
impl Git {
    pub fn new(path: &str) -> Result<Self, String> {
        let path = PathBuf::from(path);
        if !path.is_absolute() || path.to_string_lossy().contains('\0') {
            return Err("Choose an absolute local workspace path".into());
        }
        let cwd =
            std::fs::canonicalize(path).map_err(|_| "The local workspace could not be found")?;
        if !cwd.is_dir() {
            return Err("The workspace is not a directory".into());
        }
        let mut git = Self {
            cwd,
            binary: binary("git")?,
            is_repo: false,
        };
        git.is_repo = git.optional(&["rev-parse", "--git-dir"]).is_some();
        if let Some(root) = git.optional(&["rev-parse", "--show-toplevel"]) {
            git.cwd = PathBuf::from(root);
        }
        Ok(git)
    }
    pub fn output(&self, args: &[&str]) -> Result<Output, String> {
        let mut full = vec!["--no-pager"];
        full.extend_from_slice(args);
        run_program(&self.binary, &full, &self.cwd)
    }
    pub fn bytes(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let result = self.output(args)?;
        if result.code != 0 {
            return Err(if result.stderr.is_empty() {
                format!("Git exited with status {}", result.code)
            } else {
                result.stderr
            });
        }
        Ok(result.stdout)
    }
    pub fn text(&self, args: &[&str]) -> Result<String, String> {
        String::from_utf8(self.bytes(args)?)
            .map(|v| v.trim().to_owned())
            .map_err(|_| "Git returned non-UTF-8 data".into())
    }
    pub fn optional(&self, args: &[&str]) -> Option<String> {
        self.text(args).ok().filter(|s| !s.is_empty())
    }
    pub fn ok(&self, args: &[&str]) -> bool {
        self.output(args).is_ok_and(|o| o.code == 0)
    }
    pub fn branch(&self) -> Option<String> {
        self.optional(&["symbolic-ref", "--quiet", "--short", "HEAD"])
    }
    pub fn head(&self) -> Option<String> {
        self.optional(&["rev-parse", "--verify", "HEAD"])
    }
    pub fn default_branch(&self) -> Option<String> {
        if let Some(s) = self.optional(&[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ]) {
            return Some(s.trim_start_matches("origin/").into());
        }
        for b in ["main", "master"] {
            if self.ok(&[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{b}"),
            ]) || self.ok(&[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/remotes/origin/{b}"),
            ]) {
                return Some(b.into());
            }
        }
        None
    }
    pub fn ref_valid(&self, name: &str) -> bool {
        !name.is_empty()
            && !name.starts_with('-')
            && !name.contains('\0')
            && self.ok(&["check-ref-format", "--branch", name])
    }
}
