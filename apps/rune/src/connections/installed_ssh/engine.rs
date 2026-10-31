//! Sender-free lifetime for an installed remote dashboard and its SSH forward.
use super::{Cancellation, Forward, Ssh, http, remote, remote_path};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

/// This value is transient authentication material, not a saved route.
/// It intentionally has no Debug or serialization implementation.
#[derive(Clone)]
pub(crate) struct Ready {
    pub endpoint: String,
    pub token: String,
    pub resource: Resource,
    pub provenance: Provenance,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Provenance {
    Canonical,
    Owned { nonce: String },
    ReusedExternal { nonce: String },
}
/// Durable secret seam. Implementations must make each operation atomic and
/// reject stale scope/revision or generation mismatches. Errors are redacted.
#[derive(Clone)]
pub(crate) struct StoredCredential {
    pub nonce: String,
    pub spawn_token: String,
    pub served_token: Option<String>,
}
/// `Pending` means the OS bootstrap lock is held, even before a record exists.
pub(crate) enum CredentialState {
    Pending,
    Missing,
    Ready(StoredCredential),
}
/// A shared cross-process lifetime hold on the same OS lock for this stable
/// installation/scope owner. `try_last` releases this shared hold and attempts
/// exclusive ownership without blocking; true retains that exclusive hold
/// until drop. Every caller must hold its bootstrap claim before `try_last`.
pub(crate) trait LifetimeLease: Send {
    fn try_last(&mut self) -> Result<bool, String>;
}
pub(crate) trait CredentialStore: Send + Sync {
    /// Preserve both tokens until generation cleanup; the nonce and spawn token
    /// are the compare-and-swap key even after served-token reconciliation.
    fn load(&self, owner_id: &str) -> Result<CredentialState, String>;
    /// Holds the per-owner OS lock through remote publication.
    fn reserve(&self, owner_id: &str, nonce: &str, token: &str) -> Result<(), String>;
    fn claim(&self, owner_id: &str, nonce: &str, expected_token: &str) -> Result<(), String>;
    /// Releases the held scope lock without deleting the secret.
    fn finish(&self, owner_id: &str, nonce: &str, expected_token: &str) -> Result<(), String>;
    /// Called while holding the bootstrap claim. Acquire shared lifetime before
    /// releasing that claim; failure retains the claim for `finish`/`abort`.
    fn activate(
        &self,
        owner_id: &str,
        nonce: &str,
        expected_token: &str,
    ) -> Result<Box<dyn LifetimeLease>, String>;
    fn reconcile(
        &self,
        owner_id: &str,
        nonce: &str,
        expected_token: &str,
        served_token: &str,
    ) -> Result<(), String>;
    fn abort(&self, owner_id: &str, nonce: &str, expected_token: &str) -> Result<(), String>;
}
struct Owned {
    ssh: Ssh,
    owner: String,
    owner_id: String,
    marker: String,
    launcher: String,
    home: String,
    nonce: String,
    spawn_token: String,
    store: Arc<dyn CredentialStore>,
    lease: Box<dyn LifetimeLease>,
    reused: bool,
    supervised: bool,
}
impl Owned {
    fn cleanup(mut self) -> Result<(), String> {
        // Global lock order: bootstrap claim, then lifetime, then store CAS.
        self.store
            .claim(&self.owner_id, &self.nonce, &self.spawn_token)
            .map_err(|_| "Could not claim SSH cleanup")?;
        let last = match self.lease.try_last() {
            Ok(value) => value,
            Err(_) => {
                self.store
                    .finish(&self.owner_id, &self.nonce, &self.spawn_token)
                    .map_err(|_| "Could not release SSH cleanup claim")?;
                return Err("Could not prove SSH lifetime ownership".into());
            }
        };
        if !last {
            return self
                .store
                .finish(&self.owner_id, &self.nonce, &self.spawn_token)
                .map_err(|_| "Could not release SSH cleanup claim".into());
        }
        if !self.supervised {
            return self
                .store
                .finish(&self.owner_id, &self.nonce, &self.spawn_token)
                .map_err(|_| "Could not release SSH cleanup claim".into());
        }
        if remote::cleanup_dashboard(&self.ssh, &self.owner, &self.nonce).is_err() {
            let recovered = remote::reclaim_dead_generation(
                &self.ssh,
                &self.marker,
                &self.launcher,
                &self.owner,
                &self.home,
                self.ssh.spec.remote_profile.as_deref(),
                Some(&self.nonce),
                &Cancellation::default(),
            );
            if !matches!(recovered, Ok(Some(ref nonce)) if nonce == &self.nonce) {
                self.store
                    .finish(&self.owner_id, &self.nonce, &self.spawn_token)
                    .map_err(|_| "Could not release SSH cleanup claim")?;
                return Err("Could not prove SSH generation cleanup".into());
            }
        }
        self.store
            .abort(&self.owner_id, &self.nonce, &self.spawn_token)
            .map_err(|_| "SSH credential cleanup failed".to_string())
    }
}
pub(super) struct Live {
    forward: Forward,
    owned: Option<Owned>,
    endpoint: String,
    token: String,
    provenance: Provenance,
}
impl Drop for Live {
    fn drop(&mut self) {
        if let Some(owned) = self.owned.take() {
            let _ = owned.cleanup();
        }
        self.forward.close();
    }
}
#[derive(Clone)]
pub(crate) struct Resource(pub(super) Option<Arc<Mutex<Live>>>);
impl Resource {
    pub(crate) fn exited(&mut self) -> Option<String> {
        match self.0.as_ref()?.lock() {
            Ok(mut live) => live.forward.exited(),
            Err(_) => Some("SSH connection lifetime failed".into()),
        }
    }
    pub(crate) fn close(&mut self) -> Result<(), String> {
        let Some(shared) = self.0.take() else {
            return Ok(());
        };
        let Ok(mutex) = Arc::try_unwrap(shared) else {
            return Ok(());
        };
        let mut live = mutex
            .into_inner()
            .map_err(|_| "SSH connection lifetime failed")?;
        let result = match live.owned.take() {
            Some(owned) => owned.cleanup(),
            None => Ok(()),
        };
        live.forward.close();
        result
    }
}
fn check(cancel: &Cancellation) -> Result<(), String> {
    if cancel.cancelled() {
        Err("SSH operation cancelled".into())
    } else {
        Ok(())
    }
}
fn ready(forward: Forward, token: String, owned: Option<Owned>) -> Ready {
    let endpoint = format!("http://127.0.0.1:{}", forward.local_port());
    let provenance = match &owned {
        Some(owned) if owned.reused => Provenance::ReusedExternal {
            nonce: owned.nonce.clone(),
        },
        Some(owned) => Provenance::Owned {
            nonce: owned.nonce.clone(),
        },
        None => Provenance::Canonical,
    };
    Ready {
        endpoint: endpoint.clone(),
        token: token.clone(),
        resource: Resource(Some(Arc::new(Mutex::new(Live {
            forward,
            owned,
            endpoint: endpoint.clone(),
            token: token.clone(),
            provenance: provenance.clone(),
        })))),
        provenance,
    }
}
fn checked_home(raw: &str) -> Result<String, String> {
    let home = raw;
    remote_path(home)?;
    if !(home.starts_with('/') || home.starts_with("~/"))
        || home.chars().any(char::is_control)
        || home.split('/').any(|part| part == "." || part == "..")
    {
        return Err("Remote Hermes home is unsafe".into());
    }
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return Err("Remote Hermes home is unsafe".into());
    }
    Ok(home.to_owned())
}

fn remote_homes(
    ssh: &Ssh,
    launcher: &str,
    cancel: &Cancellation,
) -> Result<(String, String), String> {
    // Explicit default prevents CLI sticky-profile selection from changing
    // this process-home query; the spawn itself still omits the default arg.
    let executable = remote_path(launcher)?;
    let command = format!("env -u HERMES_HOME {executable} --profile default config path");
    let output = ssh.exec(
        &remote::watchdog(&command),
        None,
        Duration::from_secs(18),
        cancel,
    )?;
    let path = output
        .lines()
        .last()
        .ok_or("Remote Hermes did not report its config path")?;
    let parent = path
        .strip_suffix("/config.yaml")
        .ok_or("Remote Hermes reported an invalid config path")?;
    let machine_home = checked_home(parent)?;
    let explicit = ssh.exec(
        "printf '%s' \"${HERMES_HOME-}\"",
        None,
        Duration::from_secs(15),
        cancel,
    )?;
    let launch_home = if explicit.is_empty() {
        machine_home.clone()
    } else {
        checked_home(&explicit)?
    };
    Ok((launch_home, machine_home))
}
fn install_root(home: &str) -> &str {
    if let Some((root, profile)) = home.rsplit_once("/profiles/") {
        if !profile.is_empty() && !profile.contains('/') {
            return root;
        }
    }
    home
}
/// The same stable per-installation namespace as retained Desktop. The SSH-G
/// fingerprint belongs to the pool key, never to this ownership ID.
pub(crate) fn ownership_id(installation_id: &str, scope: &str) -> Result<String, String> {
    let uuid =
        Uuid::parse_str(installation_id).map_err(|_| "Invalid desktop installation identity")?;
    let bytes = installation_id.as_bytes();
    if bytes.len() != 36
        || [8, 13, 18, 23].iter().any(|i| bytes[*i] != b'-')
        || uuid.get_version_num() != 4
        || uuid.get_variant() != uuid::Variant::RFC4122
    {
        return Err("Invalid desktop installation identity".into());
    }
    let mut digest = Sha256::new();
    digest.update(installation_id.as_bytes());
    digest.update([0]);
    digest.update(scope.as_bytes());
    Ok(format!("{:x}", digest.finalize())[..32].to_string())
}
pub(super) fn ownership(ssh: &Ssh, effective: &str, owner_id: &str) -> String {
    let mut digest = Sha256::new();
    for value in [
        owner_id,
        &ssh.spec.host,
        &ssh.spec.user,
        &ssh.spec.port.to_string(),
        ssh.spec
            .key_path
            .as_ref()
            .and_then(|x| x.to_str())
            .unwrap_or(""),
        ssh.spec.remote_hermes_path.as_deref().unwrap_or(""),
        ssh.spec.remote_profile.as_deref().unwrap_or(""),
        effective,
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    format!("{:x}", digest.finalize())[..32].to_string()
}
fn wait_port(ssh: &Ssh, log: &str, cancel: &Cancellation) -> Result<u16, String> {
    let deadline = Instant::now() + Duration::from_secs(120);
    let re = regex::Regex::new(r"(?:^|[^[:alnum:]_])HERMES_(?:BACKEND|DASHBOARD)_READY port=(\d+)")
        .expect("constant ready regex");
    while Instant::now() < deadline {
        check(cancel)?;
        let command = format!("cat {} 2>/dev/null || true", remote_path(log)?);
        let output = ssh.exec(&command, None, Duration::from_secs(10), cancel)?;
        if let Some(port) = re
            .captures(&output)
            .and_then(|c| c[1].parse::<u16>().ok())
            .filter(|p| *p > 0)
        {
            return Ok(port);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("Remote dashboard did not become ready".into())
}

/// Resolve the external installed launcher, attach to a discovered canonical
/// dashboard if one exists, or start an isolated nonce-owned dashboard.
#[cfg(all(test, unix))]
pub(crate) fn connect(ssh: Ssh, owner_id: &str, cancel: &Cancellation) -> Result<Ready, String> {
    connect_with_store(ssh, owner_id, test_store(), cancel)
}
pub(crate) fn connect_with_store(
    ssh: Ssh,
    owner_id: &str,
    store: Arc<dyn CredentialStore>,
    cancel: &Cancellation,
) -> Result<Ready, String> {
    let effective = ssh.effective_identity(cancel)?;
    connect_prepared(ssh, owner_id, store, &effective, cancel)
}
pub(super) fn connect_prepared(
    ssh: Ssh,
    owner_id: &str,
    store: Arc<dyn CredentialStore>,
    expected_effective: &str,
    cancel: &Cancellation,
) -> Result<Ready, String> {
    check(cancel)?;
    if owner_id.len() != 32
        || !owner_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Invalid SSH ownership identity".into());
    }
    let effective = ssh.effective_identity(cancel)?;
    if effective != expected_effective {
        return Err("Effective SSH configuration changed while connecting".into());
    }
    let platform = ssh.exec("uname -s; uname -m", None, Duration::from_secs(15), cancel)?;
    let lines: Vec<_> = platform.lines().collect();
    let os = lines
        .get(lines.len().saturating_sub(2))
        .copied()
        .ok_or("Could not determine remote platform")?;
    if !matches!(os, "Linux" | "Darwin") {
        return Err("The remote SSH platform is not supported".into());
    }
    let launcher = ssh.locate_hermes(cancel)?;
    let (home, machine_home) = remote_homes(&ssh, &launcher, cancel)?;
    let marker = format!("{}/.hermes-update-in-progress", install_root(&home));
    remote::probe_update_gate(&ssh, &marker, &launcher, cancel)?;
    let candidates = remote::canonical_candidates(&ssh, &machine_home, cancel)?;
    for port in &candidates {
        check(cancel)?;
        if let Ok(forward) = ssh.forward(*port, Duration::from_secs(15), cancel) {
            if let Ok(verified) = http::verify(forward.local_port(), None, None, cancel) {
                check(cancel)?;
                return Ok(ready(forward, verified.token, None));
            }
        }
    }
    if !candidates.is_empty() {
        return Err("A remote dashboard exists but its connection could not be verified".into());
    }
    check(cancel)?;
    if os != "Linux" {
        return Err("Safe remote SSH bootstrap is unavailable on this platform".into());
    }
    let account_home = ssh.exec(
        "printf '%s' \"$HOME\"",
        None,
        Duration::from_secs(15),
        cancel,
    )?;
    let account_home = checked_home(&account_home)?;
    // Preserve Electron's tilde spelling for a home-relative lock record.
    let lock_root = match machine_home.strip_prefix(&format!("{account_home}/")) {
        Some(relative) => format!("~/{relative}"),
        None => machine_home.clone(),
    };
    let owner = format!("{lock_root}/desktop-ssh/{owner_id}");
    let wait_deadline = Instant::now() + Duration::from_secs(120);
    let mut stored = loop {
        check(cancel)?;
        match store
            .load(owner_id)
            .map_err(|_| "Could not load SSH credentials")?
        {
            CredentialState::Missing => break None,
            CredentialState::Ready(saved) => break Some(saved),
            CredentialState::Pending => {
                if Instant::now() >= wait_deadline {
                    return Err("Another SSH bootstrap is still active".into());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    };
    let reclaimed = remote::reclaim_dead_generation(
        &ssh,
        &marker,
        &launcher,
        &owner,
        &home,
        ssh.spec.remote_profile.as_deref(),
        stored.as_ref().map(|saved| saved.nonce.as_str()),
        cancel,
    )?;
    if let (Some(nonce), Some(saved)) = (&reclaimed, &stored) {
        if *nonce == saved.nonce {
            store
                .abort(owner_id, &saved.nonce, &saved.spawn_token)
                .map_err(|_| "SSH credential cleanup failed")?;
            stored = None;
        }
    }
    let reservation_deadline = Instant::now() + Duration::from_secs(120);
    let record = loop {
        check(cancel)?;
        let record = remote::inspect_generation(&ssh, &owner, cancel)?;
        let pending = record
            .as_ref()
            .and_then(|r| r.get("port"))
            .and_then(serde_json::Value::as_u64)
            == Some(0);
        // A durable matching secret lets us complete publication after a crash.
        let recoverable = record
            .as_ref()
            .and_then(|r| r.get("spawnNonce"))
            .and_then(serde_json::Value::as_str)
            .zip(stored.as_ref())
            .is_some_and(|(nonce, saved)| nonce == saved.nonce);
        if !pending || recoverable {
            break record;
        }
        if Instant::now() >= reservation_deadline {
            return Err("The concurrent remote SSH generation did not become ready".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    if let Some(record) = record {
        return attach_existing(
            &ssh,
            owner_id,
            &owner,
            &marker,
            &launcher,
            &home,
            &store,
            stored.as_ref(),
            record,
            cancel,
        );
    }
    if let Some(saved) = &stored {
        store
            .claim(owner_id, &saved.nonce, &saved.spawn_token)
            .map_err(|_| "Could not claim SSH credential recovery")?;
        // Crash before remote spawn: settle the matching nonce under the same
        // mutex before clearing its durable reservation.
        if remote::fence_cleanup(&ssh, &marker, &launcher, &owner, &saved.nonce).is_err() {
            store
                .finish(owner_id, &saved.nonce, &saved.spawn_token)
                .map_err(|_| "Could not release SSH credential recovery")?;
            return Err("SSH credential recovery could not be proved".into());
        }
        if store
            .abort(owner_id, &saved.nonce, &saved.spawn_token)
            .is_err()
        {
            let _ = store.finish(owner_id, &saved.nonce, &saved.spawn_token);
            return Err("SSH credential cleanup failed".into());
        }
    }
    remote::require_ownership_capability(&ssh, &launcher, cancel)?;
    let nonce = Uuid::new_v4().simple().to_string()[..16].to_owned();
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let token_path = format!("{owner}/{nonce}.token");
    let log = format!("{owner}/{nonce}.log");
    // Reserve before any remote side effect so a crash leaves a durable nonce
    // and spawn credential, not an unidentifiable live backend.
    store
        .reserve(owner_id, &nonce, &token)
        .map_err(|_| "Could not reserve SSH credentials")?;
    let mut activated = false;
    let attempt = (|| {
        check(cancel)?;
        // Do not interrupt publication: fence any uncertain result afterward.
        remote::upload_token(
            &ssh,
            &token_path,
            token.as_bytes(),
            &Cancellation::default(),
        )?;
        check(cancel)?;
        let pid = remote::spawn_dashboard(
            &ssh,
            &marker,
            &launcher,
            &owner,
            &nonce,
            ssh.spec.remote_profile.as_deref(),
            &home,
            &Cancellation::default(),
        )?;
        check(cancel)?;
        let port = wait_port(&ssh, &log, cancel)?;
        let forward = ssh.forward(port, Duration::from_secs(15), cancel)?;
        let verified = http::verify(forward.local_port(), Some(&token), Some(&nonce), cancel)?;
        check(cancel)?;
        store
            .reconcile(owner_id, &nonce, &token, &verified.token)
            .map_err(|_| "Could not reconcile SSH credentials")?;
        check(cancel)?;
        remote::publish_dashboard(&ssh, &owner, &nonce, port, &verified.token, cancel)?;
        check(cancel)?;
        let lease = store
            .activate(owner_id, &nonce, &token)
            .map_err(|_| "Could not activate SSH lifetime")?;
        activated = true;
        let owned = Owned {
            ssh: ssh.clone(),
            owner: owner.clone(),
            owner_id: owner_id.to_string(),
            marker: marker.clone(),
            launcher: launcher.clone(),
            home: home.clone(),
            nonce: nonce.clone(),
            spawn_token: token.clone(),
            store: store.clone(),
            lease,
            reused: false,
            supervised: true,
        };
        let final_record = remote::inspect_generation(&ssh, &owner, cancel);
        let valid = final_record
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .is_some_and(|record| {
                record.get("pid").and_then(serde_json::Value::as_u64) == Some(u64::from(pid))
                    && record.get("spawnNonce").and_then(serde_json::Value::as_str)
                        == Some(nonce.as_str())
            });
        if !valid || cancel.cancelled() {
            owned
                .cleanup()
                .map_err(|_| "Could not release SSH lifetime safely")?;
            return Err(if cancel.cancelled() {
                "SSH operation cancelled"
            } else {
                "Remote SSH generation changed during activation"
            }
            .into());
        }
        Ok(ready(forward, verified.token, Some(owned)))
    })();
    if attempt.is_err() && !activated {
        // Leave the reservation in the durable store if remote fencing fails.
        if remote::fence_cleanup(&ssh, &marker, &launcher, &owner, &nonce).is_err() {
            store
                .finish(owner_id, &nonce, &token)
                .map_err(|_| "Could not release SSH credential reservation")?;
            return Err(
                "SSH bootstrap failed and its owned generation could not be safely cleaned".into(),
            );
        }
        if store.abort(owner_id, &nonce, &token).is_err() {
            let _ = store.finish(owner_id, &nonce, &token);
            return Err("SSH credential cleanup failed".into());
        }
    }
    attempt
}

fn attach_existing(
    ssh: &Ssh,
    owner_id: &str,
    owner: &str,
    marker: &str,
    launcher: &str,
    home: &str,
    store: &Arc<dyn CredentialStore>,
    stored: Option<&StoredCredential>,
    record: serde_json::Value,
    cancel: &Cancellation,
) -> Result<Ready, String> {
    let expected_profile =
        super::profile_arg(ssh.spec.remote_profile.as_deref())?.unwrap_or_default();
    let nonce = record
        .get("spawnNonce")
        .and_then(serde_json::Value::as_str)
        .ok_or("Invalid SSH ownership record")?;
    let saved = stored
        .filter(|saved| saved.nonce == nonce && !saved.spawn_token.is_empty())
        .ok_or("A matching session credential is required to reuse this SSH generation")?;
    let fingerprint = record
        .get("tokenFingerprint")
        .and_then(serde_json::Value::as_str)
        .ok_or("Invalid SSH ownership record")?;
    let matches_fingerprint = std::iter::once(&saved.spawn_token)
        .chain(saved.served_token.iter())
        .any(|token| &format!("{:x}", Sha256::digest(token.as_bytes()))[..32] == fingerprint);
    if !matches_fingerprint
        || record.get("hermesPath").and_then(serde_json::Value::as_str) != Some(launcher)
        || record.get("hermesHome").and_then(serde_json::Value::as_str) != Some(home)
        || record.get("profile").and_then(serde_json::Value::as_str)
            != Some(expected_profile.as_str())
    {
        return Err("The remote SSH generation does not match this route".into());
    }
    store
        .claim(owner_id, nonce, &saved.spawn_token)
        .map_err(|_| "Could not claim SSH credential recovery")?;
    let mut activated = false;
    let recovery = (|| -> Result<Ready, String> {
        let port = match record
            .get("port")
            .and_then(serde_json::Value::as_u64)
            .and_then(|p| u16::try_from(p).ok())
        {
            Some(0) => wait_port(ssh, &format!("{owner}/{nonce}.log"), cancel)?,
            Some(port) => port,
            None => return Err("Invalid SSH ownership record".into()),
        };
        let forward = ssh.forward(port, Duration::from_secs(15), cancel)?;
        let verified = http::verify(
            forward.local_port(),
            Some(&saved.spawn_token),
            Some(nonce),
            cancel,
        )?;
        check(cancel)?;
        let after = remote::inspect_generation(ssh, &owner, cancel)?
            .ok_or("Remote SSH generation exited during verification")?;
        if after.get("pid") != record.get("pid")
            || after.get("spawnNonce") != record.get("spawnNonce")
            || after.get("creationTime") != record.get("creationTime")
        {
            return Err("Remote SSH generation changed during verification".into());
        }
        store
            .reconcile(owner_id, nonce, &saved.spawn_token, &verified.token)
            .map_err(|_| "Could not reconcile SSH credentials")?;
        let served_fingerprint = format!("{:x}", Sha256::digest(verified.token.as_bytes()));
        if record.get("port").and_then(serde_json::Value::as_u64) == Some(0)
            || fingerprint != &served_fingerprint[..32]
        {
            remote::publish_dashboard(ssh, &owner, nonce, port, &verified.token, cancel)?;
        }
        check(cancel)?;
        let lease = store
            .activate(owner_id, nonce, &saved.spawn_token)
            .map_err(|_| "Could not activate SSH lifetime")?;
        activated = true;
        let owned = Owned {
            ssh: (*ssh).clone(),
            owner: owner.to_string(),
            owner_id: owner_id.to_string(),
            marker: marker.to_string(),
            launcher: launcher.to_string(),
            home: home.to_string(),
            nonce: nonce.to_string(),
            spawn_token: saved.spawn_token.clone(),
            store: store.clone(),
            lease,
            reused: true,
            supervised: record
                .get("supervisorProtocol")
                .and_then(serde_json::Value::as_u64)
                == Some(1),
        };
        let final_record = remote::inspect_generation(ssh, &owner, cancel);
        let valid = final_record
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .is_some_and(|after| {
                after.get("pid") == record.get("pid")
                    && after.get("spawnNonce") == record.get("spawnNonce")
                    && after.get("creationTime") == record.get("creationTime")
            });
        if !valid || cancel.cancelled() {
            owned
                .cleanup()
                .map_err(|_| "Could not release SSH lifetime safely")?;
            return Err(if cancel.cancelled() {
                "SSH operation cancelled"
            } else {
                "Remote SSH generation changed during activation"
            }
            .into());
        }
        Ok(ready(forward, verified.token, Some(owned)))
    })();
    // Preserve the exact credential on failed recovery, but release the
    // claim so the next attempt can retry safely.
    if !activated {
        store
            .finish(owner_id, nonce, &saved.spawn_token)
            .map_err(|_| "Could not release SSH credential recovery")?;
    }
    recovery
}

pub(super) fn from_live(live: Arc<Mutex<Live>>) -> Result<Ready, String> {
    let mut state = live.lock().map_err(|_| "SSH connection lifetime failed")?;
    if state.forward.exited().is_some() {
        return Err("SSH forward exited".into());
    }
    let ready = Ready {
        endpoint: state.endpoint.clone(),
        token: state.token.clone(),
        provenance: state.provenance.clone(),
        resource: Resource(Some(live.clone())),
    };
    Ok(ready)
}
#[cfg(all(test, target_os = "linux"))]
#[path = "engine_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
struct TestStore(
    Mutex<std::collections::HashMap<String, (StoredCredential, bool)>>,
    std::path::PathBuf,
);
#[cfg(all(test, unix))]
impl Default for TestStore {
    fn default() -> Self {
        let path = std::env::temp_dir().join(format!("rune-ssh-lock-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(Mutex::new(std::collections::HashMap::new()), path)
    }
}
#[cfg(all(test, unix))]
impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.1);
    }
}
#[cfg(all(test, unix))]
struct TestLease(std::fs::File);
#[cfg(all(test, unix))]
impl LifetimeLease for TestLease {
    fn try_last(&mut self) -> Result<bool, String> {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        if unsafe { flock(self.0.as_raw_fd(), 8) } != 0 {
            return Err("unlock".into());
        }
        if unsafe { flock(self.0.as_raw_fd(), 2 | 4) } == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Ok(false)
        } else {
            Err("lock".into())
        }
    }
}
#[cfg(all(test, unix))]
impl CredentialStore for TestStore {
    fn load(&self, owner: &str) -> Result<CredentialState, String> {
        let map = self.0.lock().unwrap();
        Ok(match map.get(owner) {
            None => CredentialState::Missing,
            Some((_, true)) => CredentialState::Pending,
            Some((saved, false)) => CredentialState::Ready(saved.clone()),
        })
    }
    fn reserve(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        let mut map = self.0.lock().unwrap();
        if map.contains_key(owner) {
            return Err("contended".into());
        }
        map.insert(
            owner.into(),
            (
                StoredCredential {
                    nonce: nonce.into(),
                    spawn_token: token.into(),
                    served_token: None,
                },
                true,
            ),
        );
        Ok(())
    }
    fn claim(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        let mut map = self.0.lock().unwrap();
        let (saved, active) = map.get_mut(owner).ok_or("missing")?;
        if saved.nonce != nonce || saved.spawn_token != token || *active {
            return Err("stale".into());
        }
        *active = true;
        Ok(())
    }
    fn finish(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        let mut map = self.0.lock().unwrap();
        let (saved, active) = map.get_mut(owner).ok_or("missing")?;
        if saved.nonce != nonce || saved.spawn_token != token {
            return Err("stale".into());
        }
        *active = false;
        Ok(())
    }
    fn activate(
        &self,
        owner: &str,
        nonce: &str,
        token: &str,
    ) -> Result<Box<dyn LifetimeLease>, String> {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        let mut map = self.0.lock().unwrap();
        let (saved, active) = map.get_mut(owner).ok_or("missing")?;
        if saved.nonce != nonce || saved.spawn_token != token || !*active {
            return Err("stale".into());
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(self.1.join(owner))
            .map_err(|_| "open")?;
        if unsafe { flock(file.as_raw_fd(), 1 | 4) } != 0 {
            return Err("busy".into());
        }
        *active = false;
        Ok(Box::new(TestLease(file)))
    }
    fn reconcile(
        &self,
        owner: &str,
        nonce: &str,
        expected: &str,
        served: &str,
    ) -> Result<(), String> {
        let mut map = self.0.lock().unwrap();
        let (saved, _) = map.get_mut(owner).ok_or("missing")?;
        if saved.nonce != nonce || saved.spawn_token != expected {
            return Err("stale".into());
        }
        saved.served_token = Some(served.into());
        Ok(())
    }
    fn abort(&self, owner: &str, nonce: &str, expected: &str) -> Result<(), String> {
        let mut map = self.0.lock().unwrap();
        if map
            .get(owner)
            .is_some_and(|(saved, _)| saved.nonce == nonce && saved.spawn_token == expected)
        {
            map.remove(owner);
            Ok(())
        } else {
            Err("stale".into())
        }
    }
}
#[cfg(all(test, unix))]
pub(super) fn test_store() -> Arc<dyn CredentialStore> {
    Arc::new(TestStore::default())
}
