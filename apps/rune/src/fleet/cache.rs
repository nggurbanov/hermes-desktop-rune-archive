use super::*;
use crate::gateway::roster::Identity;
use std::sync::atomic::AtomicBool;

pub(super) struct Entry {
    pub source: Source,
    pub binding: Option<Binding>,
    pub(super) resolver: Option<Resolver>,
    identity_at: Option<Instant>,
    ssh_attempted: Option<Instant>,
    ssh_success: bool,
}
impl Entry {
    fn new(descriptor: Descriptor) -> Self {
        Self {
            source: Source {
                descriptor,
                status: Status::ConnectOnDemand,
                evidence: None,
                install_id: None,
                principal_stamp: None,
                profiles: vec![],
                inventory_error: None,
            },
            binding: None,
            resolver: None,
            identity_at: None,
            ssh_attempted: None,
            ssh_success: false,
        }
    }
}
struct RetainedFact {
    stamp: SourceStamp,
    binding: Option<Binding>,
    resolver: Option<Resolver>,
}
#[derive(Default)]
pub(super) struct State {
    pub rows: BTreeMap<SourceVersion, Entry>,
    pub resolver: Option<Resolver>,
    pub snapshot: Snapshot,
    primary: Option<SourceId>,
    registry_known: bool,
    currents: BTreeMap<u64, SourceVersion>,
    current_facts: BTreeMap<u64, RetainedFact>,
    retained: BTreeMap<u64, RetainedFact>,
    generation: u64,
    pending: BTreeMap<SourceVersion, Arc<AtomicBool>>,
    forced_followup: bool,
    fetched_at: Option<Instant>,
}
impl State {
    pub fn binding(&self, version: &SourceVersion) -> Option<Binding> {
        if self.registry_known
            && matches!(version.id, SourceId::Saved(_))
            && !self.rows.contains_key(version)
        {
            return None;
        }
        if let Some(binding) = self
            .rows
            .get(version)
            .and_then(|row| row.binding.clone())
            .filter(|binding| binding.validate().is_ok())
        {
            return Some(binding);
        }
        let mut found: Option<Binding> = None;
        for fact in self
            .current_facts
            .values()
            .chain(self.retained.values())
            .filter(|fact| fact.stamp.version() == version)
        {
            let Some(binding) = fact
                .binding
                .as_ref()
                .filter(|binding| binding.validate().is_ok())
            else {
                continue;
            };
            if found
                .as_ref()
                .is_some_and(|previous| previous.stamp() != binding.stamp())
            {
                return None;
            }
            found = Some(binding.clone());
        }
        found
    }
    pub fn resolver(&self, version: &SourceVersion) -> Option<Resolver> {
        let binding = self.binding(version)?;
        if let Some(resolver) = binding.resolver() {
            return Some(resolver.clone());
        }
        if let Some(entry) = self.rows.get(version) {
            if entry
                .binding
                .as_ref()
                .is_some_and(|current| current.stamp() == binding.stamp())
                && entry.resolver.is_some()
            {
                return entry.resolver.clone();
            }
        }
        self.current_facts
            .values()
            .chain(self.retained.values())
            .find(|fact| {
                &fact.stamp == binding.stamp()
                    && fact
                        .binding
                        .as_ref()
                        .is_some_and(|binding| binding.validate().is_ok())
            })
            .and_then(|fact| fact.resolver.clone())
    }
    pub fn retain_source(&mut self, token: u64, binding: Binding, resolver: Option<Resolver>) {
        let resolver = binding
            .resolver()
            .cloned()
            .or_else(|| {
                self.rows
                    .get(binding.stamp().version())
                    .filter(|entry| {
                        entry
                            .binding
                            .as_ref()
                            .is_some_and(|current| current.stamp() == binding.stamp())
                    })
                    .and_then(|entry| entry.resolver.clone())
            })
            .or(resolver);
        let binding = if let Some(origin) = &resolver {
            binding.retaining_resolver(origin.clone())
        } else {
            binding
        };
        self.retained.insert(
            token,
            RetainedFact {
                stamp: binding.stamp().clone(),
                binding: Some(binding),
                resolver,
            },
        );
    }
    pub fn retained_binding(&self, token: u64, stamp: &SourceStamp) -> Result<Binding, String> {
        let fact = self
            .retained
            .get(&token)
            .filter(|fact| &fact.stamp == stamp)
            .ok_or("The retained source owner was released")?;
        let binding = fact
            .binding
            .clone()
            .ok_or("The retained source was shut down with its last window")?;
        binding.validate()?;
        if self.registry_known
            && matches!(stamp.version().id, SourceId::Saved(_))
            && !self.rows.contains_key(stamp.version())
        {
            return Err("The retained source route changed or was removed".into());
        }
        Ok(binding)
    }
    pub fn adopt_source(
        &mut self,
        owner: u64,
        token: u64,
        stamp: &SourceStamp,
    ) -> Result<(), String> {
        let binding = self.retained_binding(token, stamp)?;
        let resolver = self
            .retained
            .get(&token)
            .and_then(|fact| fact.resolver.clone());
        if self.currents.get(&owner) != Some(stamp.version()) {
            self.remove_current(owner);
        }
        self.currents.insert(owner, stamp.version().clone());
        self.current_facts.insert(
            owner,
            RetainedFact {
                stamp: stamp.clone(),
                binding: Some(binding.clone()),
                resolver: resolver.clone(),
            },
        );
        if let Some(entry) = self.rows.get_mut(stamp.version()) {
            entry.binding = Some(binding);
            entry.resolver = resolver;
            entry.source.principal_stamp = Some(stamp.clone());
        }
        self.rebuild();
        Ok(())
    }
    pub fn release_retained(&mut self, token: u64) {
        let Some(fact) = self.retained.remove(&token) else {
            return;
        };
        let version = fact.stamp.version();
        if !matches!(version.id, SourceId::Saved(_))
            && !self.currents.values().any(|current| current == version)
            && !self
                .retained
                .values()
                .any(|other| other.stamp.version() == version)
        {
            self.rows.remove(version);
            if let Some(cancelled) = self.pending.remove(version) {
                cancelled.store(true, Ordering::Release);
            }
            if self.pending.is_empty() {
                self.snapshot.refreshing = false;
            }
            self.rebuild();
        }
    }
    pub fn shutdown(&mut self) {
        for cancelled in self.pending.values() {
            cancelled.store(true, Ordering::Release);
        }
        self.pending.clear();
        for entry in self.rows.values_mut() {
            entry.binding = None;
            entry.resolver = None;
        }
        for fact in self
            .current_facts
            .values_mut()
            .chain(self.retained.values_mut())
        {
            fact.binding = None;
            fact.resolver = None;
        }
        self.forced_followup = false;
        self.snapshot.refreshing = false;
        self.generation = self.generation.wrapping_add(1);
        self.resolver = None;
        self.fetched_at = None;
        self.rebuild();
    }
    pub fn update_registry(
        &mut self,
        descriptors: Vec<Descriptor>,
        primary: Option<SourceId>,
    ) -> bool {
        let descriptors = descriptors
            .into_iter()
            .map(|d| (d.version.clone(), d))
            .collect::<BTreeMap<_, _>>();
        let before = self
            .rows
            .values()
            .map(|entry| entry.source.descriptor.clone())
            .collect::<Vec<_>>();
        self.rows.retain(|version, _| {
            descriptors.contains_key(version)
                || self.currents.values().any(|current| current == version)
                    && !matches!(version.id, SourceId::Saved(_))
                || self
                    .retained
                    .values()
                    .any(|fact| fact.stamp.version() == version)
                    && !matches!(version.id, SourceId::Saved(_))
        });
        self.pending.retain(|version, cancelled| {
            if descriptors.contains_key(version) || self.rows.contains_key(version) {
                true
            } else {
                cancelled.store(true, Ordering::Release);
                false
            }
        });
        for (version, descriptor) in descriptors {
            self.rows
                .entry(version)
                .and_modify(|entry| entry.source.descriptor = descriptor.clone())
                .or_insert_with(|| Entry::new(descriptor));
        }
        let changed = !self.registry_known
            || self.snapshot.error.is_some()
            || self.primary != primary
            || before
                != self
                    .rows
                    .values()
                    .map(|entry| entry.source.descriptor.clone())
                    .collect::<Vec<_>>();
        self.registry_known = true;
        self.primary = primary;
        self.snapshot.error = None;
        if self.pending.is_empty() {
            self.snapshot.refreshing = false;
        }
        if changed {
            self.fetched_at = None;
            self.rebuild();
        }
        changed
    }
    pub fn unavailable(&mut self, error: String) {
        self.registry_known = false;
        for cancelled in self.pending.values() {
            cancelled.store(true, Ordering::Release);
        }
        self.pending.clear();
        self.snapshot.refreshing = false;
        self.forced_followup = false;
        self.snapshot.error = Some(error);
        self.rebuild();
    }
    pub fn retire_invalid(&mut self) {
        let mut changed = false;
        for entry in self.rows.values_mut() {
            if let Some(error) = entry
                .binding
                .as_ref()
                .and_then(|binding| binding.validate().err())
            {
                entry.binding = None;
                entry.resolver = None;
                entry.source.status = Status::Unavailable(error);
                if entry.source.descriptor.kind == Kind::Local {
                    entry.source.profiles.clear();
                    entry.source.evidence = None;
                }
                changed = true;
            }
        }
        if changed {
            self.rebuild();
        }
    }
    pub fn remove_current(&mut self, owner: u64) {
        self.current_facts.remove(&owner);
        let previous = self.currents.remove(&owner);
        if let Some(previous) = previous {
            if !matches!(previous.id, SourceId::Saved(_))
                && !self.currents.values().any(|v| v == &previous)
                && !self
                    .retained
                    .values()
                    .any(|fact| fact.stamp.version() == &previous)
            {
                self.rows.remove(&previous);
                if let Some(cancelled) = self.pending.remove(&previous) {
                    cancelled.store(true, Ordering::Release);
                }
                if self.pending.is_empty() {
                    self.snapshot.refreshing = false;
                }
                self.rebuild();
            }
        }
    }
    pub fn seed_current(
        &mut self,
        owner: u64,
        binding: Binding,
        profiles: &[Profile],
        resolver: Option<Resolver>,
    ) {
        let resolver = binding
            .resolver()
            .cloned()
            .or_else(|| {
                self.rows
                    .get(binding.stamp().version())
                    .filter(|entry| {
                        entry
                            .binding
                            .as_ref()
                            .is_some_and(|current| current.stamp() == binding.stamp())
                    })
                    .and_then(|entry| entry.resolver.clone())
            })
            .or(resolver);
        let binding = if let Some(origin) = &resolver {
            binding.retaining_resolver(origin.clone())
        } else {
            binding
        };
        let version = binding.stamp().version().clone();
        if self.currents.get(&owner) != Some(&version) {
            self.remove_current(owner);
        }
        self.currents.insert(owner, version.clone());
        let descriptor = Descriptor {
            version: version.clone(),
            label: if matches!(
                binding.connection().transport(),
                crate::gateway::Connection::Local { .. }
            ) {
                "This device"
            } else {
                "Connected agent"
            }
            .into(),
            kind: if matches!(
                binding.connection().transport(),
                crate::gateway::Connection::Local { .. }
            ) {
                Kind::Local
            } else {
                Kind::Remote
            },
            registration_order: usize::MAX,
        };
        let entry = self
            .rows
            .entry(version)
            .or_insert_with(|| Entry::new(descriptor));
        let old = entry
            .source
            .profiles
            .iter()
            .map(|p| (p.name.clone(), p.metadata.clone()))
            .collect::<BTreeMap<_, _>>();
        entry.source.profiles = profiles
            .iter()
            .filter(|p| crate::gateway::roster::valid_profile(&p.name))
            .map(|profile| {
                let mut metadata = old.get(&profile.name).cloned().unwrap_or_default();
                metadata.resolved_label = Some(profile.label.clone());
                metadata.description = Some(profile.description.clone());
                RosterProfile {
                    name: profile.name.clone(),
                    metadata,
                    canonical_session: profile.canonical_session.clone(),
                }
            })
            .collect();
        let preserve_origin = entry.resolver.is_some()
            && entry
                .binding
                .as_ref()
                .is_some_and(|old| old.stamp() == binding.stamp() && old.validate().is_ok());
        entry.binding = Some(binding.clone());
        if !preserve_origin {
            entry.resolver = resolver;
        }
        entry.source.principal_stamp = Some(binding.stamp().clone());
        entry.source.status = Status::Ready;
        entry.source.evidence = Some(Evidence::CurrentSeed);
        entry.source.inventory_error = None;
        self.current_facts.insert(
            owner,
            RetainedFact {
                stamp: binding.stamp().clone(),
                binding: Some(binding),
                resolver: entry.resolver.clone(),
            },
        );
        self.rebuild();
    }
    pub fn refresh(&mut self, reason: Reason, now: Instant) -> Vec<worker::Job> {
        if !self.registry_known {
            self.unavailable("The saved connection registry has not been loaded".into());
            return vec![];
        }
        let ttl = if self
            .rows
            .values()
            .any(|entry| matches!(entry.source.status, Status::Unavailable(_)))
        {
            Duration::from_secs(5)
        } else {
            Duration::from_secs(60)
        };
        if !reason.forced()
            && self
                .fetched_at
                .is_some_and(|at| now.saturating_duration_since(at) < ttl)
        {
            return vec![];
        }
        if self.snapshot.refreshing {
            if reason.forced() {
                self.forced_followup = true;
            }
            return vec![];
        }
        self.forced_followup = false;
        self.generation = self.generation.wrapping_add(1);
        let mut jobs = Vec::new();
        for (version, entry) in &self.rows {
            // Direct routes are observations of an actual current connection only.
            if !matches!(version.id, SourceId::Saved(_))
                && !self.currents.values().any(|v| v == version)
                && !self
                    .retained
                    .values()
                    .any(|fact| fact.stamp.version() == version)
            {
                continue;
            }
            let cancelled = Arc::new(AtomicBool::new(false));
            self.pending.insert(version.clone(), cancelled.clone());
            let identity_ttl = Duration::from_secs(if entry.source.install_id.is_some() {
                300
            } else {
                60
            });
            jobs.push(worker::Job {
                generation: self.generation,
                descriptor: entry.source.descriptor.clone(),
                binding: entry
                    .binding
                    .clone()
                    .filter(|binding| binding.validate().is_ok()),
                resolver: entry.resolver.clone().or_else(|| self.resolver.clone()),
                identity: entry
                    .identity_at
                    .is_none_or(|at| now.saturating_duration_since(at) >= identity_ttl),
                inventory: !entry.ssh_success
                    && entry.ssh_attempted.is_none_or(|at| {
                        now.saturating_duration_since(at) >= Duration::from_secs(60)
                    }),
                cancelled,
            });
        }
        self.snapshot.refreshing = !jobs.is_empty();
        if jobs.is_empty() {
            self.fetched_at = Some(now);
        }
        self.rebuild();
        jobs
    }
    pub fn complete(
        &mut self,
        job: &worker::Job,
        outcome: worker::Outcome,
        now: Instant,
    ) -> Vec<worker::Job> {
        let version = &job.descriptor.version;
        if job.generation != self.generation
            || job.cancelled.load(Ordering::Acquire)
            || !self.pending.contains_key(version)
        {
            return vec![];
        }
        self.pending.remove(version);
        if let Some(entry) = self.rows.get_mut(version) {
            match outcome {
                worker::Outcome::Gateway {
                    binding,
                    stamp,
                    read,
                } => {
                    entry.resolver = binding
                        .as_ref()
                        .and_then(|binding| binding.resolver().cloned())
                        .or_else(|| binding.as_ref().and(job.resolver.clone()));
                    entry.binding = binding;
                    entry.source.principal_stamp = Some(stamp);
                    entry.source.profiles = read.profiles;
                    entry.source.status = Status::Ready;
                    entry.source.evidence = Some(Evidence::Gateway);
                    entry.source.inventory_error = None;
                    match read.identity {
                        Identity::Observed(id) => {
                            entry.source.install_id = id;
                            entry.identity_at = Some(now);
                        }
                        Identity::Unavailable => {
                            if entry.source.install_id.is_none() {
                                entry.identity_at = Some(now);
                            }
                        }
                        Identity::NotRequested => {}
                    }
                }
                worker::Outcome::Inventory(result) => {
                    entry.ssh_attempted = Some(now);
                    entry.source.status = Status::ConnectOnDemand;
                    match result {
                        Ok(inventory) => {
                            entry.source.profiles = inventory
                                .profiles
                                .into_iter()
                                .map(|name| RosterProfile {
                                    name,
                                    metadata: ProfileMetadata::default(),
                                    canonical_session: None,
                                })
                                .collect();
                            entry.source.install_id = inventory.install_id;
                            entry.identity_at = Some(now);
                            entry.source.evidence = Some(Evidence::SshInventory);
                            entry.ssh_success = true;
                            entry.source.inventory_error = None;
                        }
                        Err(error) => entry.source.inventory_error = Some(error),
                    }
                }
                worker::Outcome::Deferred => {
                    entry.source.status = Status::ConnectOnDemand;
                }
                worker::Outcome::Failed(error) => {
                    entry.source.status = Status::Unavailable(error);
                    if entry.source.descriptor.kind == Kind::Local {
                        entry.source.profiles.clear();
                        entry.source.evidence = None;
                    }
                    if entry
                        .binding
                        .as_ref()
                        .is_some_and(|binding| binding.validate().is_err())
                    {
                        entry.binding = None;
                        entry.resolver = None;
                    }
                }
            }
        }
        if self.pending.is_empty() {
            self.snapshot.refreshing = false;
            self.fetched_at = Some(now);
            let forced = std::mem::take(&mut self.forced_followup);
            self.rebuild();
            if forced {
                return self.refresh(Reason::Explicit, now);
            }
        } else {
            self.rebuild();
        }
        vec![]
    }
    fn rebuild(&mut self) {
        let mut sources = self
            .rows
            .values()
            .map(|entry| entry.source.clone())
            .collect::<Vec<_>>();
        sources.sort_by_key(|source| source.descriptor.registration_order);
        let mut identities: BTreeMap<(String, String), Vec<(&Source, &RosterProfile)>> =
            BTreeMap::new();
        for source in &sources {
            for profile in &source.profiles {
                let key = source
                    .install_id
                    .as_ref()
                    .filter(|id| !id.is_empty())
                    .map(|id| format!("install:{id}"))
                    .unwrap_or_else(|| serde_json::to_string(&source.descriptor.version).unwrap());
                let values = identities.entry((key, profile.name.clone())).or_default();
                if !values
                    .iter()
                    .any(|(s, _)| s.descriptor.version == source.descriptor.version)
                {
                    values.push((source, profile));
                }
            }
        }
        let mut selected = identities
            .into_values()
            .map(|mut group| {
                group.sort_by_key(|(source, _)| {
                    (
                        self.primary.as_ref() != Some(&source.descriptor.version.id),
                        match source.descriptor.kind {
                            Kind::Local => 0,
                            Kind::Ssh => 1,
                            Kind::Remote => 2,
                        },
                        source.descriptor.registration_order,
                    )
                });
                group[0]
            })
            .collect::<Vec<_>>();
        selected.sort_by_key(|(source, _)| source.descriptor.registration_order);
        let mut counts = BTreeMap::<&str, usize>::new();
        for (_, profile) in &selected {
            *counts.entry(&profile.name).or_default() += 1;
        }
        self.snapshot.agents = selected
            .into_iter()
            .map(|(source, profile)| Agent {
                source: source.descriptor.version.clone(),
                profile: profile.name.clone(),
                target_profile: profile.name.clone(),
                handle: if counts[profile.name.as_str()] > 1 {
                    format!("{}-{}", profile.name, slug(&source.descriptor.label))
                } else {
                    profile.name.clone()
                },
                metadata: profile.metadata.clone(),
            })
            .collect();
        self.snapshot.sources = sources;
        self.snapshot.revision = self.snapshot.revision.wrapping_add(1);
    }
}
fn slug(label: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for c in label.trim().to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() && slug.len() < 48 {
                slug.push('-');
            }
            if slug.len() < 48 {
                slug.push(c);
            }
            separator = false;
        } else {
            separator = true;
        }
    }
    if slug.is_empty() {
        "connection".into()
    } else {
        slug
    }
}
