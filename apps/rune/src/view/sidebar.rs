//! Native Hermes navigation: pinned conversations, recents, sources and jobs.
use super::{palette::Pal, shell};
use crate::{
    app::Viewer,
    gateway::sidebar::SidebarSession,
    sidebar::{
        Action, Confirmation, Grouping, ListRow, Ordering, Status, date_rows, project,
        relative_age, source_label,
    },
};
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::shell::RowStatus;
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Menu, MenuEntry,
    MenuItem, SidebarRow, TextInput,
};
use rune_theme_opencode::icons as ic;
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub fn sessions(s: &Viewer, p: &Pal, k: &Kit, layer: &mut Vec<El>) -> El {
    let state = &s.sidebar_state;
    let mut body = col().w_full().gap(3.0).px(8.0).py(8.0);
    if let Some(notice) = &state.notice {
        body = body.child(note(notice, k));
    }
    if state.mutation.is_some() {
        body=body.child(note(if state.mutation_inflight{"Applying a revision-fenced session edit…"}else{"A session edit needs reconciliation. Retrying keeps its original request identity."},k)).child(button("Retry pending edit",Action::RetryMutation,state.demo||state.mutation_inflight,k));
    }
    for (profile, kind) in &state.storage {
        body = body.child(note(
            format!("{profile}: session storage is {kind}. Other profiles remain available."),
            k,
        ));
    }
    for (op, error) in &state.errors {
        if op.contains("mutation")
            || op.contains("snapshot")
            || op.contains("runs.")
            || op.contains("job-write.")
        {
            body = body.child(note(error, k).role("alert"));
        }
    }
    if let Some(confirm) = &state.confirm {
        let title = match confirm {
            Confirmation::Session(mutation) => format!("Delete {}?", mutation.session.title()),
            Confirmation::Job(id) => format!("Delete scheduled job {id}?"),
        };
        body = body.child(
            col()
                .gap(6.0)
                .py(8.0)
                .child(text(title).font(k.ty.ui_medium).color(k.c.text_base).wrap())
                .child(note(
                    "This removes the selected record from its owning agent.",
                    k,
                ))
                .child(
                    row()
                        .gap(6.0)
                        .child(button(
                            "Delete",
                            Action::Confirm,
                            state.demo || state.mutation_inflight,
                            k,
                        ))
                        .child(button("Cancel", Action::Cancel, false, k)),
                ),
        );
    }
    if state.rename.is_some() {
        body = body.child(
            col()
                .gap(6.0)
                .py(8.0)
                .child(
                    TextInput::new(&state.title)
                        .width(None)
                        .placeholder("Session title")
                        .on_edit(|s: &mut Viewer| &mut s.sidebar_state.title)
                        .build(k)
                        .key("session-rename-input"),
                )
                .child(
                    row()
                        .gap(6.0)
                        .child(button(
                            "Save title",
                            Action::SaveRename,
                            state.demo || state.mutation_inflight,
                            k,
                        ))
                        .child(button("Cancel", Action::Cancel, false, k)),
                ),
        );
    }
    let all = visible_rows(s);
    if !s.search.text.trim().is_empty() {
        let matches = all
            .iter()
            .filter(|row| state.matches(row, &s.search.text))
            .count();
        body =
            body.child(note(format!("{matches} of {}", all.len()), k).key("session-match-count"));
    }
    let pinned: Vec<_> = all
        .iter()
        .copied()
        .filter(|r| r.pinned == Some(true) && state.matches(r, &s.search.text))
        .collect();
    body = body.child(section("Pinned", "pinned", pinned.len(), s, k, None));
    if !state.prefs.collapsed.contains("pinned") {
        if pinned.is_empty() {
            body = body.child(note(
                if state.recents.loading && !state.recents.loaded {
                    "Loading pinned conversations…"
                } else {
                    "Pin a conversation from its context menu to keep it here."
                },
                k,
            ));
        }
        for item in pinned {
            body = body.child(session_row(item, s, k));
        }
    }
    body = body.child(
        row()
            .w_full()
            .center_y()
            .child(section("Sessions", "recents", state.recents.rows.len(), s, k, None).grow(1.0))
            .child(filter_menu(s, k, layer)),
    );
    if !state.prefs.collapsed.contains("recents") {
        let rows = state.ordered(
            state
                .recents
                .rows
                .iter()
                .filter(|r| {
                    r.pinned != Some(true) && !canonical(r, s) && state.matches(r, &s.search.text)
                })
                .collect(),
        );
        body = render_list(body, rows, s, k);
        body = slice_feedback(
            body,
            &state.recents,
            "No conversations in this view.",
            "recents",
            s,
            k,
        );
    }
    let mut sources: BTreeMap<String, Vec<&SidebarSession>> = BTreeMap::new();
    for item in &state.messaging.rows {
        if item.pinned != Some(true) && !canonical(item, s) && state.matches(item, &s.search.text) {
            sources
                .entry(item.source.clone().unwrap_or_else(|| "messaging".into()))
                .or_default()
                .push(item);
        }
    }
    if let Some(error) = &state.messaging.error {
        body = body
            .child(note(format!("Messaging sessions: {error}"), k))
            .child(button(
                "Retry messaging sessions",
                Action::Refresh,
                state.messaging.loading,
                k,
            ));
    }
    for (source, items) in sources {
        let key = format!("source:{source}");
        let label = source_label(&source);
        body = body.child(section(&label, &key, items.len(), s, k, None));
        if !state.prefs.collapsed.contains(&key) {
            for item in state.ordered(items) {
                body = body.child(session_row(item, s, k));
            }
        }
    }
    if state.messaging.truncated.values().any(|v| *v) {
        body = body.child(button(
            "More messaging conversations",
            Action::LoadMore("messaging".into()),
            state.messaging.loading,
            k,
        ));
    }
    if !s.simple_mode {
        body = cron(body, s, k);
    }
    let mut panel = col()
        .key("sessions-navigation-native")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(
            col()
                .w_full()
                .gap(10.0)
                .padding(8.0)
                .child(shell::sessions_nav(s, k))
                .child(
                    TextInput::new(&s.search)
                        .leading_icon(&ic::MAGNIFYING_GLASS)
                        .width(None)
                        .placeholder("Search conversations…")
                        .on_edit(|s: &mut Viewer| &mut s.search)
                        .build(k)
                        .key("session-search"),
                ),
        )
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(body),
        );
    if state.prefs.rail {
        panel = panel.child(shell::profile_rail(s, p, k));
    } else {
        panel = panel.child(
            Button::new(format!("Profile: {}", s.profile))
                .variant(ButtonVariant::GhostMuted)
                .on_press(|s: &mut Viewer| s.profile_picker = !s.profile_picker)
                .build(k)
                .key("collapsed-profile-rail"),
        );
        if s.profile_picker {
            panel = panel.child(shell::profile_rail(s, p, k));
        }
    }
    panel.child(context_menu(s, k, layer))
}
fn visible_rows(s: &Viewer) -> Vec<&SidebarSession> {
    let mut seen = HashSet::new();
    s.sidebar_state
        .all_rows()
        .into_iter()
        .filter(|r| !canonical(r, s) && seen.insert(r.key()))
        .collect()
}
fn canonical(row: &SidebarSession, s: &Viewer) -> bool {
    row.hidden
        || row.is_internal_child
        || row.title.as_deref().map(str::trim) == Some("Bot Chat")
        || row.root_title.as_deref().map(str::trim) == Some("Bot Chat")
        || s.profiles
            .iter()
            .find(|p| p.name == row.profile())
            .and_then(|p| p.canonical_session.as_ref())
            .is_some_and(|bot| {
                row.aliases()
                    .iter()
                    .any(|id| *id == bot.id || *id == bot.resolved_id)
            })
}
fn session_row(item: &SidebarSession, s: &Viewer, k: &Kit) -> El {
    let state = &s.sidebar_state;
    let profile = item.profile().to_owned();
    let id = item.id.clone();
    let status = state.status(item);
    let target = (profile.clone(), id.clone());
    let density = s.client_settings.prefs.session_density.as_str();
    let mut line = SidebarRow::new(item.title())
        .height(if state.prefs.cards {
            66.0
        } else {
            match density {
                "comfortable" => 44.0,
                "detailed" => 66.0,
                _ => 34.0,
            }
        })
        .detail(relative_age(item.recency(), state.now))
        .selected(s.profile == profile && s.selected_session.as_ref() == Some(&id))
        .depth(if item._branched_from.is_some() { 1 } else { 0 })
        .on_select(move |s: &mut Viewer, _| {
            s.sidebar_action(Action::Open(profile.clone(), id.clone()))
        });
    line = line.status(match status {
        Status::Working => RowStatus::Busy,
        Status::NeedsInput => RowStatus::Attention,
        Status::Unread => RowStatus::Unread,
        Status::Draft => RowStatus::Other(text("○").font(k.ty.small).color(k.c.text_faint)),
        Status::Idle => RowStatus::Other(el().size(4.0, 4.0).radius(2.0).fill(k.c.text_faint)),
    });
    if state.prefs.cards || density == "detailed" {
        let mut metadata = vec![
            project(item)
                .rsplit('/')
                .next()
                .unwrap_or("Home")
                .to_owned(),
        ];
        if let Some(model) = &item.model {
            metadata.push(model.clone());
        }
        if (state.prefs.preview || density == "detailed")
            && let Some(preview) = &item.preview
        {
            metadata.push(preview.chars().take(90).collect());
        }
        line = line.subtitle(metadata.join(" · "));
    }
    let mut extras = vec![];
    if state.prefs.profile || state.prefs.all_profiles {
        extras.push(item.profile().to_owned());
    }
    if state.prefs.tokens {
        extras.push(format!("{} tokens", item.tokens()));
    }
    if state.prefs.cost && item.cost() > 0.0 {
        extras.push(format!("${:.2}", item.cost()));
    }
    if item.archived {
        extras.push("Archived".into());
    }
    if item.continuation_kind.as_deref() == Some("compression") {
        extras.push("Continued".into());
    }
    if !extras.is_empty() {
        line = line.badge(extras.join(" · "));
    }
    line.build(k)
        .key(format!("session-{}-{}", item.profile(), item.id))
        .help(format!(
            "{} · {} · {}",
            item.profile(),
            source_label(item.source.as_deref().unwrap_or_default()),
            status.label()
        ))
        .on_secondary_press(move |s: &mut Viewer, point, _| {
            s.sidebar_state.menu = Some((target.0.clone(), target.1.clone(), point));
        })
}
fn render_list(mut body: El, rows: Vec<&SidebarSession>, s: &Viewer, k: &Kit) -> El {
    let state = &s.sidebar_state;
    if rows.is_empty() {
        return body;
    }
    match state.prefs.grouping {
        Grouping::Date => {
            let mut hidden = false;
            for row in date_rows(rows, state.now) {
                match row {
                    ListRow::Divider(key, label) => {
                        hidden = state.prefs.collapsed.contains(&key);
                        body = body.child(section(&label, &key, 0, s, k, None));
                    }
                    ListRow::Session(item) => {
                        if !hidden {
                            body = body.child(session_row(&item, s, k));
                        }
                    }
                }
            }
        }
        Grouping::None => {
            for item in rows {
                body = body.child(session_row(item, s, k));
            }
        }
        grouping => {
            let mut groups: BTreeMap<String, Vec<&SidebarSession>> = BTreeMap::new();
            for item in rows {
                let label = match grouping {
                    Grouping::Profile => item.profile().into(),
                    Grouping::Project => project(item).into(),
                    _ => state.status(item).label().into(),
                };
                groups.entry(label).or_default().push(item);
            }
            for (label, rows) in groups {
                let key = format!("group:{label}");
                body = body.child(section(&label, &key, rows.len(), s, k, None));
                if !state.prefs.collapsed.contains(&key) {
                    for item in rows {
                        body = body.child(session_row(item, s, k));
                    }
                }
            }
        }
    }
    body
}
fn slice_feedback(
    mut body: El,
    slice: &crate::sidebar::Slice,
    empty: &str,
    name: &str,
    s: &Viewer,
    k: &Kit,
) -> El {
    if let Some(error) = &slice.error {
        body = body.child(note(error, k).role("alert")).child(button(
            "Retry sessions",
            Action::Refresh,
            slice.loading,
            k,
        ));
    } else if slice.loading && !slice.loaded {
        body = body.child(note("Loading conversations…", k));
    } else if slice.rows.is_empty() {
        body = body.child(note(empty, k));
    }
    if slice.truncated.values().any(|v| *v) {
        body = body.child(button(
            "Load more conversations",
            Action::LoadMore(name.into()),
            slice.loading || s.sidebar_state.demo,
            k,
        ));
    }
    body
}
fn section(label: &str, key: &str, count: usize, s: &Viewer, k: &Kit, meta: Option<String>) -> El {
    let id = key.to_owned();
    row()
        .key(format!("sidebar-section-{key}"))
        .w_full()
        .pt(10.0)
        .pb(4.0)
        .center_y()
        .child(
            Button::new(label)
                .icon(if s.sidebar_state.prefs.collapsed.contains(key) {
                    &ic::CHEVRON_RIGHT
                } else {
                    &ic::CHEVRON_DOWN
                })
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut Viewer| s.sidebar_action(Action::Collapse(id.clone())))
                .build(k)
                .grow(1.0)
                .min_w(0.0)
                .justify_start(),
        )
        .children((count > 0 || meta.is_some()).then(|| {
            text(meta.unwrap_or_else(|| count.to_string()))
                .font(k.ty.small)
                .color(k.c.text_faint)
        }))
}
fn cron(mut body: El, s: &Viewer, k: &Kit) -> El {
    let state = &s.sidebar_state;
    body = body.child(section(
        "Cron jobs",
        "cron-jobs",
        state.jobs.len(),
        s,
        k,
        None,
    ));
    if state.prefs.collapsed.contains("cron-jobs") {
        return body;
    }
    if let Some(error) = &state.jobs_error {
        return body
            .child(note(error, k))
            .child(button("Retry jobs", Action::Refresh, false, k));
    }
    if !state.jobs_loaded {
        return body.child(note("Loading scheduled jobs…", k));
    }
    if state.jobs.is_empty() {
        return body.child(note("No scheduled jobs on this profile.", k));
    }
    for job in state.jobs.iter().take(state.jobs_visible) {
        let id = job.id.clone();
        let expanded = state.peek_job.as_ref() == Some(&id);
        let next = job_meta(job, state.now);
        body = body.child(
            row()
                .w_full()
                .center_y()
                .child(
                    Button::new(job.title())
                        .icon(if expanded {
                            &ic::CHEVRON_DOWN
                        } else {
                            &ic::CHEVRON_RIGHT
                        })
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(move |s: &mut Viewer| {
                            s.sidebar_action(Action::PeekJob(id.clone()))
                        })
                        .build(k)
                        .grow(1.0)
                        .min_w(0.0)
                        .justify_start(),
                )
                .child(text(next).font(k.ty.small).color(k.c.text_faint)),
        );
        if expanded {
            let id = job.id.clone();
            let busy = state.demo || state.jobs_busy.contains(&id);
            body = body
                .child(note(job.schedule(), k))
                .child(
                    row()
                        .gap(4.0)
                        .child(button("Manage", Action::ManageJob(id.clone()), false, k))
                        .child(button("Run", Action::TriggerJob(id.clone()), busy, k))
                        .child(button(
                            if matches!(job.state(), "paused" | "disabled") {
                                "Resume"
                            } else {
                                "Pause"
                            },
                            Action::PauseJob(id.clone()),
                            busy,
                            k,
                        )),
                )
                .child(button("Delete job", Action::DeleteJob(id.clone()), busy, k));
            if state.pending.contains(&format!("sidebar.runs.{id}")) {
                body = body.child(note("Loading recent runs…", k));
            }
            for (index, run) in state.runs.iter().enumerate() {
                let label = run
                    .started_at
                    .map(|time| {
                        chrono::Local
                            .timestamp_opt(time as i64, 0)
                            .single()
                            .map(|date| date.format("%b %-d, %H:%M").to_string())
                            .unwrap_or_default()
                    })
                    .unwrap_or_else(|| "Scheduled run".into());
                body = body.child(button(
                    label,
                    Action::OpenRun(index),
                    run.source.as_deref() != Some("cron"),
                    k,
                ));
            }
            if state.runs.is_empty() && !state.pending.contains(&format!("sidebar.runs.{id}")) {
                body = body.child(note("No recent runs.", k));
            }
        }
    }
    if state.jobs.len().min(50) > state.jobs_visible {
        body = body.child(button("More scheduled jobs", Action::MoreJobs, false, k));
    }
    body
}
fn job_meta(job: &crate::pages::cron::Job, now: f64) -> String {
    if matches!(job.state(), "paused" | "disabled" | "completed" | "error") {
        return job.state().into();
    }
    let Some(next) = job
        .next_run_at
        .as_deref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
    else {
        return job.state().into();
    };
    let delta = next.timestamp() as f64 - now;
    let age = relative_age(now - delta.abs(), now);
    if delta < 0.0 {
        format!("Overdue {age}")
    } else {
        format!("In {age}")
    }
}
fn filter_menu(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let state = &s.sidebar_state;
    let mut menu = Menu::new("sidebar-filters")
        .trigger(
            IconButton::new(&ic::SETTINGS_GEAR)
                .variant(IconButtonVariant::GhostMuted)
                .label("Filter sessions")
                .on_press(|s: &mut Viewer| s.sidebar_action(Action::Filters))
                .build(k)
                .key("sessions-filter"),
        )
        .open(state.filter_open)
        .min_width(220.0)
        .on_close(|s: &mut Viewer| {
            s.sidebar_state.filter_open = false;
            s.sidebar_state.open_select = None;
        });
    let groups = Grouping::ALL
        .into_iter()
        .map(|group| {
            MenuEntry::item(
                MenuItem::new(group.label())
                    .checked(state.prefs.grouping == group)
                    .on_select(move |s: &mut Viewer| s.sidebar_action(Action::Grouping(group))),
            )
        })
        .collect();
    menu = menu.item(submenu("Grouping", "grouping", groups, s));
    let ordering = Ordering::ALL
        .into_iter()
        .filter(|order| {
            *order != Ordering::Cost
                || state.all_rows().iter().any(|r| r.cost() > 0.0)
                || state.prefs.ordering == Ordering::Cost
        })
        .map(|order| {
            MenuEntry::item(
                MenuItem::new(order.label())
                    .checked(state.prefs.ordering == order)
                    .on_select(move |s: &mut Viewer| s.sidebar_action(Action::Ordering(order))),
            )
        })
        .collect();
    menu = menu.item(submenu("Ordering", "ordering", ordering, s));
    let options = [
        ("cards", "Inbox style", state.prefs.cards),
        ("preview", "Preview", state.prefs.preview),
        ("tokens", "Tokens", state.prefs.tokens),
        ("cost", "Cost", state.prefs.cost),
        ("profile", "Profile labels", state.prefs.profile),
        ("rail", "Profile rail", state.prefs.rail),
    ];
    let show = options
        .into_iter()
        .filter(|(key, _, _)| !s.simple_mode || matches!(*key, "cards" | "preview"))
        .map(|(key, label, checked)| {
            MenuEntry::item(
                MenuItem::new(label)
                    .checked(checked)
                    .on_select(move |s: &mut Viewer| s.sidebar_action(Action::Toggle(key))),
            )
        })
        .collect();
    menu = menu.item(submenu("Show", "show", show, s)).separator();
    let status = Status::ALL
        .into_iter()
        .map(|status| {
            let name = status.label().to_owned();
            MenuEntry::item(
                MenuItem::new(&name)
                    .checked(state.prefs.status.contains(&name))
                    .on_select(move |s: &mut Viewer| {
                        s.sidebar_action(Action::Status(name.clone()))
                    }),
            )
        })
        .collect();
    menu = menu.item(submenu("Status", "status", status, s));
    let profiles = s
        .profiles
        .iter()
        .map(|profile| {
            let name = profile.name.clone();
            MenuEntry::item(
                MenuItem::new(&profile.label)
                    .checked(state.prefs.profiles.contains(&name))
                    .on_select(move |s: &mut Viewer| {
                        s.sidebar_action(Action::Profile(name.clone()))
                    }),
            )
        })
        .collect();
    if state.prefs.all_profiles {
        menu = menu.item(submenu("Profiles", "profiles", profiles, s));
    }
    let projects: BTreeSet<_> = state
        .all_rows()
        .iter()
        .map(|r| project(r).to_owned())
        .collect();
    if projects.len() > 1 {
        let entries = projects
            .into_iter()
            .map(|project| {
                MenuEntry::item(
                    MenuItem::new(&project)
                        .checked(state.prefs.projects.contains(&project))
                        .on_select(move |s: &mut Viewer| {
                            s.sidebar_action(Action::Project(project.clone()))
                        }),
                )
            })
            .collect();
        menu = menu.item(submenu("Projects", "projects", entries, s));
    }
    menu.item(
        MenuItem::new("All profiles")
            .checked(state.prefs.all_profiles)
            .on_select(|s: &mut Viewer| s.sidebar_action(Action::Toggle("all-profiles"))),
    )
    .item(
        MenuItem::new("Include archived")
            .checked(state.prefs.archived)
            .on_select(|s: &mut Viewer| s.sidebar_action(Action::Toggle("archived"))),
    )
    .separator()
    .item(
        MenuItem::new("Expand or collapse all groups")
            .on_select(|s: &mut Viewer| s.sidebar_action(Action::CollapseAll)),
    )
    .item(
        MenuItem::new("Mark all read")
            .disabled(
                state.demo
                    || state.blocks_owner_change()
                    || !state.all_rows().iter().any(|r| r.unread == Some(true)),
            )
            .on_select(|s: &mut Viewer| s.sidebar_action(Action::MarkAllRead)),
    )
    .item(
        MenuItem::new("Reset view").on_select(|s: &mut Viewer| s.sidebar_action(Action::ResetView)),
    )
    .item(MenuItem::new("Refresh").on_select(|s: &mut Viewer| s.sidebar_action(Action::Refresh)))
    .build(k, layer)
}
fn submenu(label: &str, key: &str, entries: Vec<MenuEntry>, s: &Viewer) -> MenuItem {
    let id = key.to_owned();
    MenuItem::new(label)
        .submenu(entries, s.sidebar_state.open_select.as_deref() == Some(key))
        .on_select(move |s: &mut Viewer| {
            s.sidebar_state.open_select = if s.sidebar_state.open_select.as_ref() == Some(&id) {
                None
            } else {
                Some(id.clone())
            };
        })
}
fn context_menu(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let Some((profile, id, point)) = &s.sidebar_state.menu else {
        return el().size(0.0, 0.0);
    };
    let Some(item) = s.sidebar_state.find(profile, id) else {
        return el().size(0.0, 0.0);
    };
    let profile = profile.clone();
    let id = id.clone();
    let disabled = s.sidebar_state.demo
        || s.sidebar_state.blocks_owner_change()
        || item.connection_id.is_some();
    let mut menu = Menu::new("sidebar-session-actions")
        .open(true)
        .at(point.x, point.y)
        .on_close(|s: &mut Viewer| s.sidebar_state.menu = None);
    for (label, action) in [
        ("Open", Action::Open(profile.clone(), id.clone())),
        (
            if item.pinned == Some(true) {
                "Unpin"
            } else {
                "Pin"
            },
            Action::Pin(profile.clone(), id.clone(), item.pinned != Some(true)),
        ),
        (
            if item.unread == Some(true) {
                "Mark read"
            } else {
                "Mark unread"
            },
            Action::Unread(profile.clone(), id.clone(), item.unread != Some(true)),
        ),
        ("Rename", Action::Rename(profile.clone(), id.clone())),
        (
            if item.archived {
                "Unarchive"
            } else {
                "Archive"
            },
            Action::Archive(profile.clone(), id.clone(), !item.archived),
        ),
        ("Delete", Action::Delete(profile.clone(), id.clone())),
    ] {
        let is_open = matches!(&action, Action::Open(..));
        menu = menu.item(
            MenuItem::new(label)
                .disabled(!is_open && disabled)
                .on_select(move |s: &mut Viewer| {
                    s.sidebar_state.menu = None;
                    s.sidebar_action(action.clone());
                }),
        );
    }
    let target = crate::app::ConversationTarget::Session {
        profile: profile.clone(),
        id: id.clone(),
        title: item.title.clone().unwrap_or_else(|| "Conversation".into()),
    };
    menu = menu.item(
        MenuItem::new("Open in new window")
            .disabled(!s.can_open_window() || item.connection_id.is_some())
            .on_select(move |s: &mut Viewer| s.request_window_target(Some(target.clone()))),
    );
    menu.build(k, layer)
}
fn note(value: impl Into<String>, k: &Kit) -> El {
    text(value.into())
        .font(k.ty.small)
        .color(k.c.text_faint)
        .wrap()
        .px(8.0)
        .py(5.0)
}
fn button(label: impl Into<String>, action: Action, disabled: bool, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .disabled(disabled)
        .on_press(move |s: &mut Viewer| s.sidebar_action(action.clone()))
        .build(k)
}
use chrono::TimeZone;
