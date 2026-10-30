use super::*;
use crate::{
    sidebar::Action,
    source::{SessionOwner, SessionRef},
};

impl Viewer {
    pub(super) fn sync_fleet_partitions(&mut self) {
        let Some(waker) = self.fleet_state.waker.clone() else {
            return;
        };
        let sources = self
            .fleet_snapshot()
            .sources
            .iter()
            .map(|source| {
                (
                    source.descriptor.version.clone(),
                    source.descriptor.label.clone(),
                    source
                        .profiles
                        .iter()
                        .map(|profile| profile.gateway_profile())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let bindings = sources
            .iter()
            .filter_map(|(version, _, _)| {
                self.fleet
                    .binding(version)
                    .map(|binding| (version.clone(), binding))
            })
            .collect::<BTreeMap<_, _>>();
        self.fleet_state.partitions.retain(|_, partition| {
            let version = partition.stamp().version();
            // A replacement forward has the same saved version but a new runtime stamp.
            // Its new controller loads the durable original intent; workers retain their claim.
            if bindings.get(version).is_some_and(|binding| partition.is_retired() || binding.stamp() != partition.stamp()) {
                return false;
            }
            let present = sources.iter().any(|(candidate, _, _)| candidate == version);
            if !present || partition.binding().validate().is_err() {
                partition.retire("This source was removed or changed. Its unresolved edit is retained for reconciliation on the original source version.".into());
                return partition.blocks_owner_change();
            }
            true
        });
        for (version, label, profiles) in sources {
            let Some(binding) = bindings.get(&version).cloned() else {
                continue;
            };
            let key = binding.stamp().key();
            if let Some(partition) = self.fleet_state.partitions.get_mut(&key) {
                partition.set_label(label);
                partition.set_preferences(&self.sidebar_state.prefs);
                partition.set_profiles(&profiles);
                continue;
            }
            let remembered = match &version.id {
                SourceId::Saved(id) => self.connections.remembered_profile(id),
                _ => None,
            };
            let profile = remembered
                .filter(|name| profiles.iter().any(|p| p.name == *name))
                .or_else(|| {
                    profiles
                        .iter()
                        .find(|p| p.name == "default")
                        .map(|p| p.name.as_str())
                })
                .or_else(|| profiles.first().map(|p| p.name.as_str()))
                .unwrap_or("default")
                .to_owned();
            match crate::sidebar::fleet::SourceSidebar::new(
                binding,
                windows::source_epoch(),
                profile.clone(),
                waker.clone(),
            ) {
                Ok(mut partition) => {
                    partition.set_label(label);
                    partition.set_preferences(&self.sidebar_state.prefs);
                    partition.set_profiles(&profiles);
                    partition.load(&profile);
                    self.fleet_state.partitions.insert(key, partition);
                }
                Err(error) => self.client_error = Some(error),
            }
        }
    }
    pub(crate) fn mirror_sidebar_preferences(&mut self) {
        for partition in self.fleet_state.partitions.values_mut() {
            partition.set_preferences(&self.sidebar_state.prefs);
        }
    }
    pub fn fleet_sidebar_groups(
        &self,
    ) -> Vec<(SessionOwner, String, &crate::sidebar::SidebarState)> {
        let active = self
            .connection
            .as_ref()
            .and_then(|c| SourceStamp::from_connection(c).ok());
        let mut groups = self
            .fleet_state
            .partitions
            .values()
            .filter_map(|partition| {
                if active.as_ref() == Some(partition.stamp()) {
                    return None;
                }
                let source = self
                    .fleet_snapshot()
                    .sources
                    .iter()
                    .find(|s| &s.descriptor.version == partition.stamp().version());
                if source.is_none() && !partition.is_retired() {
                    return None;
                }
                let mut profiles = partition
                    .state()
                    .all_rows()
                    .iter()
                    .map(|row| row.profile().to_owned())
                    .collect::<std::collections::BTreeSet<_>>();
                profiles.insert(partition.state().owner_profile.clone());
                if partition.state().prefs.all_profiles {
                    profiles.extend(source.into_iter().flat_map(|source| {
                        source.profiles.iter().map(|profile| profile.name.clone())
                    }));
                }
                let visible = profiles
                    .iter()
                    .any(|profile| self.sidebar_profile_visible(partition.stamp(), profile));
                let state = partition.state();
                let diagnostic = partition.is_retired()
                    || partition.blocks_owner_change()
                    || state.menu.is_some()
                    || state.rename.is_some()
                    || state.confirm.is_some()
                    || !state.errors.is_empty()
                    || !state.storage.is_empty()
                    || state.recents.error.is_some()
                    || state.messaging.error.is_some()
                    || state.cron_sessions.error.is_some()
                    || state.jobs_error.is_some()
                    || source.is_none_or(|source| {
                        !matches!(source.status, crate::fleet::Status::Ready)
                            || source.inventory_error.is_some()
                    });
                if !visible && !diagnostic {
                    return None;
                }
                let key = crate::view::fleet::source_key(partition.stamp().version());
                let label = self
                    .fleet_preferences
                    .group_alias(&key)
                    .unwrap_or(partition.label())
                    .to_owned();
                Some((
                    partition.owner(&partition.state().owner_profile),
                    label,
                    partition.state(),
                ))
            })
            .collect::<Vec<_>>();
        groups.sort_by(|(a, al, _), (b, bl, _)| {
            source_group_compare(
                (a.source.version(), al),
                (b.source.version(), bl),
                self.fleet_preferences.source_order(),
            )
        });
        groups
    }
    pub fn sidebar_profile_visible(&self, source: &SourceStamp, profile: &str) -> bool {
        let active = self.fleet_active_version();
        let mut profiles = self
            .sidebar_state
            .all_rows()
            .iter()
            .map(|row| row.profile().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        profiles.insert(self.sidebar_state.owner_profile.clone());
        if self.sidebar_state.prefs.all_profiles {
            profiles.extend(self.profiles.iter().map(|profile| profile.name.clone()));
        }
        profile_projection_visible(
            self.fleet_snapshot(),
            source.version(),
            profile,
            active.as_ref(),
            &profiles.into_iter().collect::<Vec<_>>(),
        )
    }
    pub fn sidebar_action_owned(&mut self, owner: SessionOwner, action: Action) {
        let current = self
            .connection
            .as_ref()
            .and_then(|c| SourceStamp::from_connection(c).ok());
        if preference_action(&action) {
            if matches!(action, Action::CollapseAll) && current.as_ref() != Some(&owner.source) {
                let Some(partition) = self.fleet_state.partitions.get_mut(&owner.source.key())
                else {
                    return;
                };
                partition.state_mut().action(Action::CollapseAll);
                let changed = self
                    .sidebar_state
                    .prefs
                    .collapsed
                    .symmetric_difference(&partition.state().prefs.collapsed)
                    .cloned()
                    .collect::<Vec<_>>();
                for key in changed {
                    self.sidebar_action(Action::Collapse(key));
                }
            } else {
                self.sidebar_action(action);
            }
            self.mirror_sidebar_preferences();
            return;
        }
        if current.as_ref() == Some(&owner.source) {
            if let Err(error) = self.current_source_binding() {
                self.client_error = Some(error);
                return;
            }
            self.sidebar_action(action);
            return;
        }
        let key = owner.source.key();
        let Some(partition) = self.fleet_state.partitions.get(&key) else {
            self.client_error = Some("This sidebar source is no longer available".into());
            return;
        };
        if partition.is_retired() || partition.binding().validate().is_err() {
            self.client_error =
                Some("This sidebar source changed; refresh it before using its records".into());
            return;
        }
        match action {
            Action::Open(profile, id) => {
                let Some(row) = partition.state().find_owned(
                    &SessionOwner::new(owner.source.clone(), profile.clone()),
                    &id,
                ) else {
                    return;
                };
                let read_only = (row.source.as_deref() == Some("cron")).then(|| {
                    crate::sidebar::cron_read_only(
                        row.ended_at,
                        row.scheduler_owned,
                        row.is_active,
                        Some(row.last_active),
                        partition.state().now,
                    )
                });
                self.begin_fleet_navigation(
                    owner.source.version().clone(),
                    profile,
                    Destination::Session {
                        id: row.id.clone(),
                        title: row.title().into(),
                        read_only,
                    },
                );
            }
            Action::OpenRun(index) => {
                let Some(run) = partition
                    .state()
                    .runs
                    .get(index)
                    .filter(|run| run.source.as_deref() == Some("cron"))
                else {
                    return;
                };
                let profile = run
                    .profile
                    .clone()
                    .unwrap_or_else(|| partition.state().owner_profile.clone());
                let destination = Destination::Session {
                    id: run.session_id.clone(),
                    title: run.title.clone().unwrap_or_else(|| "Scheduled run".into()),
                    read_only: Some(crate::sidebar::cron_read_only(
                        run.ended_at,
                        run.scheduler_owned,
                        run.is_active,
                        run.last_active,
                        partition.state().now,
                    )),
                };
                self.begin_fleet_navigation(owner.source.version().clone(), profile, destination);
            }
            Action::ManageJob(id) => {
                let Some(job) = partition.state().jobs.iter().find(|job| job.id == id) else {
                    return;
                };
                let profile = job
                    .profile
                    .clone()
                    .unwrap_or_else(|| partition.state().owner_profile.clone());
                self.begin_fleet_navigation(
                    owner.source.version().clone(),
                    profile,
                    Destination::Cron(id),
                );
            }
            action => {
                if let Some(partition) = self.fleet_state.partitions.get_mut(&key) {
                    partition.action(action);
                }
            }
        }
    }
    pub fn sidebar_title_owned(&mut self, owner: &SessionOwner) -> &mut TextEdit {
        let current = self
            .connection
            .as_ref()
            .and_then(|c| SourceStamp::from_connection(c).ok());
        if current.as_ref() == Some(&owner.source) {
            return &mut self.sidebar_state.title;
        }
        if let Some(partition) = self
            .fleet_state
            .partitions
            .get_mut(&owner.source.key())
            .filter(|partition| !partition.is_retired() && partition.binding().validate().is_ok())
        {
            return &mut partition.state_mut().title;
        }
        &mut self.retired_editor_input
    }
    pub fn sidebar_open_window_owned(&mut self, target: SessionRef, title: String) {
        let current = self
            .connection
            .as_ref()
            .and_then(|c| SourceStamp::from_connection(c).ok());
        if current.as_ref() == Some(&target.owner.source) {
            if let Err(error) = self.current_source_binding() {
                self.client_error = Some(error);
                return;
            }
            self.request_window_target(Some(ConversationTarget::Session {
                profile: target.owner.profile,
                id: target.stored_id,
                title,
            }));
        } else if let Err(error) = self.open_bound_session_window(target, title) {
            self.client_error = Some(error);
        }
    }
}
fn preference_action(action: &Action) -> bool {
    matches!(
        action,
        Action::Collapse(_)
            | Action::Grouping(_)
            | Action::Ordering(_)
            | Action::Toggle(_)
            | Action::Status(_)
            | Action::Profile(_)
            | Action::Project(_)
            | Action::ResetView
            | Action::CollapseAll
    )
}

fn source_group_compare(
    a: (&SourceVersion, &str),
    b: (&SourceVersion, &str),
    explicit: &[String],
) -> std::cmp::Ordering {
    let ai = crate::view::fleet::source_key(a.0);
    let bi = crate::view::fleet::source_key(b.0);
    let rank = |id: &String| {
        let position = explicit.iter().position(|key| key == id);
        (position.is_none(), position.unwrap_or(usize::MAX))
    };
    rank(&ai)
        .cmp(&rank(&bi))
        .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        .then_with(|| ai.cmp(&bi))
}
fn profile_projection_visible(
    snapshot: &crate::fleet::Snapshot,
    source: &SourceVersion,
    profile: &str,
    actual: Option<&SourceVersion>,
    represented: &[String],
) -> bool {
    let ready = |version: &SourceVersion| {
        snapshot
            .sources
            .iter()
            .find(|source| &source.descriptor.version == version)
            .is_some_and(|source| {
                matches!(source.status, crate::fleet::Status::Ready)
                    && source.inventory_error.is_none()
            })
    };
    if !ready(source) {
        return true;
    }
    let Some(canonical) = snapshot.canonical_source(source, profile) else {
        return true;
    };
    if !ready(&canonical) {
        return true;
    }
    if let Some(actual) =
        actual.filter(|actual| ready(actual) && represented.iter().any(|name| name == profile))
    {
        if snapshot.canonical_source(actual, profile).as_ref() == Some(&canonical) {
            return source == actual;
        }
    }
    source == &canonical
}
#[cfg(test)]
mod order_tests {
    use super::*;
    fn version(id: &str) -> SourceVersion {
        SourceVersion {
            id: SourceId::Saved(id.into()),
            revision: "one".into(),
        }
    }
    #[test]
    fn source_groups_use_explicit_then_label_and_id_without_rail_local_or_numeric_priority() {
        let entries = [
            (version("z"), "Zulu", false),
            (version("b"), "Alpha", false),
            (version("a"), "Alpha", false),
            (version("local"), "Zulu local", true),
        ];
        let ordered = |entries: Vec<(SourceVersion, &str, bool)>, preference: &[String]| {
            let mut entries = entries;
            entries.sort_by(|(av, al, _), (bv, bl, _)| {
                source_group_compare((av, al), (bv, bl), preference)
            });
            entries
                .into_iter()
                .map(|(version, _, _)| version.id)
                .collect::<Vec<_>>()
        };
        let expected = [
            SourceId::Saved("a".into()),
            SourceId::Saved("b".into()),
            SourceId::Saved("z".into()),
            SourceId::Saved("local".into()),
        ];
        assert_eq!(ordered(entries.to_vec(), &[]), expected);
        assert_eq!(
            ordered(entries.iter().rev().cloned().collect(), &[]),
            expected
        );
        for selected in ["a", "z"] {
            assert_eq!(
                ordered(
                    entries
                        .iter()
                        .filter(|(v, _, _)| v.id != SourceId::Saved(selected.into()))
                        .cloned()
                        .collect(),
                    &[]
                ),
                expected
                    .iter()
                    .filter(|id| **id != SourceId::Saved(selected.into()))
                    .cloned()
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(
            ordered(
                vec![
                    (version("two"), "Remote2", false),
                    (version("ten"), "Remote10", true)
                ],
                &[]
            ),
            [SourceId::Saved("ten".into()), SourceId::Saved("two".into())]
        );
        let preferred = crate::view::fleet::source_key(&version("z"));
        assert_eq!(
            ordered(entries.to_vec(), &[preferred])[0],
            SourceId::Saved("z".into())
        );
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use crate::fleet::{
        Agent, Descriptor, Evidence, ProfileMetadata, RosterProfile, Snapshot, Source, Status,
    };
    fn version(id: SourceId) -> SourceVersion {
        SourceVersion {
            id,
            revision: "fixture".into(),
        }
    }
    fn source(version: &SourceVersion, names: &[&str], install: &str) -> Source {
        Source {
            descriptor: Descriptor {
                version: version.clone(),
                label: "Fixture".into(),
                kind: crate::connections::Kind::Remote,
                registration_order: 0,
            },
            status: Status::Ready,
            evidence: Some(Evidence::Gateway),
            install_id: Some(install.into()),
            principal_stamp: None,
            profiles: names
                .iter()
                .map(|name| RosterProfile {
                    name: (*name).into(),
                    metadata: ProfileMetadata::default(),
                    canonical_session: None,
                })
                .collect(),
            inventory_error: None,
        }
    }
    fn agent(source: &SourceVersion, profile: &str) -> Agent {
        Agent {
            source: source.clone(),
            profile: profile.into(),
            target_profile: profile.into(),
            handle: profile.into(),
            metadata: ProfileMetadata::default(),
        }
    }
    #[test]
    fn sidebar_alias_projection_keeps_actual_current_rows_unique_profiles_and_unproven_error_doors()
    {
        let saved = version(SourceId::Saved("registered-A".into()));
        let raw = version(SourceId::Direct("current-A".into()));
        let other = version(SourceId::Saved("registered-B".into()));
        let snapshot = Snapshot {
            sources: vec![
                source(&saved, &["research", "saved-only"], "same-A"),
                source(&raw, &["research", "current-only"], "same-A"),
                source(&other, &["research"], "distinct-B"),
            ],
            agents: vec![
                agent(&saved, "research"),
                agent(&saved, "saved-only"),
                agent(&raw, "current-only"),
                agent(&other, "research"),
            ],
            ..Default::default()
        };
        let represented = vec!["research".into()];
        assert!(profile_projection_visible(
            &snapshot,
            &raw,
            "research",
            Some(&raw),
            &represented
        ));
        assert!(!profile_projection_visible(
            &snapshot,
            &saved,
            "research",
            Some(&raw),
            &represented
        ));
        assert!(profile_projection_visible(
            &snapshot,
            &saved,
            "saved-only",
            Some(&raw),
            &represented
        ));
        assert!(profile_projection_visible(
            &snapshot,
            &raw,
            "current-only",
            Some(&raw),
            &represented
        ));
        assert!(profile_projection_visible(
            &snapshot,
            &other,
            "research",
            Some(&raw),
            &represented
        ));
        assert!(!profile_projection_visible(
            &snapshot,
            &raw,
            "research",
            None,
            &[]
        ));
        assert!(profile_projection_visible(
            &snapshot,
            &saved,
            "research",
            None,
            &[]
        ));
        let mut unknown = snapshot.clone();
        unknown
            .sources
            .iter_mut()
            .for_each(|source| source.install_id = None);
        assert!(profile_projection_visible(
            &unknown,
            &raw,
            "research",
            Some(&raw),
            &represented
        ));
        assert!(profile_projection_visible(
            &unknown,
            &saved,
            "research",
            Some(&raw),
            &represented
        ));
        let mut offline = snapshot.clone();
        offline.sources[0].status = Status::Unavailable("Private fixture offline".into());
        assert!(profile_projection_visible(
            &offline,
            &saved,
            "research",
            Some(&raw),
            &represented
        ));
        assert!(profile_projection_visible(
            &offline,
            &raw,
            "research",
            None,
            &[]
        ));
        let mut partial = snapshot.clone();
        partial.sources[1].inventory_error = Some("A profile could not be read".into());
        assert!(profile_projection_visible(
            &partial,
            &raw,
            "research",
            None,
            &[]
        ));
        assert!(profile_projection_visible(
            &partial,
            &saved,
            "research",
            Some(&raw),
            &represented
        ));
    }
}
