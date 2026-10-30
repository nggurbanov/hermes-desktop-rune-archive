//! Source-qualified fleet rail. Rendering consumes cached facts and never dials.
use crate::{
    app::Viewer,
    connections::Kind,
    fleet::{Agent, Snapshot, Status},
    source::SourceVersion,
};
use rune::{
    Color, Cursor, Drag, El, Phase, PointerPress, ScrollSpec, TextEdit, Tx, col, el, row, text,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Menu, MenuItem,
    TextInput,
};
use rune_theme_opencode::icons as ic;
use std::time::{Duration, Instant};
static HOME: rune::Icon = rune::Icon {
    name: "hermes-home",
    view: 16.0,
    body: r#"<path d="M2 7.5 8 2l6 5.5V14H9.5v-4h-3v4H2Z" fill="none" stroke="currentColor" stroke-linejoin="round"/>"#,
};
static DEVICE: rune::Icon = rune::Icon {
    name: "hermes-device",
    view: 16.,
    body: r#"<path d="M1.5 2.5h13v9h-13ZM8 11.5V14M4.5 14h7" fill="none" stroke="currentColor" stroke-linejoin="round"/>"#,
};
static LAYERS: rune::Icon = rune::Icon {
    name: "hermes-layers",
    view: 16.0,
    body: r#"<path d="m1.5 5 6.5-3.5L14.5 5 8 8.5ZM1.5 8 8 11.5 14.5 8M1.5 11 8 14.5l6.5-3.5" fill="none" stroke="currentColor" stroke-linejoin="round"/>"#,
};

#[derive(Clone, Copy)]
pub enum ManageAction {
    Manage,
    Create,
    Import,
}

#[derive(Clone)]
pub struct ProfileTarget {
    pub version: SourceVersion,
    pub profile: String,
    pub point: PointerPress,
}
#[derive(Clone)]
struct GroupTarget {
    version: SourceVersion,
    key: String,
    label: String,
    point: PointerPress,
}
struct Hold {
    target: ProfileTarget,
    deadline: Instant,
}
#[derive(Default)]
pub struct State {
    pub profile: Option<ProfileTarget>,
    group: Option<GroupTarget>,
    color: Option<ProfileTarget>,
    alias_group: Option<GroupTarget>,
    alias: TextEdit,
    hold: Option<Hold>,
}
impl State {
    pub fn open_profile(&mut self, version: SourceVersion, profile: String, point: PointerPress) {
        self.group = None;
        self.color = None;
        self.profile = Some(ProfileTarget {
            version,
            profile,
            point,
        });
    }
    pub fn close(&mut self) {
        self.profile = None;
        self.group = None;
        self.color = None;
        self.alias_group = None;
        self.hold = None;
    }
    pub fn poll(&mut self) -> bool {
        if self
            .hold
            .as_ref()
            .is_some_and(|h| h.deadline <= Instant::now())
        {
            let hold = self.hold.take().unwrap();
            self.color = Some(hold.target);
            return true;
        }
        false
    }
    pub fn wake_after(&self) -> Option<Duration> {
        self.hold
            .as_ref()
            .map(|h| h.deadline.saturating_duration_since(Instant::now()))
    }
}
#[derive(Clone)]
struct Choice {
    version: Option<SourceVersion>,
    profile: String,
    label: String,
    handle: String,
    active_source: bool,
    activity: Option<String>,
}
#[derive(Clone)]
struct Group {
    version: Option<SourceVersion>,
    key: String,
    label: String,
    kind: Kind,
    active: bool,
    status: Status,
    diagnostic: Option<String>,
    choices: Vec<Choice>,
}

pub fn source_key(version: &SourceVersion) -> String {
    serde_json::to_string(&version.id).expect("source identity is serializable")
}
fn route_key(version: &SourceVersion, profile: &str) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(serde_json::json!([version, profile]).to_string().as_bytes())
    )
}
fn choice_key(choice: &Choice, picker: bool) -> String {
    let prefix = if picker { "pick-profile" } else { "profile" };
    if choice.active_source {
        format!("{prefix}-{}", choice.profile)
    } else {
        format!(
            "fleet-{prefix}-{}",
            route_key(choice.version.as_ref().unwrap(), &choice.profile)
        )
    }
}
fn agent_label(agent: &Agent) -> String {
    agent.metadata.gateway_profile(&agent.profile).label
}

fn groups(s: &Viewer) -> Vec<Group> {
    let active = s.fleet_active_version();
    let mut groups = project_current(
        s.fleet_snapshot(),
        active.as_ref(),
        &s.profiles,
        &s.endpoint_label,
        if s.is_local_connection() {
            Kind::Local
        } else {
            Kind::Remote
        },
        &s.fleet_preferences,
    );
    for group in &mut groups {
        for choice in &mut group.choices {
            choice.activity = activity(s, choice);
        }
    }
    groups
}
fn project_current(
    snapshot: &Snapshot,
    active: Option<&SourceVersion>,
    profiles: &[crate::gateway::Profile],
    endpoint_label: &str,
    kind: Kind,
    prefs: &crate::client::fleet::Preferences,
) -> Vec<Group> {
    let mut groups = project(snapshot, active, prefs);
    let has_active = groups.iter().any(|g| g.active);
    let identity_replaced = active.is_some_and(|active| {
        groups.iter().any(|g| {
            g.version
                .as_ref()
                .is_some_and(|v| v.id == active.id && v != active)
        })
    });
    if !has_active && !identity_replaced && !profiles.is_empty() {
        groups.insert(
            0,
            Group {
                version: active.cloned(),
                key: active.map(source_key).unwrap_or_else(|| "current".into()),
                label: endpoint_label.into(),
                kind,
                active: true,
                status: Status::Ready,
                diagnostic: None,
                choices: vec![],
            },
        );
    }
    if !identity_replaced {
        let choices = crate::client::fleet::ordered(profiles, prefs.profile_order(), |p| &p.name)
            .into_iter()
            .map(|profile| Choice {
                version: active.cloned(),
                profile: profile.name.clone(),
                label: profile.label.clone(),
                handle: profile.name.clone(),
                active_source: true,
                activity: None,
            })
            .collect();
        merge_current(&mut groups, snapshot, active, choices, prefs);
    }
    groups
}
// A physical-install alias chooses a display slot only. Every action keeps its
// actual captured source version, including a raw current connection.
fn merge_current(
    groups: &mut Vec<Group>,
    snapshot: &Snapshot,
    actual: Option<&SourceVersion>,
    choices: Vec<Choice>,
    prefs: &crate::client::fleet::Preferences,
) {
    for group in groups.iter_mut() {
        group.choices.retain(|choice| !choice.active_source);
        group.active = false;
    }
    for mut choice in choices {
        let display = actual
            .and_then(|v| snapshot.canonical_source(v, &choice.profile))
            .or_else(|| actual.cloned());
        if let Some(agent) = snapshot.agents.iter().find(|agent| {
            Some(&agent.source) == display.as_ref() && agent.profile == choice.profile
        }) {
            choice.handle = agent.handle.clone();
        }
        if let Some(group) = groups.iter_mut().find(|g| g.version == display) {
            group.choices.retain(|old| old.profile != choice.profile);
            group.choices.push(choice);
            group.active = true;
        }
    }
    for group in groups.iter_mut() {
        group.choices =
            crate::client::fleet::ordered(&group.choices, prefs.profile_order(), |c| &c.profile)
                .into_iter()
                .cloned()
                .collect();
        group
            .choices
            .sort_by_key(|choice| choice.profile != "default");
    }
    groups.retain(|group| {
        if !group.choices.is_empty() {
            return true;
        }
        let Some(source) = snapshot
            .sources
            .iter()
            .find(|source| Some(&source.descriptor.version) == group.version.as_ref())
        else {
            return true;
        };
        !(matches!(source.status, Status::Ready)
            && source.inventory_error.is_none()
            && !source.profiles.is_empty()
            && source.profiles.iter().all(|profile| {
                snapshot
                    .canonical_source(&source.descriptor.version, &profile.name)
                    .is_some_and(|canonical| canonical != source.descriptor.version)
            }))
    });
}
fn activity_visible(active_source: bool, same_profile: bool, all_profiles: bool) -> bool {
    !(active_source && same_profile && !all_profiles)
}
fn activity(s: &Viewer, choice: &Choice) -> Option<String> {
    if !activity_visible(
        choice.active_source,
        choice.profile == s.profile,
        s.fleet_all_profiles_active(),
    ) {
        return None;
    }
    let remote = s.fleet_sidebar_groups();
    let state = if choice.active_source {
        Some(&s.sidebar_state)
    } else {
        remote
            .iter()
            .find(|(owner, _, _)| choice.version.as_ref() == Some(owner.source.version()))
            .map(|(_, _, state)| *state)
    }?;
    activity_summary(state, &choice.profile)
}
fn activity_summary(state: &crate::sidebar::SidebarState, profile: &str) -> Option<String> {
    let mut needs = 0;
    let mut working = 0;
    let mut unread = 0;
    for session in crate::sidebar::lineage_rows(state.all_rows())
        .iter()
        .filter(|r| r.profile() == profile)
    {
        match state.status(session) {
            crate::sidebar::Status::NeedsInput => needs += 1,
            crate::sidebar::Status::Working => working += 1,
            crate::sidebar::Status::Unread => unread += 1,
            _ => {}
        }
    }
    let mut labels = vec![];
    if needs > 0 {
        labels.push(format!("{needs} need your answer"));
    }
    if working > 0 {
        labels.push(format!("{working} working"));
    }
    if unread > 0 {
        labels.push(format!("{unread} unread"));
    }
    (!labels.is_empty()).then(|| labels.join(", "))
}
fn project(
    snapshot: &Snapshot,
    active: Option<&SourceVersion>,
    prefs: &crate::client::fleet::Preferences,
) -> Vec<Group> {
    let mut sources: Vec<_> = snapshot.sources.iter().collect();
    sources.sort_by(|a, b| {
        let ak = source_key(&a.descriptor.version);
        let bk = source_key(&b.descriptor.version);
        let rank = |key: &str| prefs.source_order().iter().position(|s| s == key);
        match (rank(&ak), rank(&bk)) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => (a.descriptor.kind != Kind::Local)
                .cmp(&(b.descriptor.kind != Kind::Local))
                .then_with(|| {
                    crate::client::fleet::compare_labels(&a.descriptor.label, &b.descriptor.label)
                })
                .then_with(|| {
                    crate::client::fleet::compare_labels(
                        &source_key(&a.descriptor.version),
                        &source_key(&b.descriptor.version),
                    )
                }),
        }
    });
    sources
        .into_iter()
        .map(|source| {
            let version = &source.descriptor.version;
            let key = source_key(version);
            let is_active = active == Some(version);
            let mut choices = crate::client::fleet::ordered(
                snapshot.agents.iter().filter(|a| a.source == *version),
                prefs.profile_order(),
                |a| &a.profile,
            )
            .into_iter()
            .map(|agent| Choice {
                version: Some(version.clone()),
                profile: agent.profile.clone(),
                label: agent_label(agent),
                handle: agent.handle.clone(),
                active_source: is_active,
                activity: None,
            })
            .collect::<Vec<_>>();
            choices.sort_by_key(|c| c.profile != "default");
            Group {
                version: Some(version.clone()),
                label: prefs
                    .group_alias(&key)
                    .unwrap_or(&source.descriptor.label)
                    .to_owned(),
                key,
                kind: source.descriptor.kind,
                active: is_active,
                status: source.status.clone(),
                diagnostic: source.inventory_error.clone(),
                choices,
            }
        })
        .collect()
}
fn condensed(groups: &[Group]) -> bool {
    groups.iter().map(|g| g.choices.len().max(1)).sum::<usize>() > 13
}
fn status_label(status: &Status) -> &str {
    match status {
        Status::Ready => "Ready",
        Status::ConnectOnDemand => "Connect on demand",
        Status::Unavailable(error) => error,
    }
}
fn color(s: &Viewer, name: &str, k: &Kit) -> Color {
    s.fleet_preferences
        .profile_color(name)
        .ok()
        .flatten()
        .unwrap_or(k.c.text_muted)
}
fn color_note(s: &Viewer, name: &str) -> &'static str {
    s.fleet_preferences.profile_color(name).err().unwrap_or("")
}

fn result(s: &mut Viewer, r: Result<(), String>) {
    if let Err(error) = r {
        s.client_error = Some(error);
    }
}
fn open(s: &mut Viewer, choice: &Choice) {
    if s.fleet_ui.color.is_some() {
        return;
    }
    s.profile_picker = false;
    if let Some(version) = &choice.version {
        s.fleet_open_profile(version.clone(), choice.profile.clone());
    } else {
        s.select_profile(choice.profile.clone());
    }
}
fn context(s: &mut Viewer, choice: &Choice, point: PointerPress) {
    if let Some(version) = &choice.version {
        s.fleet_profile_context(version.clone(), choice.profile.clone(), point);
    } else {
        s.open_profile_route_menu(choice.profile.clone(), point);
    }
}
fn glyph(s: &Viewer, choice: &Choice, k: &Kit) -> El {
    if choice.profile == "default" {
        let device = !choice.active_source
            && s.fleet_snapshot().sources.iter().any(|source| {
                choice.version.as_ref() == Some(&source.descriptor.version)
                    && source.descriptor.kind == Kind::Local
            });
        return rune::icon(if device { &DEVICE } else { &HOME }, 15.)
            .size(15., 15.)
            .color(k.c.text_muted);
    }
    let initial = choice
        .label
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    el().size(20., 20.)
        .radius(4.)
        .fill(k.alpha(
            color(s, &choice.profile, k),
            color(s, &choice.profile, k).a * 0.18,
        ))
        .center()
        .child(
            text(initial)
                .font(k.ty.small)
                .color(color(s, &choice.profile, k)),
        )
}
fn square(s: &Viewer, choice: &Choice, group_label: &str, k: &Kit, sortable: &[Choice]) -> El {
    let active =
        choice.active_source && choice.profile == s.profile && !s.fleet_all_profiles_active();
    let own = choice.clone();
    let menu = choice.clone();
    let drag = choice.clone();
    let key = choice_key(choice, false);
    let drag_key = key.clone();
    let order = sortable
        .iter()
        .filter(|c| c.profile != "default")
        .map(|c| (choice_key(c, false), c.profile.clone()))
        .collect::<Vec<_>>();
    let mut label = format!(
        "{} · {}{}",
        choice.label,
        group_label,
        if active { " · Current profile" } else { "" }
    );
    if !color_note(s, &choice.profile).is_empty() {
        label.push_str(&format!(" · {}", color_note(s, &choice.profile)));
    }
    if let Some(activity) = &choice.activity {
        label.push_str(&format!(" · {activity}"));
    }
    let mut square = el()
        .key(key)
        .role("button")
        .label(&label)
        .help(format!("{label}\n@{}", choice.handle))
        .focusable(true)
        .focus_selected(active)
        .focus_ring(2., 1., k.c.border_focus)
        .size(28., 28.)
        .center()
        .radius(6.)
        .fill(if active {
            k.c.layer_01
        } else {
            Color::TRANSPARENT
        })
        .opacity(if choice.active_source { 1. } else { 0.62 })
        .hover(k.c.hover)
        .on_press(move |s: &mut Viewer| open(s, &own))
        .on_secondary_press(move |s: &mut Viewer, p, _| context(s, &menu, p))
        .child(glyph(s, choice, k))
        .children(choice.activity.as_ref().map(|_| {
            el().key("profile-status-dot")
                .absolute(21., 1.)
                .size(6., 6.)
                .radius(3.)
                .fill(k.c.warning.fg)
        }));
    if choice.active_source && choice.profile != "default" {
        let rect = rune::memo::layout(&drag_key);
        let targets = order
            .iter()
            .filter_map(|(key, name)| rune::memo::layout(key).map(|rect| (name.clone(), rect)))
            .collect::<Vec<_>>();
        square = square.cursor(Cursor::Grab).on_drag(
            move |s: &mut Viewer, d: &mut Drag, tx: &mut Tx| match d.phase {
                Phase::Down => {
                    if let (Some(version), Some(rect)) = (&drag.version, rect) {
                        s.fleet_ui.hold = Some(Hold {
                            target: ProfileTarget {
                                version: version.clone(),
                                profile: drag.profile.clone(),
                                point: PointerPress {
                                    x: rect[0] + d.start_x,
                                    y: rect[1] + d.start_y,
                                },
                            },
                            deadline: Instant::now() + Duration::from_millis(500),
                        });
                        tx.wake_after(Duration::from_millis(500));
                    }
                }
                Phase::Move => {
                    if d.dx.abs() + d.dy.abs() > 4. {
                        s.fleet_ui.hold = None;
                    }
                }
                Phase::Up => {
                    s.fleet_ui.hold = None;
                    if d.cancelled() {
                        return;
                    }
                    if d.dx.abs() + d.dy.abs() > 4. {
                        if let Some(rect) = rect {
                            let x = rect[0] + d.start_x + d.dx;
                            if let Some((target, _)) =
                                targets.iter().find(|(_, r)| x >= r[0] && x <= r[0] + r[2])
                            {
                                let mut names =
                                    order.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>();
                                if let (Some(from), Some(to)) = (
                                    names.iter().position(|n| n == &drag.profile),
                                    names.iter().position(|n| n == target),
                                ) {
                                    let n = names.remove(from);
                                    names.insert(to, n);
                                    let r = s.fleet_preferences.set_profile_order(names);
                                    result(s, r);
                                }
                            }
                        }
                    }
                }
            },
        );
    }
    square
}
fn source_button(_s: &Viewer, group: &Group, k: &Kit) -> El {
    let glyph = match group.kind {
        Kind::Local => "Device",
        Kind::Remote => "Gateway",
        Kind::Ssh => "SSH",
    };
    let key = group.key.clone();
    let title = group.label.clone();
    Button::new(format!(
        "{glyph}{}",
        if matches!(group.status, Status::Ready) {
            ""
        } else {
            " ·"
        }
    ))
    .variant(ButtonVariant::GhostMuted)
    .control_size(ControlSize::Small)
    .help(format!("{} · {}", group.label, status_label(&group.status)))
    .on_press(move |s: &mut Viewer| {
        let collapsed = !s.fleet_preferences.collapsed(&key);
        let r = s.fleet_preferences.set_collapsed(key.clone(), collapsed);
        result(s, r);
    })
    .build(k)
    .key(format!(
        "fleet-group-{}",
        route_key(group.version.as_ref().unwrap(), "group")
    ))
    .on_secondary_press({
        let key = group.key.clone();
        let version = group.version.clone().unwrap();
        move |s: &mut Viewer, point, _| {
            s.fleet_ui.profile = None;
            s.fleet_ui.group = Some(GroupTarget {
                version: version.clone(),
                key: key.clone(),
                label: title.clone(),
                point,
            });
        }
    })
}
fn route_door(group: &Group, k: &Kit) -> El {
    let Some(version) = group.version.clone() else {
        return el();
    };
    Button::new(if matches!(group.status, Status::Unavailable(_)) {
        "Reconnect"
    } else {
        "Connect"
    })
    .variant(ButtonVariant::GhostMuted)
    .control_size(ControlSize::Small)
    .help(format!(
        "{} · {}. Validate the source before opening its Home profile",
        group.label,
        status_label(&group.status)
    ))
    .on_press(move |s: &mut Viewer| s.fleet_connect_source(version.clone()))
    .build(k)
    .key(format!(
        "fleet-source-connect-{}",
        route_key(group.version.as_ref().unwrap(), "door")
    ))
}
fn rail_policy(
    sidebar: bool,
    simple: bool,
    preference: bool,
    profiles: usize,
    sources: usize,
) -> bool {
    sidebar
        && if simple {
            profiles > 1 || sources > 1
        } else {
            preference
        }
}
pub fn rail_visible(s: &Viewer) -> bool {
    rail_policy(
        s.sidebar.is_open(),
        s.simple_mode,
        s.show_profile_rail,
        s.profiles.len().max(s.fleet_snapshot().agents.len()),
        s.connections
            .rows
            .len()
            .max(s.fleet_snapshot().sources.len()),
    )
}
pub fn rail(s: &Viewer, k: &Kit) -> El {
    if !rail_visible(s) {
        return el().h(0.);
    }
    let groups = groups(s);
    let compact = condensed(&groups);
    let mut strip = row().grow(1.).min_w(0.).gap(4.).center_y().scroll_x();
    if compact {
        strip = strip.child(
            Button::new(format!("{} ▾", s.profile))
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut Viewer| {
                    s.profile_picker = !s.profile_picker;
                    s.fleet_refresh();
                })
                .build(k)
                .key("profile-picker-toggle"),
        );
    } else {
        for group in &groups {
            let mut zone = row()
                .key(format!("fleet-zone-{}", group.key))
                .role("group")
                .label(&group.label)
                .gap(3.)
                .center_y();
            if groups.len() > 1 && group.version.is_some() {
                zone = zone.child(source_button(s, group, k));
            }
            if !s.fleet_preferences.collapsed(&group.key) {
                for choice in &group.choices {
                    if groups.len() == 1 && group.choices.len() > 1 && choice.profile == "default" {
                        continue;
                    }
                    zone = zone.child(square(s, choice, &group.label, k, &group.choices));
                }
                if group.choices.is_empty() {
                    zone = zone.child(route_door(group, k));
                }
            }
            strip = strip.child(zone);
        }
    }
    let current_version = s.fleet_active_version();
    let create = current_version.clone();
    let manage = current_version.clone();
    let import = current_version.clone();
    let fleet = groups.len() > 1;
    let home = groups
        .iter()
        .flat_map(|g| g.choices.iter())
        .find(|choice| choice.active_source && choice.profile == "default")
        .cloned();
    let many = groups.iter().map(|g| g.choices.len()).sum::<usize>() > 1;
    let home_action = home.clone();
    let home_face = !fleet && !s.fleet_all_profiles_active();
    let mut rail = row()
        .key("profile-rail")
        .role("group")
        .label("Session profile scope")
        .w_full()
        .h(40.)
        .px(4.)
        .gap(3.)
        .center_y()
        .children((fleet || many).then(|| {
            IconButton::new(if home_face && home.is_some() {
                &HOME
            } else {
                &LAYERS
            })
            .variant(IconButtonVariant::GhostMuted)
            .label(if fleet {
                "All profiles on this gateway"
            } else if home_face {
                "Home or all profiles"
            } else {
                "Return to Home"
            })
            .on_press(move |s: &mut Viewer| {
                if fleet {
                    s.fleet_show_all_profiles();
                } else if let Some(home) = &home_action {
                    if s.profile == "default" && !s.fleet_all_profiles_active() {
                        s.fleet_show_all_profiles();
                    } else {
                        open(s, home);
                    }
                } else {
                    s.fleet_show_all_profiles();
                }
            })
            .build(k)
            .key("fleet-all-profiles")
        }))
        .child(strip)
        .children((!compact).then(|| {
            IconButton::new(&ic::CHEVRON_DOWN)
                .variant(IconButtonVariant::GhostMuted)
                .label("Choose session profile")
                .on_press(|s: &mut Viewer| {
                    s.profile_picker = !s.profile_picker;
                    s.fleet_refresh();
                })
                .build(k)
                .key("profile-picker-toggle")
        }))
        .child(
            IconButton::new(&ic::PLUS)
                .variant(IconButtonVariant::GhostMuted)
                .label("New profile")
                .on_press(move |s: &mut Viewer| {
                    if let Some(version) = &create {
                        s.fleet_manage_source(version.clone(), ManageAction::Create);
                    } else {
                        s.open_route(crate::pages::Route::Bots);
                        s.bots_action(crate::pages::bots::Action::New);
                    }
                })
                .build(k)
                .key("fleet-new-profile"),
        )
        .child(
            Button::new("…")
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .help("Manage bots and connections")
                .on_press(move |s: &mut Viewer| {
                    if let Some(version) = &manage {
                        s.fleet_manage_source(version.clone(), ManageAction::Manage);
                    } else {
                        s.open_route(crate::pages::Route::Bots);
                    }
                })
                .build(k)
                .key("fleet-manage"),
        );
    rail = rail.child(
        IconButton::new(&ic::FOLDER)
            .label("Import profile")
            .variant(IconButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .disabled(import.is_none())
            .on_press(move |s: &mut Viewer| {
                if let Some(version) = &import {
                    s.fleet_manage_source(version.clone(), ManageAction::Import);
                }
            })
            .build(k)
            .key("fleet-import-profile"),
    );
    let mut footer = col().key("profile-footer").w_full();
    if s.profile_picker {
        footer = footer.child(picker(s, k, false));
    }
    footer
        .child(el().w_full().h(1.).fill(k.c.border_muted))
        .child(rail)
}
pub fn picker(s: &Viewer, k: &Kit, floating: bool) -> El {
    let mut menu = col()
        .key("profile-picker-rows")
        .growth(rune::GrowthSpec::default())
        .virtualize(30.)
        .w_full()
        .min_w(190.)
        .gap(2.)
        .padding(6.)
        .fill(k.c.bg_deep);
    for group in groups(s) {
        if s.fleet_snapshot().sources.len() > 1 {
            let key = group.key.clone();
            let collapsed = s.fleet_preferences.collapsed(&key);
            let target = group.version.clone().map(|version| GroupTarget {
                version,
                key: key.clone(),
                label: group.label.clone(),
                point: PointerPress { x: 0., y: 0. },
            });
            menu = menu.child(
                Button::new(format!(
                    "{} {}{}",
                    if collapsed { "▸" } else { "▾" },
                    group.label,
                    if matches!(group.status, Status::Ready) {
                        ""
                    } else {
                        " ·"
                    }
                ))
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .help(format!("{} · {}", group.label, status_label(&group.status)))
                .on_press(move |s: &mut Viewer| {
                    let r = s.fleet_preferences.set_collapsed(key.clone(), !collapsed);
                    result(s, r);
                })
                .build(k)
                .key(format!("fleet-picker-group-{}", group.key))
                .w_full()
                .justify_start()
                .on_secondary_press(move |s: &mut Viewer, point, _| {
                    if let Some(mut target) = target.clone() {
                        target.point = point;
                        s.fleet_ui.profile = None;
                        s.fleet_ui.group = Some(target);
                    }
                }),
            );
            if collapsed {
                continue;
            }
        }
        for choice in &group.choices {
            let own = choice.clone();
            let target = choice.clone();
            let current = choice.active_source
                && choice.profile == s.profile
                && !s.fleet_all_profiles_active();
            let leading = glyph(s, choice, k);
            let display = choice.label.clone();
            let activity = choice.activity.clone();
            menu = menu.child(
                Button::new(format!(
                    "{}{}",
                    choice.label,
                    if current { " ✓" } else { "" }
                ))
                .label_with(move |font, ink| {
                    row()
                        .gap(8.)
                        .center_y()
                        .child(leading)
                        .child(text(&display).font(font).color(ink).grow(1.).truncate())
                        .children(activity.as_ref().map(|_| text("●").font(font).color(ink)))
                        .children(current.then(|| text("✓").font(font).color(ink)))
                })
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .help(format!(
                    "{} · {}{}{}",
                    choice.label,
                    group.label,
                    if color_note(s, &choice.profile).is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", color_note(s, &choice.profile))
                    },
                    choice
                        .activity
                        .as_ref()
                        .map(|v| format!(" · {v}"))
                        .unwrap_or_default()
                ))
                .on_press(move |s: &mut Viewer| open(s, &own))
                .build(k)
                .key(choice_key(choice, true))
                .w_full()
                .justify_start()
                .on_secondary_press(move |s: &mut Viewer, p, _| context(s, &target, p)),
            );
        }
        if group.choices.is_empty() {
            menu = menu.child(route_door(&group, k));
        }
        if let Some(diagnostic) = &group.diagnostic {
            menu = menu.child(
                text(diagnostic)
                    .key(format!("fleet-inventory-error-{}", group.key))
                    .font(k.ty.small)
                    .color(k.c.text_faint)
                    .wrap(),
            );
        }
        if !matches!(group.status, Status::Ready) {
            menu = menu.child(
                text(status_label(&group.status))
                    .key(format!("fleet-picker-status-{}", group.key))
                    .font(k.ty.small)
                    .color(k.c.text_faint)
                    .wrap(),
            );
        }
    }
    if let Some(error) = s
        .fleet_snapshot()
        .error
        .as_deref()
        .or(s.fleet_preferences.error())
    {
        menu = menu.child(
            text(error)
                .key("fleet-picker-error")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    menu = menu
        .child(
            Button::new(if s.fleet_snapshot().refreshing {
                "Refreshing…"
            } else {
                "Refresh agents"
            })
            .variant(ButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .disabled(s.fleet_snapshot().refreshing)
            .on_press(|s: &mut Viewer| s.fleet_refresh())
            .build(k)
            .key("fleet-refresh"),
        )
        .child(
            Button::new("Manage bots")
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut Viewer| {
                    s.profile_picker = false;
                    s.open_route(crate::pages::Route::Bots);
                })
                .build(k)
                .key("fleet-picker-manage"),
        );
    col()
        .key("profile-picker")
        .role("menu")
        .label("Choose profile")
        .w_full()
        .min_w(210.)
        .max_h(if floating { 580. } else { 280. })
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(menu)
}

pub fn overlays(s: &Viewer, k: &Kit, layer: &mut Vec<El>) {
    if let Some(target) = &s.fleet_ui.profile {
        let is_default = s.fleet_is_default(&target.version, &target.profile);
        let can_set = s.fleet_can_set_default(&target.version, &target.profile);
        let open = target.clone();
        let default = target.clone();
        let mut menu = Menu::new("profile-route-actions")
            .open(true)
            .min_width(260.)
            .at(target.point.x, target.point.y)
            .on_close(|s: &mut Viewer| s.fleet_ui.close())
            .item(
                MenuItem::new("Open in new window")
                    .disabled(!s.can_open_window())
                    .on_select(move |s: &mut Viewer| {
                        s.fleet_ui.close();
                        s.fleet_open_window(open.version.clone(), open.profile.clone());
                    }),
            )
            .item(
                MenuItem::new(if is_default {
                    "Default profile"
                } else {
                    "Set as default"
                })
                .checked(is_default)
                .disabled(is_default || !can_set)
                .on_select(move |s: &mut Viewer| {
                    s.fleet_ui.close();
                    s.fleet_set_default(default.version.clone(), default.profile.clone());
                }),
            );
        if is_default {
            menu = menu.item(MenuItem::new("Clear default").on_select(|s: &mut Viewer| {
                s.fleet_ui.close();
                s.clear_default_profile();
            }));
        }
        if target.profile != "default" {
            let target = target.clone();
            menu = menu
                .separator()
                .item(
                    MenuItem::new("Profile color…").on_select(move |s: &mut Viewer| {
                        s.fleet_ui.profile = None;
                        s.fleet_ui.color = Some(target.clone());
                    }),
                );
        }
        let built = menu.build(k, layer);
        layer.push(built);
    }
    if let Some(target) = &s.fleet_ui.color {
        let mut menu = Menu::new("fleet-profile-colors")
            .open(true)
            .at(target.point.x, target.point.y)
            .on_close(|s: &mut Viewer| s.fleet_ui.close())
            .group_label("Profile color, shared by name");
        if !color_note(s, &target.profile).is_empty() {
            menu = menu.group_label(color_note(s, &target.profile));
        }
        for hue in (0..360).step_by(30) {
            let name = target.profile.clone();
            menu = menu.item(
                MenuItem::new(format!("Hue {hue}°"))
                    .lead(
                        el().size(14., 14.)
                            .radius(7.)
                            .fill(crate::client::fleet::hue_color(hue)),
                    )
                    .checked(s.fleet_preferences.explicit_profile_hue(&target.profile) == Some(hue))
                    .on_select(move |s: &mut Viewer| {
                        let r = s.fleet_preferences.set_color(name.clone(), Some(hue));
                        result(s, r);
                        s.fleet_ui.close();
                    }),
            );
        }
        let name = target.profile.clone();
        menu = menu.separator().item(
            MenuItem::new("Automatic color")
                .checked(
                    s.fleet_preferences
                        .explicit_profile_color(&target.profile)
                        .is_none(),
                )
                .on_select(move |s: &mut Viewer| {
                    let r = s.fleet_preferences.set_color(name.clone(), None);
                    result(s, r);
                    s.fleet_ui.close();
                }),
        );
        let built = menu.build(k, layer);
        layer.push(built);
    }
    if let Some(target) = &s.fleet_ui.group {
        let key = target.key.clone();
        let collapsed = s.fleet_preferences.collapsed(&key);
        let alias = target.clone();
        let move_key = key.clone();
        let reset_key = key.clone();
        let all_version = target.version.clone();
        let manage_version = target.version.clone();
        let create_version = target.version.clone();
        let import_version = target.version.clone();
        let menu = Menu::new("fleet-source-actions")
            .open(true)
            .at(target.point.x, target.point.y)
            .on_close(|s: &mut Viewer| s.fleet_ui.close())
            .group_label(&target.label)
            .item(
                MenuItem::new("All profiles on this gateway").on_select(move |s: &mut Viewer| {
                    s.fleet_ui.close();
                    s.fleet_show_source_all(all_version.clone());
                }),
            )
            .item(
                MenuItem::new("Manage profiles").on_select(move |s: &mut Viewer| {
                    s.fleet_ui.close();
                    s.fleet_manage_source(manage_version.clone(), ManageAction::Manage);
                }),
            )
            .item(
                MenuItem::new("New profile").on_select(move |s: &mut Viewer| {
                    s.fleet_ui.close();
                    s.fleet_manage_source(create_version.clone(), ManageAction::Create);
                }),
            )
            .item(
                MenuItem::new("Import profile…").on_select(move |s: &mut Viewer| {
                    s.fleet_ui.close();
                    s.fleet_manage_source(import_version.clone(), ManageAction::Import);
                }),
            )
            .separator()
            .item(
                MenuItem::new(if collapsed {
                    "Expand profiles"
                } else {
                    "Collapse profiles"
                })
                .on_select(move |s: &mut Viewer| {
                    let r = s.fleet_preferences.set_collapsed(key.clone(), !collapsed);
                    result(s, r);
                    s.fleet_ui.close();
                }),
            )
            .item(
                MenuItem::new("Display alias…").on_select(move |s: &mut Viewer| {
                    s.fleet_ui
                        .alias
                        .replace_text(s.fleet_preferences.group_alias(&alias.key).unwrap_or(""));
                    s.fleet_ui.group = None;
                    s.fleet_ui.alias_group = Some(alias.clone());
                }),
            )
            .item(
                MenuItem::new("Move source first").on_select(move |s: &mut Viewer| {
                    let mut order = groups(s).iter().map(|g| g.key.clone()).collect::<Vec<_>>();
                    order.retain(|key| key != &move_key);
                    order.insert(0, move_key.clone());
                    let r = s.fleet_preferences.reorder_sources(order);
                    result(s, r);
                    s.fleet_ui.close();
                }),
            )
            .item(
                MenuItem::new("Reset source order").on_select(|s: &mut Viewer| {
                    let r = s.fleet_preferences.set_source_order(vec![]);
                    result(s, r);
                    s.fleet_ui.close();
                }),
            )
            .item(
                MenuItem::new("Clear display alias").on_select(move |s: &mut Viewer| {
                    let r = s.fleet_preferences.set_alias(reset_key.clone(), None);
                    result(s, r);
                    s.fleet_ui.close();
                }),
            );
        let built = menu.build(k, layer);
        layer.push(built);
    }
    if let Some(target) = &s.fleet_ui.alias_group {
        let key = target.key.clone();
        let submit_key = key.clone();
        let bounds = rune::memo::layout("hermes-window").unwrap_or([0., 0., 1440., 900.]);
        let x = target.point.x.clamp(8., (bounds[2] - 288.).max(8.));
        let y = (target.point.y - 144.).clamp(8., (bounds[3] - 152.).max(8.));
        let close = el()
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .hit()
            .on_press(|s: &mut Viewer| s.fleet_ui.close());
        layer.push(close);
        layer.push(
            col()
                .key("fleet-source-alias")
                .absolute(x, y)
                .modal(true, |s: &mut Viewer| s.fleet_ui.close())
                .w(280.)
                .padding(12.)
                .gap(8.)
                .radius(8.)
                .fill(k.c.bg_deep)
                .border(1., k.c.border_muted)
                .child(
                    text("Display alias")
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base),
                )
                .child(
                    TextInput::new(&s.fleet_ui.alias)
                        .placeholder("Use the saved connection name")
                        .on_edit(|s: &mut Viewer| &mut s.fleet_ui.alias)
                        .on_submit(move |s: &mut Viewer, _| {
                            let value = s.fleet_ui.alias.text.clone();
                            let r = s
                                .fleet_preferences
                                .set_alias(submit_key.clone(), Some(value));
                            result(s, r);
                            s.fleet_ui.close();
                        })
                        .build(k)
                        .key("fleet-source-alias-input"),
                )
                .child(
                    Button::new("Save display alias")
                        .on_press(move |s: &mut Viewer| {
                            let value = s.fleet_ui.alias.text.clone();
                            let r = s.fleet_preferences.set_alias(key.clone(), Some(value));
                            result(s, r);
                            s.fleet_ui.close();
                        })
                        .build(k)
                        .key("fleet-source-alias-save"),
                ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        fleet::{Descriptor, ProfileMetadata, Source},
        source::SourceId,
    };
    fn version(id: &str) -> SourceVersion {
        SourceVersion {
            id: SourceId::Saved(id.into()),
            revision: "r1".into(),
        }
    }
    fn source(id: &str, label: &str, kind: Kind, order: usize, status: Status) -> Source {
        Source {
            descriptor: Descriptor {
                version: version(id),
                label: label.into(),
                kind,
                registration_order: order,
            },
            status,
            evidence: None,
            install_id: None,
            principal_stamp: None,
            profiles: vec![],
            inventory_error: None,
        }
    }
    fn agent(id: &str, name: &str) -> Agent {
        Agent {
            source: version(id),
            profile: name.into(),
            target_profile: name.into(),
            handle: format!("{name}-{id}"),
            metadata: ProfileMetadata::default(),
        }
    }
    #[test]
    fn groups_stay_in_their_slots_across_foreground_changes() {
        let snapshot = Snapshot {
            sources: vec![
                source("b", "Remote", Kind::Remote, 0, Status::Ready),
                source("a", "This device", Kind::Local, 1, Status::Ready),
            ],
            agents: vec![agent("b", "research"), agent("a", "research")],
            ..Default::default()
        };
        let prefs = crate::client::fleet::Preferences::default();
        let a = project(&snapshot, Some(&version("a")), &prefs);
        let b = project(&snapshot, Some(&version("b")), &prefs);
        assert_eq!(
            a.iter().map(|g| &g.key).collect::<Vec<_>>(),
            b.iter().map(|g| &g.key).collect::<Vec<_>>()
        );
        assert!(a[0].active);
        assert!(b[1].active);
        assert_ne!(
            choice_key(&a[1].choices[0], false),
            choice_key(&a[0].choices[0], false)
        );
        assert_eq!(choice_key(&a[0].choices[0], false), "profile-research");
    }
    #[test]
    fn thirteen_vs_fourteen_counts_all_sources_and_undialed_door_without_profile_proof() {
        let mut snapshot = Snapshot {
            sources: vec![
                source("a", "One", Kind::Remote, 0, Status::Ready),
                source("b", "Sleeping SSH", Kind::Ssh, 1, Status::ConnectOnDemand),
            ],
            agents: (0..12).map(|i| agent("a", &format!("p{i}"))).collect(),
            ..Default::default()
        };
        let prefs = crate::client::fleet::Preferences::default();
        let groups = project(&snapshot, None, &prefs);
        assert!(!condensed(&groups));
        assert!(groups[1].choices.is_empty());
        snapshot.agents.push(agent("a", "thirteenth"));
        assert!(condensed(&project(&snapshot, None, &prefs)));
    }
    #[test]
    fn group_presentation_survives_revision_change_and_shared_profile_order() {
        let mut snapshot = Snapshot {
            sources: vec![
                source("a", "One", Kind::Remote, 0, Status::Ready),
                source("b", "Two", Kind::Remote, 1, Status::Ready),
            ],
            agents: vec![
                agent("a", "alpha"),
                agent("a", "zulu"),
                agent("b", "alpha"),
                agent("b", "zulu"),
            ],
            ..Default::default()
        };
        let mut prefs = crate::client::fleet::Preferences::default();
        let key = source_key(&version("b"));
        prefs.set_alias(key.clone(), Some("Desk".into())).unwrap();
        prefs.set_collapsed(key.clone(), true).unwrap();
        prefs.set_source_order(vec![key.clone()]).unwrap();
        prefs.set_profile_order(vec!["zulu".into()]).unwrap();
        snapshot.sources[1].descriptor.version.revision = "r2".into();
        for a in &mut snapshot.agents {
            if a.source.id == SourceId::Saved("b".into()) {
                a.source.revision = "r2".into();
            }
        }
        let groups = project(&snapshot, None, &prefs);
        assert_eq!(groups[0].label, "Desk");
        assert!(prefs.collapsed(&groups[0].key));
        assert_eq!(groups[0].choices[0].profile, "zulu");
        assert_eq!(groups[1].choices[0].profile, "zulu");
    }
    #[test]
    fn proven_alias_uses_one_display_slot_but_keeps_actual_action_owner_and_active_keys() {
        let mut saved = source("a", "Saved primary", Kind::Remote, 0, Status::Ready);
        let mut current = source("direct", "Connected", Kind::Remote, 1, Status::Ready);
        let actual = SourceVersion {
            id: SourceId::Direct("opaque-current".into()),
            revision: "principal".into(),
        };
        current.descriptor.version = actual.clone();
        let names = (0..13).map(|i| format!("p{i}")).collect::<Vec<_>>();
        for source in [&mut saved, &mut current] {
            source.install_id = Some("same-install".into());
            source.profiles = names
                .iter()
                .map(|name| crate::fleet::RosterProfile {
                    name: name.clone(),
                    metadata: ProfileMetadata::default(),
                    canonical_session: None,
                })
                .collect();
        }
        let snapshot = Snapshot {
            sources: vec![saved, current],
            agents: names.iter().map(|n| agent("a", n)).collect(),
            ..Default::default()
        };
        let prefs = crate::client::fleet::Preferences::default();
        let mut groups = project(&snapshot, Some(&actual), &prefs);
        let choices = names
            .iter()
            .map(|name| Choice {
                version: Some(actual.clone()),
                profile: name.clone(),
                label: name.clone(),
                handle: name.clone(),
                active_source: true,
                activity: None,
            })
            .collect();
        merge_current(&mut groups, &snapshot, Some(&actual), choices, &prefs);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].version, Some(version("a")));
        assert!(!condensed(&groups));
        for choice in &groups[0].choices {
            assert_eq!(choice.version, Some(actual.clone()));
            assert!(choice.active_source);
            assert_eq!(
                choice_key(choice, false),
                format!("profile-{}", choice.profile)
            );
        }
    }
    #[test]
    fn exact_current_alias_composition_has_thirteen_real_choices_and_no_phantom_source_door() {
        let actual = SourceVersion {
            id: SourceId::Direct("raw-a-principal".into()),
            revision: "raw-a-principal".into(),
        };
        let a_names = ["default", "research", "archive", "a1", "a2", "a3", "a4"];
        let b_names = ["default", "research", "archive", "b1", "b2", "b3"];
        let mut a = source("a", "Saved A", Kind::Remote, 0, Status::Ready);
        let mut b = source("b", "Saved B", Kind::Remote, 1, Status::Ready);
        let mut raw = source("raw", "Raw A", Kind::Remote, 2, Status::Ready);
        raw.descriptor.version = actual.clone();
        a.install_id = Some("proved-a".into());
        raw.install_id = a.install_id.clone();
        b.install_id = Some("proved-b".into());
        let rows = |names: &[&str]| {
            names
                .iter()
                .map(|name| crate::fleet::RosterProfile {
                    name: (*name).into(),
                    metadata: ProfileMetadata::default(),
                    canonical_session: None,
                })
                .collect::<Vec<_>>()
        };
        a.profiles = rows(&a_names);
        raw.profiles = rows(&a_names);
        b.profiles = rows(&b_names);
        let profiles = raw
            .profiles
            .iter()
            .map(crate::fleet::RosterProfile::gateway_profile)
            .collect::<Vec<_>>();
        let snapshot = Snapshot {
            sources: vec![a, b, raw],
            agents: a_names
                .iter()
                .map(|name| agent("a", name))
                .chain(b_names.iter().map(|name| agent("b", name)))
                .collect(),
            ..Default::default()
        };
        let prefs = crate::client::fleet::Preferences::default();
        let groups = project_current(
            &snapshot,
            Some(&actual),
            &profiles,
            "Raw A",
            Kind::Remote,
            &prefs,
        );
        assert_eq!(
            groups.len(),
            2,
            "an exact active version must enter alias composition"
        );
        assert!(
            !condensed(&groups),
            "thirteen physical choices must stay in the rail"
        );
        assert_eq!(
            groups
                .iter()
                .map(|group| group.choices.len())
                .sum::<usize>(),
            13
        );
        for choice in &groups
            .iter()
            .find(|group| group.version == Some(version("a")))
            .unwrap()
            .choices
        {
            assert_eq!(choice.version, Some(actual.clone()));
            assert!(choice.active_source);
            assert_eq!(
                choice_key(choice, false),
                format!("profile-{}", choice.profile)
            );
        }
    }
    #[test]
    fn changed_saved_revision_never_overlays_stale_current_profiles_or_marks_them_active() {
        let actual = version("a");
        let mut changed = source("a", "Changed source", Kind::Remote, 0, Status::Ready);
        changed.descriptor.version.revision = "r2".into();
        let mut current = agent("a", "served-new");
        current.source = changed.descriptor.version.clone();
        let snapshot = Snapshot {
            sources: vec![changed],
            agents: vec![current],
            ..Default::default()
        };
        let profiles = vec![crate::gateway::Profile {
            name: "stale-old".into(),
            label: "Old profile".into(),
            description: String::new(),
            canonical_session: None,
        }];
        let groups = project_current(
            &snapshot,
            Some(&actual),
            &profiles,
            "Old source",
            Kind::Remote,
            &crate::client::fleet::Preferences::default(),
        );
        assert_eq!(groups.len(), 1);
        assert!(!groups[0].active);
        assert_eq!(groups[0].choices.len(), 1);
        assert_eq!(groups[0].choices[0].profile, "served-new");
        assert!(!groups[0].choices[0].active_source);
        assert_eq!(
            groups[0].choices[0].version.as_ref().unwrap().revision,
            "r2"
        );
    }
    #[test]
    fn partial_unknown_and_unavailable_aliases_never_become_invented_ready_sources() {
        let mut saved = source("a", "Saved", Kind::Remote, 0, Status::Ready);
        let mut other = source(
            "b",
            "Other",
            Kind::Remote,
            1,
            Status::Unavailable("Offline".into()),
        );
        for source in [&mut saved, &mut other] {
            source.install_id = Some("same".into());
            source.profiles = vec![crate::fleet::RosterProfile {
                name: "shared".into(),
                metadata: ProfileMetadata::default(),
                canonical_session: None,
            }];
        }
        let mut snapshot = Snapshot {
            sources: vec![saved, other],
            agents: vec![agent("a", "shared"), agent("b", "unique")],
            ..Default::default()
        };
        snapshot.sources[1]
            .profiles
            .push(crate::fleet::RosterProfile {
                name: "unique".into(),
                metadata: ProfileMetadata::default(),
                canonical_session: None,
            });
        let prefs = crate::client::fleet::Preferences::default();
        let mut groups = project(&snapshot, Some(&version("b")), &prefs);
        let choices = ["shared", "unique"]
            .into_iter()
            .map(|name| Choice {
                version: Some(version("b")),
                profile: name.into(),
                label: name.into(),
                handle: name.into(),
                active_source: true,
                activity: None,
            })
            .collect();
        merge_current(&mut groups, &snapshot, Some(&version("b")), choices, &prefs);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].choices[0].version, Some(version("b")));
        assert_eq!(
            groups
                .iter()
                .find(|g| g.version == Some(version("b")))
                .unwrap()
                .choices[0]
                .profile,
            "unique"
        );
        snapshot.sources[1].profiles.retain(|p| p.name == "shared");
        snapshot.agents.retain(|a| a.profile != "unique");
        let mut groups = project(&snapshot, None, &prefs);
        merge_current(&mut groups, &snapshot, None, vec![], &prefs);
        assert_eq!(
            groups.len(),
            2,
            "offline alias retains its real retry/status door"
        );
        snapshot.sources[1].install_id = None;
        assert!(snapshot.canonical_source(&version("b"), "shared").is_none());
    }
    #[test]
    fn simple_visibility_keeps_multiple_profile_door_and_advanced_respects_explicit_preference() {
        assert!(!rail_policy(true, false, false, 2, 2));
        assert!(rail_policy(true, true, false, 2, 1));
        assert!(rail_policy(true, true, false, 1, 2));
        assert!(!rail_policy(true, true, true, 1, 1));
        assert!(!rail_policy(false, true, false, 2, 2));
        assert!(rail_policy(true, false, true, 1, 1));
    }
    #[test]
    fn activity_hides_the_actual_open_profile_and_counts_one_compression_conversation() {
        assert!(!activity_visible(true, true, false));
        assert!(activity_visible(false, true, false));
        assert!(activity_visible(true, false, false));
        assert!(activity_visible(true, true, true));
        let mut state = crate::sidebar::SidebarState::default();
        let root = crate::gateway::sidebar::SidebarSession {
            id: "root".into(),
            profile: Some("same".into()),
            _lineage_root_id: Some("root".into()),
            unread: Some(true),
            last_active: 1.,
            ..Default::default()
        };
        let tip = crate::gateway::sidebar::SidebarSession {
            id: "tip".into(),
            profile: Some("same".into()),
            _lineage_root_id: Some("root".into()),
            _lineage_ids: Some(vec!["root".into(), "tip".into()]),
            unread: Some(true),
            last_active: 2.,
            ..Default::default()
        };
        state.recents.rows = vec![root];
        state.messaging.rows = vec![tip];
        assert_eq!(
            activity_summary(&state, "same").as_deref(),
            Some("1 unread")
        );
        assert_eq!(activity_summary(&state, "other"), None);
    }
}
