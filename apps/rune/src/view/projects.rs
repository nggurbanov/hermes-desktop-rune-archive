//! Project navigation in the existing Sessions sidebar.
use crate::{
    app::Viewer,
    projects::{Owner, Scope},
};
use rune::{El, col, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Dialog, Kit, Menu, MenuItem, SidebarRow, TextInput,
};
use rune_theme_opencode::icons as ic;

/// The host provides owner-checked actions; rendering does not initiate I/O.
#[derive(Clone, Copy)]
pub struct Actions {
    pub select: fn(&mut Viewer, Owner, Scope, u64),
    pub retry: fn(&mut Viewer, Owner, u64),
    pub open_session: fn(&mut Viewer, Owner, Scope, u64, String),
    pub open_preview: fn(&mut Viewer, Owner, Scope, u64, String, String),
    pub toggle: fn(&mut Viewer, Owner, Scope, u64, String),
    pub expand: fn(&mut Viewer, Owner, Scope, u64, String),
    pub reveal: fn(&mut Viewer, Owner, Scope, u64, String),
}
fn button(
    label: String,
    owner: Owner,
    scope: Scope,
    selected: bool,
    generation: u64,
    actions: Actions,
    kit: &Kit,
) -> El {
    Button::new(label)
        .variant(if selected {
            ButtonVariant::Neutral
        } else {
            ButtonVariant::GhostMuted
        })
        .control_size(ControlSize::Small)
        .on_press(move |viewer: &mut Viewer| {
            (actions.select)(viewer, owner.clone(), scope.clone(), generation)
        })
        .build(kit)
}
/// The host must only render this for the current proved owner.
pub fn navigation(viewer: &Viewer, actions: Actions, kit: &Kit, layers: &mut Vec<El>) -> El {
    let state = viewer.projects_state();
    let sidebar = &viewer.sidebar_state;
    let mut body = col().w_full().gap(3.0);
    let Some(owner) = state.owner.clone() else {
        return body;
    };
    if let Some(result) = viewer.project_create_result() {
        body = body.child(crate::view::project_create::status(result, kit));
    }
    if let Some(form) = viewer.project_create_form() {
        crate::view::project_create::modal::<Viewer>(
            crate::view::project_create::Snapshot {
                target: form.target(),
                name: &form.name,
                idea: &form.idea,
                folders: &form.folders,
                error: form.error.as_deref(),
                pending: viewer.project_create_busy(),
                generating: viewer.project_idea_busy(),
                uncertain: form.uncertain,
                templates: viewer.project_create_templates(),
            },
            kit,
            layers,
            &form.focus_return,
        );
    }
    if let Some(result) = viewer.project_active_result() {
        body = body.child(rune::text(result.to_owned()));
    }
    if let Some(result) = viewer.project_folders_result() {
        body = body.child(
            text(result)
                .font(kit.ty.small)
                .color(kit.c.text_faint)
                .wrap()
                .role("alert"),
        );
    }
    if viewer.project_folders_visible() {
        let mut panel = col().gap(6.0).py(8.0).child(
            Button::new("Add folder")
                .disabled(viewer.project_folders_busy() || viewer.project_folders_error().is_some())
                .on_press(|v: &mut Viewer| v.project_folders_choose())
                .build(kit)
                .key("project-folders-choose"),
        );
        if let Some(error) = viewer.project_folders_error() {
            panel = panel.child(
                text(error)
                    .font(kit.ty.small)
                    .color(kit.c.text_faint)
                    .wrap()
                    .role("alert"),
            );
        }
        Dialog::new("project-folders", "Add folder")
            .open(true)
            .fit()
            .click_outside(false)
            .on_close(|v: &mut Viewer| v.project_folders_cancel())
            .child(panel.px(16.0).pb(16.0))
            .build(kit, layers);
    }
    if let Some(result) = viewer.project_rename_result() {
        body = body.child(
            text(result)
                .font(kit.ty.small)
                .color(kit.c.text_faint)
                .wrap()
                .role("alert"),
        );
    }
    if let Some(form) = viewer.project_rename_form() {
        let pending = viewer.project_rename_busy();
        let disabled = pending || form.name.text.trim().is_empty();
        let mut panel = col().gap(6.0).py(8.0).child(
            TextInput::new(&form.name)
                .width(None)
                .placeholder("e.g. Skunkworks")
                .disabled(pending)
                .on_submit(|v: &mut Viewer, tx| {
                    v.project_rename_submit();
                    tx.instant();
                })
                .on_key(|v: &mut Viewer, event, tx| {
                    if event.key == rune::Key::Escape {
                        v.project_rename_cancel();
                        tx.instant();
                        true
                    } else {
                        false
                    }
                })
                .on_edit(|v: &mut Viewer| v.project_rename_name_mut())
                .build(kit)
                .key("project-rename-input"),
        );
        if let Some(error) = &form.error {
            panel = panel.child(
                text(error.clone())
                    .font(kit.ty.small)
                    .color(kit.c.text_faint)
                    .wrap()
                    .role("alert"),
            );
        }
        panel = panel.child(
            row()
                .gap(6.0)
                .child(
                    Button::new("Cancel")
                        .disabled(pending)
                        .on_press(|v: &mut Viewer| v.project_rename_cancel())
                        .build(kit)
                        .key("project-rename-cancel"),
                )
                .child(
                    Button::new("Save")
                        .disabled(disabled)
                        .on_press(|v: &mut Viewer| v.project_rename_submit())
                        .build(kit)
                        .key("project-rename-submit"),
                ),
        );
        Dialog::new("project-rename", "Rename project")
            .open(true)
            .fit()
            .click_outside(false)
            .on_close(|v: &mut Viewer| v.project_rename_cancel())
            .child(panel.px(16.0).pb(16.0))
            .build(kit, layers);
        if let Some(layer) = layers.pop() {
            layers.push(layer.focus_return(project_focus_key(
                &form.id,
                &form.owner,
                form.generation,
            )));
        }
    }
    body = body.child(button(
        "All projects".into(),
        owner.clone(),
        Scope::All,
        state.scope == Scope::All,
        state.generation(),
        actions,
        kit,
    ));
    let overview =
        state.scope == Scope::All && sidebar.prefs.grouping == crate::sidebar::Grouping::Project;
    if !overview
        || state
            .tree
            .as_ref()
            .is_none_or(|tree| !tree.projects.iter().any(|p| p.is_no_project))
    {
        body = body.child(button(
            "Home".into(),
            owner.clone(),
            Scope::Home,
            state.scope == Scope::Home,
            state.generation(),
            actions,
            kit,
        ));
    }
    if state.loading {
        body = body.child(
            text("Loading projects…")
                .font(kit.ty.small)
                .color(kit.c.text_faint),
        );
    }
    if let Some(error) = &state.error {
        body = body.child(
            text(error.clone())
                .font(kit.ty.small)
                .color(kit.c.text_faint)
                .wrap(),
        );
        let retry_owner = owner.clone();
        let generation = state.generation();
        body = body.child(
            Button::new("Retry")
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .on_press(move |v: &mut Viewer| (actions.retry)(v, retry_owner.clone(), generation))
                .build(kit),
        );
    }
    if let Some(tree) = &state.tree {
        let overview = state.scope == Scope::All
            && sidebar.prefs.grouping == crate::sidebar::Grouping::Project;
        let pinned: std::collections::HashSet<String> = sidebar
            .all_rows()
            .into_iter()
            .filter(|row| {
                row.pinned == Some(true)
                    && row.provenance.as_ref() == Some(&owner.session.source)
                    && row.profile() == owner.session.profile
            })
            .flat_map(|row| {
                row.aliases()
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut projects: Vec<_> = tree.projects.iter().collect();
        projects.sort_by(|a, b| {
            b.is_no_project
                .cmp(&a.is_no_project)
                .then_with(|| {
                    (tree.active_id.as_deref() == Some(b.id.as_str()) && !b.is_auto)
                        .cmp(&(tree.active_id.as_deref() == Some(a.id.as_str()) && !a.is_auto))
                })
                .then_with(|| a.is_auto.cmp(&b.is_auto))
                .then_with(|| (b.session_count > 0).cmp(&(a.session_count > 0)))
                .then_with(|| {
                    b.last_active
                        .unwrap_or_default()
                        .total_cmp(&a.last_active.unwrap_or_default())
                })
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        });
        for project in projects {
            if state.scope != Scope::All {
                continue;
            }
            if project.is_no_project && !overview {
                continue;
            }
            if overview && project.is_auto && !project.is_no_project && project.session_count == 0 {
                continue;
            }
            if overview
                && !sidebar.prefs.projects.is_empty()
                && tree
                    .projects
                    .iter()
                    .any(|p| sidebar.prefs.projects.contains(&p.id))
                && !sidebar.prefs.projects.contains(&project.id)
            {
                continue;
            }
            let target = if project.is_no_project {
                Scope::Home
            } else {
                Scope::Project(project.id.clone())
            };
            let key = format!("project-disclosure:{}", owner.session.row_key(&project.id));
            let mut candidates: Vec<_> = project
                .preview_sessions
                .iter()
                .take(
                    if state.show_all_sessions || state.expanded.contains_key(&project.id) {
                        usize::MAX
                    } else {
                        3
                    },
                )
                .collect();
            if let Some(expanded) = state.expanded.get(&project.id) {
                let mut more: Vec<_> = expanded
                    .repos
                    .iter()
                    .flat_map(|r| &r.groups)
                    .flat_map(|g| &g.sessions)
                    .collect();
                more.sort_by(|a, b| {
                    let time = |v: &serde_json::Value| {
                        v.get("last_active")
                            .and_then(|v| v.as_f64())
                            .filter(|n| *n > 0.0)
                            .or_else(|| v.get("started_at").and_then(|v| v.as_f64()))
                            .unwrap_or_default()
                    };
                    time(b).total_cmp(&time(a))
                });
                let mut seen: std::collections::HashSet<_> = candidates
                    .iter()
                    .filter_map(|v| v.get("id").and_then(|v| v.as_str()))
                    .collect();
                for session in more {
                    if session
                        .get("id")
                        .and_then(|v| v.as_str())
                        .is_some_and(|id| seen.insert(id))
                    {
                        candidates.push(session);
                    }
                }
            }
            let candidate_ids: std::collections::HashSet<_> = candidates
                .iter()
                .filter_map(|row| row.get("id").and_then(|v| v.as_str()))
                .collect();
            let candidate_count = candidates.len();
            let pinned_elsewhere = project
                .session_ids
                .iter()
                .filter(|id| pinned.contains(id.as_str()) && !candidate_ids.contains(id.as_str()))
                .count();
            let rows: Vec<_> = candidates
                .into_iter()
                .filter(|session| {
                    session
                        .get("id")
                        .and_then(|v| v.as_str())
                        .is_some_and(|id| !id.is_empty())
                        && session
                            .get("profile")
                            .and_then(|v| v.as_str())
                            .is_none_or(|p| p == owner.session.profile)
                        && !session
                            .get("hidden")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        && !session
                            .get("is_internal_child")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        && !pinned.contains(
                            session
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default(),
                        )
                        && !pinned.contains(
                            session
                                .get("_lineage_root_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default(),
                        )
                        && !session
                            .get("pinned")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        && session.get("title").and_then(|v| v.as_str()).map(str::trim)
                            != Some("Bot Chat")
                        && {
                            let mut item = crate::gateway::sidebar::SidebarSession::default();
                            item.id = session["id"].as_str().unwrap_or_default().to_owned();
                            item.profile = Some(owner.session.profile.clone());
                            item.title = session
                                .get("title")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned);
                            item.preview = session
                                .get("preview")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned);
                            item.source = session
                                .get("source")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned);
                            item.cwd = session
                                .get("cwd")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned);
                            item.git_repo_root = session
                                .get("git_repo_root")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned);
                            item.provenance = Some(owner.session.source.clone());
                            item.unread = session.get("unread").and_then(|v| v.as_bool());
                            item.message_count = session
                                .get("message_count")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            item.archived = session
                                .get("archived")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            let query = viewer.search.text.trim().to_lowercase();
                            (query.is_empty()
                                || format!(
                                    "{} {} {} {}",
                                    item.title(),
                                    item.preview.as_deref().unwrap_or_default(),
                                    item.profile(),
                                    crate::sidebar::source_label(
                                        item.source.as_deref().unwrap_or_default()
                                    )
                                )
                                .to_lowercase()
                                .contains(&query))
                                && (!item.archived || sidebar.prefs.archived)
                                && (sidebar.prefs.status.is_empty()
                                    || sidebar.prefs.status.contains(sidebar.status(&item).label()))
                                && (sidebar.prefs.profiles.is_empty()
                                    || sidebar.prefs.profiles.contains(item.profile()))
                        }
                })
                .collect();
            let hidden_count = candidate_count.saturating_sub(rows.len()) + pinned_elsewhere;
            let visible_total = project.session_count.saturating_sub(hidden_count as u64);
            let mut header = row()
                .key(format!("project-header-{}", project.id))
                .w_full()
                .center_y()
                .hover_group();
            if overview && !rows.is_empty() {
                let toggle_owner = owner.clone();
                let toggle_scope = state.scope.clone();
                let toggle_key = key.clone();
                let generation = state.generation();
                header = header.child(
                    Button::new("")
                        .icon(if sidebar.prefs.collapsed.contains(&key) {
                            &ic::CHEVRON_RIGHT
                        } else {
                            &ic::CHEVRON_DOWN
                        })
                        .variant(ButtonVariant::GhostMuted)
                        .control_size(ControlSize::Small)
                        .on_press(move |v: &mut Viewer| {
                            (actions.toggle)(
                                v,
                                toggle_owner.clone(),
                                toggle_scope.clone(),
                                generation,
                                toggle_key.clone(),
                            )
                        })
                        .build(kit),
                );
            }
            header = header.child(
                button(
                    project.label.clone(),
                    owner.clone(),
                    target.clone(),
                    state.scope == target,
                    state.generation(),
                    actions,
                    kit,
                )
                .grow(1.0),
            );
            if state
                .tree
                .as_ref()
                .is_some_and(|t| t.active_id.as_deref() == Some(project.id.as_str()))
                && !project.is_auto
            {
                header = header.child(text("Active").font(kit.ty.small).color(kit.c.text_faint));
            }
            if !project.is_no_project && !project.is_auto {
                header = header.child(project_menu(
                    viewer,
                    &owner,
                    &state.scope,
                    state.generation(),
                    &project.id,
                    kit,
                    layers,
                ));
            }
            if !project.is_no_project && !project.is_auto {
                let menu_owner = owner.clone();
                let menu_scope = state.scope.clone();
                let menu_generation = state.generation();
                let menu_id = project.id.clone();
                header = header.on_secondary_press(move |v: &mut Viewer, point, _| {
                    v.project_rename_menu_open(
                        menu_owner.clone(),
                        menu_scope.clone(),
                        menu_generation,
                        menu_id.clone(),
                        Some((point.x, point.y)),
                    );
                });
            }
            body = body.child(header);
            if overview && !sidebar.prefs.collapsed.contains(&key) {
                let limit = if state.expanded.contains_key(&project.id) {
                    *state.revealed.get(&project.id).unwrap_or(&50)
                } else {
                    usize::MAX
                };
                for session in rows.iter().take(limit) {
                    let id = session["id"].as_str().unwrap().to_owned();
                    let title = session
                        .get("title")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or("Untitled session")
                        .to_owned();
                    let session_owner = owner.clone();
                    let scope = state.scope.clone();
                    let project_id = project.id.clone();
                    let generation = state.generation();
                    body = body.child(
                        SidebarRow::new(title)
                            .depth(1)
                            .selected(
                                viewer.selected_session.as_deref() == Some(id.as_str())
                                    && viewer.profile == owner.session.profile,
                            )
                            .on_select({
                                let id = id.clone();
                                let project_id = project_id.clone();
                                move |v: &mut Viewer, _| {
                                    (actions.open_preview)(
                                        v,
                                        session_owner.clone(),
                                        scope.clone(),
                                        generation,
                                        project_id.clone(),
                                        id.clone(),
                                    )
                                }
                            })
                            .build(kit)
                            .key(format!("project-preview-{}-{}", project.id, id)),
                    );
                }
                if state.expanded.contains_key(&project.id) && rows.len() > limit {
                    let owner = owner.clone();
                    let scope = state.scope.clone();
                    let generation = state.generation();
                    let id = project.id.clone();
                    body = body.child(
                        Button::new(format!(
                            "Show {} more in {}",
                            (rows.len() - limit).min(50),
                            project.label
                        ))
                        .variant(ButtonVariant::GhostMuted)
                        .control_size(ControlSize::Small)
                        .on_press(move |v: &mut Viewer| {
                            (actions.reveal)(
                                v,
                                owner.clone(),
                                scope.clone(),
                                generation,
                                id.clone(),
                            )
                        })
                        .build(kit),
                    );
                } else if !state.show_all_sessions
                    && !state.expanded.contains_key(&project.id)
                    && !rows.is_empty()
                    && visible_total > rows.len() as u64
                {
                    let owner = owner.clone();
                    let scope = state.scope.clone();
                    let generation = state.generation();
                    let id = project.id.clone();
                    body = body.child(
                        Button::new(format!("Show all {} sessions", visible_total))
                            .variant(ButtonVariant::GhostMuted)
                            .control_size(ControlSize::Small)
                            .disabled(state.loading)
                            .on_press(move |v: &mut Viewer| {
                                (actions.expand)(
                                    v,
                                    owner.clone(),
                                    scope.clone(),
                                    generation,
                                    id.clone(),
                                )
                            })
                            .build(kit)
                            .key(format!("project-expand-{}", project.id)),
                    );
                }
            }
        }
    }
    if let Some(project) = &state.entered {
        let selected = match &state.scope {
            Scope::Project(id) => &project.id == id,
            Scope::Home => project.is_no_project,
            Scope::All => false,
        };
        if selected {
            if !project.is_no_project && !project.is_auto {
                let menu_owner = owner.clone();
                let menu_scope = state.scope.clone();
                let menu_generation = state.generation();
                let menu_id = project.id.clone();
                body = body.child(
                    row()
                        .key(format!("project-header-{}", project.id))
                        .w_full()
                        .center_y()
                        .hover_group()
                        .on_secondary_press(move |v: &mut Viewer, point, _| {
                            v.project_rename_menu_open(
                                menu_owner.clone(),
                                menu_scope.clone(),
                                menu_generation,
                                menu_id.clone(),
                                Some((point.x, point.y)),
                            )
                        })
                        .child(text(project.label.clone()).grow(1.0))
                        .child(project_menu(
                            viewer,
                            &owner,
                            &state.scope,
                            state.generation(),
                            &project.id,
                            kit,
                            layers,
                        )),
                );
            }
            for repo in &project.repos {
                body = body.child(
                    text(repo.label.clone())
                        .font(kit.ty.small)
                        .color(kit.c.text_faint),
                );
                for lane in &repo.groups {
                    body = body.child(
                        text(lane.label.clone())
                            .font(kit.ty.small)
                            .color(kit.c.text_faint),
                    );
                    for session in &lane.sessions {
                        if session
                            .get("profile")
                            .and_then(|v| v.as_str())
                            .is_some_and(|profile| profile != owner.session.profile)
                        {
                            continue;
                        }
                        let Some(id) = session.get("id").and_then(|v| v.as_str()) else {
                            continue;
                        };
                        let id = id.to_owned();
                        let title = session
                            .get("title")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or("Untitled session")
                            .to_owned();
                        let session_owner = owner.clone();
                        let session_scope = state.scope.clone();
                        let generation = state.generation();
                        body = body.child(
                            Button::new(title)
                                .variant(ButtonVariant::GhostMuted)
                                .control_size(ControlSize::Small)
                                .on_press(move |v: &mut Viewer| {
                                    (actions.open_session)(
                                        v,
                                        session_owner.clone(),
                                        session_scope.clone(),
                                        generation,
                                        id.clone(),
                                    )
                                })
                                .build(kit),
                        );
                    }
                }
            }
        }
    }
    body
}

fn project_focus_key(id: &str, owner: &Owner, generation: u64) -> String {
    format!("project-focus-{id}-{}-{generation}", owner.epoch)
}

fn project_menu(
    viewer: &Viewer,
    owner: &Owner,
    scope: &Scope,
    generation: u64,
    id: &str,
    kit: &Kit,
    layers: &mut Vec<El>,
) -> El {
    let key = format!("project-actions-{id}");
    let opened = viewer.project_rename_menu().filter(|m| m.id == id);
    let captured = opened.cloned();
    let trigger_owner = owner.clone();
    let trigger_scope = scope.clone();
    let trigger_id = id.to_owned();
    let trigger = Button::new("⋮")
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .on_press(move |v: &mut Viewer| {
            v.project_rename_menu_open(
                trigger_owner.clone(),
                trigger_scope.clone(),
                generation,
                trigger_id.clone(),
                None,
            )
        })
        .build(kit)
        .key(key.clone())
        .label("Actions")
        .group_opacity(if opened.is_some() { 1.0 } else { 0.001 }, 1.0);
    let mut menu = Menu::new(key)
        .trigger(trigger)
        .open(opened.is_some())
        .on_close(|v: &mut Viewer| v.project_rename_menu_close())
        .item(MenuItem::new("Rename…").on_select(move |v: &mut Viewer| {
            v.project_rename_menu_close();
            if let Some(m) = &captured {
                v.project_rename_open(m.owner.clone(), m.scope.clone(), m.generation, m.id.clone());
            }
        }));
    {
        let captured = opened.cloned();
        menu = menu.item(
            MenuItem::new("Add folder").on_select(move |v: &mut Viewer| {
                v.project_rename_menu_close();
                if let Some(m) = &captured {
                    v.project_folders_open(
                        m.owner.clone(),
                        m.scope.clone(),
                        m.generation,
                        m.id.clone(),
                    );
                }
            }),
        );
    }
    {
        let captured = opened.cloned();
        let active = viewer
            .projects_state()
            .tree
            .as_ref()
            .is_some_and(|t| t.active_id.as_deref() == Some(id));
        menu = menu.item(
            MenuItem::new(if active { "Clear active" } else { "Set active" })
                .checked(active)
                .disabled(viewer.project_active_busy())
                .on_select(move |v: &mut Viewer| {
                    v.project_rename_menu_close();
                    if let Some(m) = &captured {
                        let still_active = v
                            .projects_state()
                            .tree
                            .as_ref()
                            .is_some_and(|t| t.active_id.as_deref() == Some(m.id.as_str()));
                        if still_active != active {
                            return;
                        }
                        v.project_set_active(
                            m.owner.clone(),
                            m.scope.clone(),
                            m.generation,
                            if active { None } else { Some(m.id.clone()) },
                            m.binding.clone(),
                        );
                    }
                }),
        );
    }
    if let Some((x, y)) = opened.and_then(|m| m.point) {
        menu = menu.at(x, y);
    }
    row()
        .key(project_focus_key(id, owner, generation))
        .child(menu.build(kit, layers))
}

#[cfg(test)]
pub(crate) fn sidebar_test_view(s: &Viewer, _: &mut rune::Cx<Viewer>) -> El {
    let kit = rune_theme_opencode::kit(true);
    let mut layers = Vec::new();
    let sidebar = super::sidebar::sessions(s, super::palette::pal(true), &kit, &mut layers);
    col().size(700., 700.).child(sidebar).child(
        rune::el()
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .children(layers),
    )
}
