//! Linux private control attachment, without invoking gateway lifecycle commands.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::IpAddr,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use url::Url;

#[derive(Clone)]
pub(super) struct Endpoint {
    pub profile_id: PathBuf,
    pub origin: String,
    control_home: PathBuf,
    instance: String,
    served: Vec<PathBuf>,
}

impl Endpoint {
    pub fn profile_name(&self) -> String {
        if self
            .profile_id
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == "profiles")
        {
            self.profile_id
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "default".into())
        } else {
            "default".into()
        }
    }

    pub fn ticket(&self, home: &Path, purpose: &str) -> Result<String, String> {
        self.ticket_until(home, purpose, Instant::now() + Duration::from_secs(5))
    }

    pub fn ticket_until(
        &self,
        home: &Path,
        purpose: &str,
        deadline: Instant,
    ) -> Result<String, String> {
        if !matches!(purpose, "interactive" | "native-http") {
            return Err("Invalid read-only ticket purpose".into());
        }
        private_home(home)?;
        if fs::canonicalize(home).map_err(|_| "Profile home is unavailable")? != home
            || !self.served.iter().any(|served| served == home)
        {
            return Err("Profile is not bound to this gateway owner".into());
        }
        let home = home
            .to_str()
            .ok_or("Profile home is not a supported UTF-8 path")?;
        let mut params =
            json!({"profile_id": home, "instance_id": self.instance, "purpose": purpose});
        if purpose == "interactive" {
            params["scope"] = json!("host");
        }
        let value = control(&self.control_home, "session-ticket", Some(params), deadline)?;
        if value.get("profile_id").and_then(Value::as_str) != Some(home)
            || value.get("instance_id").and_then(Value::as_str) != Some(&self.instance)
        {
            return Err("Gateway ticket identity mismatch; reconnect".into());
        }
        let ticket = value
            .get("ticket")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 4096)
            .ok_or("Gateway returned an invalid ticket")?;
        if !ticket
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err("Gateway returned an invalid ticket".into());
        }
        Ok(ticket.into())
    }
}

pub(super) fn identify(requested: &Path) -> Result<Endpoint, String> {
    identify_until(requested, Instant::now() + Duration::from_secs(5))
}

pub(super) fn identify_until(requested: &Path, deadline: Instant) -> Result<Endpoint, String> {
    let home = fs::canonicalize(requested).map_err(|_| "Local Hermes home is unavailable")?;
    private_home(&home)?;
    let published = [home.join("gateway.sock"), home.join("gateway.sock.path")]
        .iter()
        .map(|path| {
            fs::symlink_metadata(path).map(|_| true).or_else(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    Ok(false)
                } else {
                    Err(error)
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Gateway control path is inaccessible")?;
    let control_home = if published.into_iter().any(|present| present) {
        socket_path(&home)?;
        home.clone()
    } else if home
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "profiles")
    {
        let root = home
            .parent()
            .and_then(Path::parent)
            .ok_or("Invalid profile home")?
            .to_path_buf();
        private_home(&root)?;
        // A served secondary may be owned by the root multiplexer's private socket.
        socket_path(&root)?;
        root
    } else {
        return Err("No private gateway control socket; start Hermes separately".into());
    };
    let value = control(&control_home, "identify", None, deadline)?;
    if value.get("runtime_protocol").and_then(Value::as_u64) != Some(1)
        || value.get("state").and_then(Value::as_str) != Some("ready")
        || !value
            .get("capabilities")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|c| c == "session-authority-v1"))
        || value
            .get("authority_epoch")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            == 0
    {
        return Err("Gateway is not ready or lacks the native session authority protocol".into());
    }
    let instance = value
        .get("instance_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or("Invalid gateway instance identity")?
        .to_owned();
    let mut served = Vec::new();
    for row in value
        .get("served_profiles")
        .and_then(Value::as_array)
        .ok_or("Gateway did not advertise served profiles")?
    {
        let id = row
            .get("profile_id")
            .and_then(Value::as_str)
            .ok_or("Invalid served profile identity")?;
        if row.get("home").and_then(Value::as_str) != Some(id) {
            return Err("Inconsistent served profile identity".into());
        }
        let path = PathBuf::from(id);
        if !path.is_absolute() {
            return Err("Invalid served profile path".into());
        }
        served.push(path);
    }
    if served.iter().filter(|p| **p == home).count() != 1 {
        return Err("Gateway does not own the selected Hermes home".into());
    }
    let origin = value
        .get("api_origin")
        .and_then(Value::as_str)
        .ok_or("Gateway did not advertise an API origin")?;
    let url = Url::parse(origin).map_err(|_| "Invalid local gateway API origin")?;
    let host = url
        .host_str()
        .and_then(|h| h.trim_matches(['[', ']']).parse::<IpAddr>().ok());
    if !matches!(url.scheme(), "http" | "https")
        || !host.is_some_and(|ip| ip.is_loopback())
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Native gateway API origin must be an explicit loopback origin".into());
    }
    Ok(Endpoint {
        profile_id: home,
        origin: url.to_string(),
        control_home,
        instance,
        served,
    })
}

fn private_home(path: &Path) -> Result<(), String> {
    private_node(path, Kind::Home).map(|_| ())
}
#[derive(Clone, Copy)]
enum Kind {
    Home,
    Directory,
    Socket,
    File,
}

fn private_node(path: &Path, kind: Kind) -> Result<fs::Metadata, String> {
    let meta =
        fs::symlink_metadata(path).map_err(|_| "Private gateway control path is unavailable")?;
    let matches = match kind {
        Kind::Home | Kind::Directory => meta.is_dir(),
        Kind::Socket => meta.file_type().is_socket(),
        Kind::File => meta.is_file(),
    };
    if !matches || meta.uid() != rustix::process::geteuid().as_raw() {
        return Err("Unsafe gateway control path type or owner".into());
    }
    let forbidden = if matches!(kind, Kind::Home) {
        0o022
    } else {
        0o077
    };
    if meta.mode() & forbidden != 0 {
        return Err("Unsafe gateway control path permissions; no permissions were changed".into());
    }
    Ok(meta)
}

fn socket_path(home: &Path) -> Result<PathBuf, String> {
    private_home(home)?;
    let socket = home.join("gateway.sock");
    match fs::symlink_metadata(&socket) {
        Ok(_) => {
            private_node(&socket, Kind::Socket)?;
            return Ok(socket);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Gateway control socket is inaccessible".into()),
    }
    let pointer = home.join("gateway.sock.path");
    let before = private_node(&pointer, Kind::File)?;
    let file = fs::File::open(&pointer).map_err(|_| "Could not read gateway socket pointer")?;
    let after = file
        .metadata()
        .map_err(|_| "Could not validate gateway socket pointer")?;
    if before.dev() != after.dev() || before.ino() != after.ino() {
        return Err("Gateway socket pointer changed during validation".into());
    }
    let mut target = String::new();
    file.take(4097)
        .read_to_string(&mut target)
        .map_err(|_| "Invalid gateway socket pointer")?;
    if target.len() > 4096 {
        return Err("Oversized gateway socket pointer".into());
    }
    let target = PathBuf::from(target.trim());
    let home = home.to_str().ok_or("Unsupported profile home encoding")?;
    let hash = format!("{:x}", Sha256::digest(home.as_bytes()));
    let dirname = format!("hermes-gw-{}", &hash[..16]);
    if !target.is_absolute()
        || target.file_name().is_none_or(|n| n != "control.sock")
        || target
            .parent()
            .and_then(Path::file_name)
            .is_none_or(|n| n != dirname.as_str())
    {
        return Err("Gateway socket pointer is not bound to this home".into());
    }
    private_node(
        target.parent().ok_or("Invalid socket pointer")?,
        Kind::Directory,
    )?;
    private_node(&target, Kind::Socket)?;
    Ok(target)
}

fn control(
    home: &Path,
    verb: &str,
    params: Option<Value>,
    deadline: Instant,
) -> Result<Value, String> {
    let path = socket_path(home)?;
    let before = private_node(&path, Kind::Socket)?;
    let fd = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        rustix::net::SocketFlags::CLOEXEC | rustix::net::SocketFlags::NONBLOCK,
        None,
    )
    .map_err(|_| "Could not create private gateway control connection")?;
    let address =
        rustix::net::SocketAddrUnix::new(&path).map_err(|_| "Invalid gateway socket path")?;
    rustix::net::connect(&fd, &address)
        .map_err(|_| "Private gateway control unavailable or busy; retry")?;
    let mut stream = UnixStream::from(fd);
    stream
        .set_nonblocking(false)
        .map_err(|_| "Could not initialize gateway control connection")?;
    let after = private_node(&path, Kind::Socket)?;
    if before.dev() != after.dev() || before.ino() != after.ino() {
        return Err("Gateway control socket changed during connection".into());
    }
    let peer = rustix::net::sockopt::socket_peercred(&stream)
        .map_err(|_| "Could not verify gateway control peer")?;
    if peer.uid != rustix::process::geteuid() {
        return Err("Gateway control peer belongs to another user".into());
    }
    let mut request = json!({"protocol": 1, "id": 1, "verb": verb});
    if let Some(params) = params {
        request["params"] = params;
    }
    let timeout = deadline
        .checked_duration_since(Instant::now())
        .ok_or("Gateway control request timed out")?
        .min(Duration::from_secs(5));
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|_| "Could not bound gateway control writes")?;
    stream
        .write_all(format!("{request}\n").as_bytes())
        .map_err(|_| "Gateway control write failed")?;
    let mut bytes = Vec::new();
    while !bytes.contains(&b'\n') {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("Gateway control request timed out")?
            .min(Duration::from_secs(5));
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|_| "Could not bound gateway control reads")?;
        let mut chunk = [0; 4096];
        let read = stream
            .read(&mut chunk)
            .map_err(|_| "Gateway control read failed or timed out")?;
        if read == 0 {
            return Err("Gateway control closed before a response".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > 65536 {
            return Err("Oversized gateway control response".into());
        }
    }
    let newline = bytes
        .iter()
        .position(|b| *b == b'\n')
        .ok_or("Incomplete gateway control response")?;
    let reply: Value = serde_json::from_slice(&bytes[..newline])
        .map_err(|_| "Invalid gateway control response")?;
    if reply.get("protocol").and_then(Value::as_u64) != Some(1)
        || reply.get("id").and_then(Value::as_u64) != Some(1)
        || reply.get("ok").and_then(Value::as_bool) != Some(true)
    {
        return Err("Gateway control refused the read-only attachment".into());
    }
    let value = reply
        .get("result")
        .filter(|v| v.is_object())
        .ok_or("Invalid gateway control result")?;
    if verb == "identify"
        && value.get("pid").and_then(Value::as_u64) != Some(peer.pid.as_raw_nonzero().get() as u64)
    {
        return Err("Gateway identify process does not match its control peer".into());
    }
    Ok(value.clone())
}
