use super::*;
#[derive(Clone)]
pub struct AmbientTarget {
    pub token: String,
    pub lifecycle: String,
    pub source: String,
    pub principal: String,
    pub profile: String,
    pub request: String,
    pub key: String,
    pub runtime: String,
    pub lease: Option<crate::connections::LeaseGuard>,
}
impl PluginHosts {
    pub fn ambient_target(&mut self, action: &HostAction) -> Result<AmbientTarget, String> {
        if self.ambient.len() >= 32 {
            return Err("Too many ambient claims are pending".into());
        }
        let running = self.running.as_mut().ok_or("The plugin window closed")?;
        if !super::notifications::same_request_owner(
            &action.payload["native_owner"],
            &running.token,
            &running.key,
            &running.request_source,
            &running.request_profile,
        ) {
            return Err("The plugin claim owner changed".into());
        }
        let runtime = running.owner["runtime_session"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("The plugin has no active owned runtime")?
            .to_owned();
        let key = action.payload["key"]
            .as_str()
            .filter(|s| s.len() <= 1024 && !s.chars().any(char::is_control))
            .ok_or("Invalid ambient cue key")?;
        let key = if key == format!("sound:{runtime}") {
            key.into()
        } else if key.starts_with("speak:") {
            format!("speak:{runtime}:{}", &key[6..])
        } else {
            return Err("The cue does not belong to the active runtime".into());
        };
        let binding = running
            .sources
            .bindings()
            .into_iter()
            .find(|b| {
                b.id == running.request_source
                    && b.profiles.contains(&running.request_profile)
                    && b.lease.as_ref().is_none_or(|l| l.is_valid())
            })
            .ok_or("The active plugin source retired")?;
        Ok(AmbientTarget {
            token: running.token.clone(),
            lifecycle: running.key.clone(),
            source: binding.id,
            principal: crate::client::connection_key(&binding.connection)?,
            profile: running.request_profile.clone(),
            request: action.id.clone(),
            key,
            runtime,
            lease: binding.lease.as_ref().map(|l| l.guard()),
        })
    }
    pub fn queue_ambient(&mut self, id: u64, target: AmbientTarget) {
        self.ambient.insert(id, target);
    }
    pub fn ambient_reply(&mut self, id: u64, owned: bool) {
        let Some(target) = self.ambient.remove(&id) else {
            return;
        };
        let valid = self.running.as_mut().is_some_and(|r| {
            r.token == target.token
                && r.key == target.lifecycle
                && r.request_source == target.source
                && r.request_profile == target.profile
                && r.owner["runtime_session"].as_str() == Some(target.runtime.as_str())
                && target.lease.as_ref().is_none_or(|l| l.is_valid())
                && r.sources.bindings().into_iter().any(|b| {
                    b.id == target.source
                        && b.profiles.contains(&target.profile)
                        && crate::client::connection_key(&b.connection).as_deref()
                            == Ok(target.principal.as_str())
                        && b.lease.as_ref().is_none_or(|l| l.is_valid())
                })
        });
        if valid {
            self.reply_owned(&target.token, &target.request, Ok(Value::Bool(owned)));
        }
    }
}
