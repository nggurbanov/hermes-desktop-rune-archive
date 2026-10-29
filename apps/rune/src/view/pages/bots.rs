//! Native Hermes bot/profile manager; identities and their SOUL remain primary.
mod files;
use crate::{
    app::Viewer,
    pages::bots::{Action, Bots, string},
};
use rune::{El, ScrollSpec, TextEdit, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, Switch, TextArea, TextInput};
pub(super) fn owns(s: &Viewer, owner: &crate::composer::DraftScope, epoch: u64) -> bool {
    let now = s.composer_scope();
    now.connection == owner.connection
        && now.profile == owner.profile
        && s.bots.editor_epoch == epoch
}
pub(super) fn action(s: &Viewer, label: &str, a: Action, disabled: bool, k: &Kit) -> El {
    let owner = s.composer_scope();
    let epoch = s.bots.editor_epoch;
    let preview = matches!(
        a,
        Action::Load
            | Action::Select(_)
            | Action::New
            | Action::Discard
            | Action::Tab(_)
            | Action::CancelDialog
            | Action::CancelModel
    );
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled || s.bots.demo && !preview)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if owns(s, &owner, epoch) {
                s.bots_action(a.clone());
                tx.instant();
            }
        })
        .build(k)
}
fn input(
    s: &Viewer,
    label: &str,
    value: &TextEdit,
    access: impl Fn(&mut Bots) -> &mut TextEdit + 'static,
    k: &Kit,
) -> El {
    let owner = s.composer_scope();
    let epoch = s.bots.editor_epoch;
    col()
        .w_full()
        .gap(5.0)
        .child(text(label).font(k.ty.small_medium).color(k.c.text_muted))
        .child(
            TextInput::new(value)
                .width(None)
                .disabled(
                    s.bots.busy
                        || s.bots.normalizing
                        || s.bots.demo
                        || s.bots.confirm_model.is_some(),
                )
                .on_edit(move |s: &mut Viewer| {
                    if !owns(s, &owner, epoch)
                        || s.bots.busy
                        || s.bots.normalizing
                        || s.bots.confirm_model.is_some()
                    {
                        return s.bots.edit_detached();
                    }
                    access(&mut s.bots)
                })
                .build(k),
        )
}
fn area(
    s: &Viewer,
    label: &str,
    value: &TextEdit,
    access: impl Fn(&mut Bots) -> &mut TextEdit + 'static,
    k: &Kit,
) -> El {
    let owner = s.composer_scope();
    let epoch = s.bots.editor_epoch;
    col()
        .w_full()
        .gap(6.0)
        .child(text(label).font(k.ty.small_medium).color(k.c.text_muted))
        .child(
            TextArea::new(value)
                .width(None)
                .rows(14)
                .max_height(450.0)
                .disabled(s.bots.busy || s.bots.demo)
                .on_edit(move |s: &mut Viewer| {
                    if !owns(s, &owner, epoch) || s.bots.busy {
                        return s.bots.edit_detached();
                    }
                    access(&mut s.bots)
                })
                .build(k),
        )
}
fn heading(label: &str, k: &Kit) -> El {
    text(label)
        .font(k.ty.body_medium)
        .color(k.c.text_base)
        .pt(12.0)
}
fn body_text(label: &str, k: &Kit) -> El {
    text(label)
        .font(k.ty.small)
        .color(k.c.text_muted)
        .wrap()
        .max_w(760.0)
}
fn label(row: &serde_json::Value) -> String {
    let name = string(row, "name");
    crate::gateway::profile_label(row, row["ui_meta"].get("hermes-bots"), name)
}
pub fn page(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let b = &s.bots;
    let mut roster = col()
        .w(240.0)
        .min_w(190.0)
        .h_full()
        .min_h(0.0)
        .px(14.0)
        .py(18.0)
        .gap(10.0)
        .child(
            text("Bots & profiles")
                .font(k.ty.body_medium)
                .color(k.c.text_base),
        )
        .child(
            TextInput::new(&b.search)
                .placeholder("Search identities…")
                .width(None)
                .on_edit(|s: &mut Viewer| &mut s.bots.search)
                .build(k),
        );
    let mut list = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(3.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    let query = b.search.text.to_lowercase();
    for bot in &b.roster {
        let name = string(bot, "name");
        let title = label(bot);
        if !query.is_empty()
            && !format!(
                "{title} {name} {} {}",
                string(bot, "description"),
                string(bot, "model")
            )
            .to_lowercase()
            .contains(&query)
        {
            continue;
        }
        let selected = b.selected.as_deref() == Some(name) && !b.creating;
        list = list.child(
            action(
                s,
                &title,
                Action::Select(name.into()),
                b.busy || b.normalizing,
                k,
            )
            .w_full()
            .justify_start()
            .fill(if selected {
                k.c.layer_01
            } else {
                rune::Color::TRANSPARENT
            })
            .key(format!("bot-manager-row-{name}")),
        );
        list = list.child(
            text(format!(
                "@{}{}",
                name,
                if name == s.profile { " · active" } else { "" }
            ))
            .font(k.ty.small)
            .color(k.c.text_faint)
            .px(12.0)
            .pb(7.0),
        );
    }
    roster = roster
        .child(list)
        .child(action(s, "New bot", Action::New, b.busy || b.dirty(), k).w_full())
        .child(action(s, "Import profile", Action::Import, b.busy || b.dirty(), k).w_full());
    let mut content = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .gap(12.0)
        .px(28.0)
        .py(18.0);
    let title = if b.creating {
        "New bot".into()
    } else {
        b.selected_row()
            .map(label)
            .unwrap_or_else(|| "Choose a bot".into())
    };
    let mut toolbar = row()
        .key("bots-toolbar")
        .w_full()
        .flex_wrap()
        .gap(8.0)
        .center_y()
        .child(
            text(title)
                .font(k.ty.body_medium)
                .color(k.c.text_base)
                .min_w(100.0)
                .wrap_anywhere()
                .grow(1.0),
        )
        .child(action(
            s,
            "Refresh roster",
            Action::Load,
            b.loading || b.busy,
            k,
        ));
    if b.selected.is_some() || b.creating {
        toolbar = toolbar
            .child(action(
                s,
                if b.busy {
                    "Saving…"
                } else if b.creating {
                    "Create bot"
                } else {
                    "Save changes"
                },
                Action::Save,
                b.busy
                    || b.normalizing
                    || b.detail_loading
                    || !b.dirty()
                    || b.confirm_model.is_some(),
                k,
            ))
            .child(action(
                s,
                if b.creating {
                    "Cancel new bot"
                } else {
                    "Revert changes"
                },
                Action::Discard,
                b.busy || !b.dirty(),
                k,
            ));
    }
    content = content.child(toolbar);
    if b.demo {
        content = content.child(body_text(
            "Demonstration data · live writes are disabled",
            k,
        ));
    }
    if let Some(error) = &b.error {
        content = content.child(
            text(error)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .role("alert")
                .wrap(),
        );
    }
    if let Some(notice) = &b.notice {
        content = content.child(body_text(notice, k));
    }
    if b.awaiting_readiness() && !b.busy {
        content = content.child(action(
            s,
            "Check new bot readiness",
            Action::CheckReady,
            b.dirty() || b.confirm_model.is_some(),
            k,
        ));
    }
    if b.confirm_model.is_some() {
        content = content.child(
            col()
                .w_full()
                .gap(8.0)
                .px(12.0)
                .py(12.0)
                .fill(k.c.layer_01)
                .child(
                    text(&b.confirm_message)
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .wrap(),
                )
                .child(
                    row()
                        .w_full()
                        .flex_wrap()
                        .gap(8.0)
                        .child(action(
                            s,
                            "Confirm model selection",
                            Action::ConfirmModel,
                            b.busy,
                            k,
                        ))
                        .child(action(
                            s,
                            "Keep model draft",
                            Action::CancelModel,
                            b.busy,
                            k,
                        )),
                ),
        );
    }
    let mut tabs = row().key("bots-section-tabs").w_full().flex_wrap().gap(5.0);
    for (id, title) in [
        ("identity", "Identity"),
        ("model", "Model"),
        ("soul", "SOUL"),
        ("capabilities", "Capabilities"),
        ("appearance", "Appearance"),
        ("manage", "Manage profile"),
    ] {
        tabs = tabs.child(
            action(s, title, Action::Tab(id.into()), b.busy, k)
                .key(format!("bot-tab-{id}"))
                .fill(if b.tab == id {
                    k.c.layer_01
                } else {
                    rune::Color::TRANSPARENT
                }),
        );
    }
    content = content.child(tabs);
    let mut body = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(12.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    if b.selected.is_none() && !b.creating {
        body=body.child(body_text(if b.loading{"Loading bot identities…"}else{"Create a bot or select an existing agent profile. Each bot has its own canonical Bot Chat, persona and capabilities."},k));
    } else {
        body=body.children(match b.tab.as_str(){"model"=>model(s,k,layer),"soul"=>vec![heading("SOUL.md",k),body_text("The agent owns this persona. It applies to new context; existing conversations keep their cached prompt.",k),area(s,"Persona and instructions",&b.soul,|b|&mut b.soul,k)],"capabilities"=>capabilities(s,k),"appearance"=>appearance(s,k,layer),"manage"=>manage(s,k),_=>identity(s,k,layer)});
    }
    content = content.child(body);
    if let Some(dialog) = &b.dialog {
        content = content.child(dialog_view(s, dialog, k));
    }
    row()
        .w_full()
        .h_full()
        .min_h(0.0)
        .key("bots-manager")
        .child(roster)
        .child(content)
}
fn identity(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    let b = &s.bots;
    let mut out = vec![
        heading("Identity", k),
        body_text(
            "A bot is a named Hermes agent profile. Its conversations, memory and credentials stay with the connected agent.",
            k,
        ),
    ];
    if b.creating {
        out.push(input(s, "Profile ID", &b.name, |b| &mut b.name, k));
        out.push(body_text(
            "Lowercase letters, numbers, hyphens and underscores; up to 64 characters.",
            k,
        ));
    } else {
        out.push(body_text(
            &format!("Profile ID: {}", b.selected.as_deref().unwrap_or("")),
            k,
        ));
    }
    out.push(input(s, "Display title", &b.title, |b| &mut b.title, k));
    out.push(input(
        s,
        "Description / mission",
        &b.description,
        |b| &mut b.description,
        k,
    ));
    if b.creating {
        let mut options = vec!["Fresh profile".into()];
        options.extend(
            b.roster
                .iter()
                .filter_map(|r| r["name"].as_str().map(str::to_owned)),
        );
        let owner = s.composer_scope();
        let epoch = b.editor_epoch;
        out.push(
            Select::new("new-bot-clone", &options)
                .current(Some(b.clone_from.as_deref().unwrap_or("Fresh profile")))
                .width(300.0)
                .open(b.open.as_deref() == Some("clone"))
                .disabled(b.busy || b.demo)
                .on_toggle(|s: &mut Viewer| {
                    s.bots.open = if s.bots.open.as_deref() == Some("clone") {
                        None
                    } else {
                        Some("clone".into())
                    }
                })
                .on_close(|s: &mut Viewer| s.bots.open = None)
                .on_choose(move |s: &mut Viewer, value, tx| {
                    if owns(s, &owner, epoch) {
                        s.bots.open = None;
                        s.bots_action(Action::CloneFrom(if value == "Fresh profile" {
                            None
                        } else {
                            Some(value.into())
                        }));
                        tx.instant();
                    }
                })
                .build(k, layer),
        );
        for (label, current, field) in [
            (
                "Share authentication with the source profile",
                b.share_auth,
                0,
            ),
            ("Create without copied skills", b.no_skills, 1),
            ("Clone all supported profile files", b.clone_all, 2),
        ] {
            let owner = s.composer_scope();
            let epoch = b.editor_epoch;
            out.push(
                row()
                    .w_full()
                    .flex_wrap()
                    .gap(10.0)
                    .center_y()
                    .child(
                        Switch::new(current)
                            .disabled(b.busy || b.demo)
                            .on_toggle(move |s: &mut Viewer| {
                                if owns(s, &owner, epoch) {
                                    match field {
                                        0 => s.bots.share_auth = !current,
                                        1 => s.bots.no_skills = !current,
                                        _ => s.bots.clone_all = !current,
                                    }
                                }
                            })
                            .build(k),
                    )
                    .child(
                        text(label)
                            .font(k.ty.ui)
                            .color(k.c.text_base)
                            .wrap()
                            .min_w(0.0),
                    ),
            );
        }
        out.push(body_text("The profile is created when you press Create bot. Model, SOUL and capability choices can be staged before it exists.",k));
    } else if let Some(name) = &b.selected {
        out.push(
            row()
                .w_full()
                .flex_wrap()
                .gap(8.0)
                .child(action(
                    s,
                    "Open Bot Chat",
                    Action::OpenChat(name.clone()),
                    b.busy || b.dirty(),
                    k,
                ))
                .child(action(
                    s,
                    "Edit installed capabilities",
                    Action::OpenCapabilities(name.clone()),
                    b.busy,
                    k,
                )),
        );
    }
    out
}
fn model(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    let b = &s.bots;
    let mut out = vec![
        heading("Profile model", k),
        body_text(
            "Leave both provider and model blank to inherit. A changed model may require the agent's explicit cost or data-policy confirmation.",
            k,
        ),
    ];
    let options = b
        .catalog
        .iter()
        .filter_map(|p| p["slug"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let owner = s.composer_scope();
    let epoch = b.editor_epoch;
    out.push(
        Select::new("bot-model-provider", &options)
            .current(Some(&b.provider.text))
            .width(340.0)
            .open(b.open.as_deref() == Some("provider"))
            .disabled(b.busy || b.demo || b.confirm_model.is_some())
            .on_toggle(|s: &mut Viewer| {
                s.bots.open = if s.bots.open.as_deref() == Some("provider") {
                    None
                } else {
                    Some("provider".into())
                }
            })
            .on_close(|s: &mut Viewer| s.bots.open = None)
            .on_choose(move |s: &mut Viewer, value, tx| {
                if owns(s, &owner, epoch) {
                    s.bots.provider.replace_text(value);
                    s.bots.model.replace_text("");
                    s.bots.open = None;
                    tx.instant();
                }
            })
            .build(k, layer),
    );
    out.push(input(
        s,
        "Provider ID (custom values supported)",
        &b.provider,
        |b| &mut b.provider,
        k,
    ));
    out.push(input(s, "Model ID", &b.model, |b| &mut b.model, k));
    if let Some(p) = b.catalog.iter().find(|p| p["slug"] == b.provider.text) {
        let models = p["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !models.is_empty() {
            let owner = s.composer_scope();
            let epoch = b.editor_epoch;
            out.push(
                Select::new("bot-model-catalog", &models)
                    .current(Some(&b.model.text))
                    .width(400.0)
                    .open(b.open.as_deref() == Some("model"))
                    .disabled(b.busy || b.demo || b.confirm_model.is_some())
                    .on_toggle(|s: &mut Viewer| {
                        s.bots.open = if s.bots.open.as_deref() == Some("model") {
                            None
                        } else {
                            Some("model".into())
                        }
                    })
                    .on_close(|s: &mut Viewer| s.bots.open = None)
                    .on_choose(move |s: &mut Viewer, value, tx| {
                        if owns(s, &owner, epoch) {
                            s.bots.model.replace_text(value);
                            s.bots.open = None;
                            tx.instant();
                        }
                    })
                    .build(k, layer),
            );
        }
    }
    out.push(
        row()
            .w_full()
            .flex_wrap()
            .gap(8.0)
            .child(action(s, "Inherit model", Action::ResetModel, b.busy, k))
            .child(action(
                s,
                "Provider authentication & endpoints",
                Action::ProviderSetup,
                b.busy || b.creating,
                k,
            )),
    );
    out
}
fn capabilities(s: &Viewer, k: &Kit) -> Vec<El> {
    let b = &s.bots;
    let mut out = vec![
        heading("Capabilities", k),
        body_text(
            "Only changed sections are saved. All or no toolsets clears the profile pin, matching Hermes inheritance. MCP setup uses this bot's scoped connector editor.",
            k,
        ),
    ];
    if b.detail_loading {
        out.push(body_text("Loading profile capabilities…", k));
    }
    out.push(
        TextInput::new(&b.capability_search)
            .placeholder("Filter skills, tools and MCP servers…")
            .width(None)
            .on_edit(|s: &mut Viewer| &mut s.bots.capability_search)
            .build(k),
    );
    let query = b.capability_search.text.to_lowercase();
    for (kind, label, caps) in [
        ("skills", "Skills", &b.skills),
        ("toolsets", "Toolsets", &b.toolsets),
        ("mcp", "MCP servers", &b.mcp),
    ] {
        out.push(heading(label, k));
        if caps.is_empty() {
            out.push(body_text("No entries reported by this profile.", k));
        }
        for c in caps {
            if !query.is_empty()
                && !format!("{} {}", c.name, c.description)
                    .to_lowercase()
                    .contains(&query)
            {
                continue;
            }
            let owner = s.composer_scope();
            let epoch = b.editor_epoch;
            let name = c.name.clone();
            let next = !c.enabled;
            let mut line = row()
                .w_full()
                .gap(10.0)
                .center_y()
                .py(8.0)
                .child(
                    Switch::new(c.enabled)
                        .disabled(b.busy || b.demo || kind == "mcp" && !c.installed)
                        .on_toggle(move |s: &mut Viewer| {
                            if owns(s, &owner, epoch) {
                                s.bots_action(Action::Toggle(kind.into(), name.clone(), next));
                            }
                        })
                        .build(k),
                )
                .child(
                    col()
                        .grow(1.0)
                        .gap(3.0)
                        .child(text(&c.label).font(k.ty.ui_medium).color(k.c.text_base))
                        .child(
                            text(&c.description)
                                .font(k.ty.small)
                                .color(k.c.text_muted)
                                .wrap(),
                        ),
                );
            if kind == "mcp" {
                line = line.child(action(
                    s,
                    if c.installed { "Configure" } else { "Set up" },
                    Action::OpenMcp(c.name.clone()),
                    b.busy || b.creating,
                    k,
                ));
                if !c.requires.is_empty() {
                    line = line.help(format!("Requires {}", c.requires.join(", ")));
                }
            }
            out.push(line);
        }
    }
    out
}
fn appearance(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    let b = &s.bots;
    let mut out = vec![
        heading("Avatar & appearance", k),
        body_text(
            "Appearance is saved to the profile's shared metadata. Avatar images use the agent asset store, so they follow the bot across devices.",
            k,
        ),
    ];
    let mut preview = el()
        .w(88.0)
        .h(88.0)
        .radius(20.0)
        .clip_rounded(20.0)
        .fill(k.c.layer_01)
        .center();
    if let Some(image) = &b.avatar_preview {
        preview = preview.pixels_fit(image.pixels(), rune::SurfaceFit::Fill);
    } else if let Some(image) = b.shape_preview(88.0, 2.0) {
        preview = preview.pixels_fit(image.pixels(), rune::SurfaceFit::Fit);
    }
    out.push(
        row()
            .w_full()
            .flex_wrap()
            .gap(12.0)
            .center_y()
            .child(preview)
            .child(files::avatar(s, k))
            .child(action(
                s,
                "Clear uploaded avatar",
                Action::AvatarClear,
                b.busy || b.avatar_data.is_none(),
                k,
            ))
            .child(action(
                s,
                if b.generating {
                    "Generating…"
                } else {
                    "Generate avatar"
                },
                Action::AvatarGenerate,
                b.busy || !b.can_generate,
                k,
            )),
    );
    if b.normalizing {
        out.push(body_text("Preparing a 256px avatar image…", k));
    }
    let mut shapes = [
        "circle", "blobatar", "squircle", "pill", "triangle", "hexagon", "cloud", "drop",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>();
    if !shapes.contains(&b.shape.text) {
        shapes.push(b.shape.text.clone());
    }
    let owner = s.composer_scope();
    let epoch = b.editor_epoch;
    out.push(
        Select::new("bot-avatar-shape", &shapes)
            .current(Some(&b.shape.text))
            .width(300.0)
            .open(b.open.as_deref() == Some("shape"))
            .disabled(b.busy || b.demo)
            .on_toggle(|s: &mut Viewer| {
                s.bots.open = if s.bots.open.as_deref() == Some("shape") {
                    None
                } else {
                    Some("shape".into())
                }
            })
            .on_close(|s: &mut Viewer| s.bots.open = None)
            .on_choose(move |s: &mut Viewer, value, tx| {
                if owns(s, &owner, epoch) {
                    s.bots.shape.replace_text(value);
                    s.bots.open = None;
                    tx.instant();
                }
            })
            .build(k, layer),
    );
    if b.shape.text.starts_with("blobatar") {
        let (seed, kind) = crate::pages::bots::faces::blob_parts(&b.shape.text);
        out.push(
            row()
                .w_full()
                .flex_wrap()
                .gap(8.0)
                .child(action(
                    s,
                    if seed.is_empty() {
                        "Lock face to this seed"
                    } else {
                        "Follow bot name"
                    },
                    Action::BlobLock,
                    b.busy,
                    k,
                ))
                .child(action(s, "Randomize face", Action::BlobReroll, b.busy, k)),
        );
        let kinds = [
            "", "round", "organic", "boxy", "capsule", "nub", "cloud", "droplet", "hexagon", "sun",
            "triangle",
        ]
        .iter()
        .map(|s| {
            if s.is_empty() {
                "Automatic".to_owned()
            } else {
                (*s).to_owned()
            }
        })
        .collect::<Vec<_>>();
        let selected = if kind.is_empty() { "Automatic" } else { kind };
        let owner = s.composer_scope();
        let epoch = b.editor_epoch;
        out.push(
            Select::new("bot-blob-kind", &kinds)
                .current(Some(selected))
                .width(300.0)
                .open(b.open.as_deref() == Some("blob-kind"))
                .disabled(b.busy || b.demo)
                .on_toggle(|s: &mut Viewer| {
                    s.bots.open = if s.bots.open.as_deref() == Some("blob-kind") {
                        None
                    } else {
                        Some("blob-kind".into())
                    }
                })
                .on_close(|s: &mut Viewer| s.bots.open = None)
                .on_choose(move |s: &mut Viewer, v, tx| {
                    if owns(s, &owner, epoch) {
                        s.bots.open = None;
                        s.bots_action(Action::BlobKind(if v == "Automatic" {
                            String::new()
                        } else {
                            v.to_owned()
                        }));
                        tx.instant();
                    }
                })
                .build(k, layer),
        );
    }
    out.push(input(
        s,
        "Describe an avatar to generate (optional)",
        &b.avatar_prompt,
        |b| &mut b.avatar_prompt,
        k,
    ));
    out.push(input(
        s,
        "Color override (blank: automatic)",
        &b.color,
        |b| &mut b.color,
        k,
    ));
    out.push(body_text(
        "Existing custom shape identifiers remain intact until you choose a new shape.",
        k,
    ));
    out
}
fn manage(s: &Viewer, k: &Kit) -> Vec<El> {
    let b = &s.bots;
    let mut out = vec![heading("Manage profile", k)];
    if b.creating {
        out.push(body_text(
            "Create the bot before renaming, duplicating, exporting or running profile setup.",
            k,
        ));
        return out;
    }
    let default = b.selected.as_deref() == Some("default");
    out.push(
        row()
            .w_full()
            .flex_wrap()
            .gap(8.0)
            .child(action(
                s,
                if default {
                    "Set default display label"
                } else {
                    "Rename profile ID"
                },
                Action::Rename,
                b.busy || b.dirty(),
                k,
            ))
            .child(action(
                s,
                "Duplicate bot",
                Action::Duplicate,
                b.busy || b.dirty(),
                k,
            ))
            .child(action(s, "Setup command", Action::Setup, b.busy, k))
            .child(action(
                s,
                "Export profile",
                Action::Export,
                b.busy || b.dirty(),
                k,
            ))
            .child(action(
                s,
                "Delete profile",
                Action::Delete,
                b.busy || b.dirty() || default,
                k,
            )),
    );
    out.push(body_text(if default{"The default identity keeps its canonical ID. Its display label can change; it cannot be deleted."}else{"Renaming moves the profile identity. Deleting permanently removes its configuration and history; the application retires open runtimes first."},k));
    if let Some(row) = b.selected_row() {
        if !string(row, "path").is_empty() {
            out.push(body_text(
                &format!("Agent profile directory: {}", string(row, "path")),
                k,
            ));
        }
    }
    if let Some(archive) = &b.exported_archive {
        out.push(
            text(archive)
                .font(k.ty.ui)
                .color(k.c.text_base)
                .selectable(true)
                .wrap(),
        );
        out.push(files::download_archive(s, k));
    }
    out
}
fn dialog_view(s: &Viewer, dialog: &str, k: &Kit) -> El {
    let b = &s.bots;
    let mut body = col()
        .w_full()
        .gap(10.0)
        .px(14.0)
        .py(14.0)
        .fill(k.c.layer_01);
    if let Some(name) = dialog.strip_prefix("rename:") {
        body = body
            .child(heading(
                if name == "default" {
                    "Default profile display label"
                } else {
                    "Rename profile"
                },
                k,
            ))
            .child(input(
                s,
                if name == "default" {
                    "Display label"
                } else {
                    "New profile ID"
                },
                &b.rename,
                |b| &mut b.rename,
                k,
            ))
            .child(action(s, "Rename", Action::SubmitRename, b.busy, k));
    } else if let Some(name) = dialog.strip_prefix("delete:") {
        body=body.child(text(format!("Permanently delete {name}, including its configuration and conversation history?")).font(k.ty.ui).color(k.c.text_base).wrap()).child(action(s,"Delete profile permanently",Action::ConfirmDelete,b.busy,k));
    } else if dialog == "setup" {
        body = body.child(heading("Profile setup command", k));
        if let Some(command) = &b.setup_command {
            let command = command.clone();
            let owner = s.composer_scope();
            let epoch = b.editor_epoch;
            body = body
                .child(
                    text(&command)
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .selectable(true)
                        .wrap(),
                )
                .child(
                    Button::new("Copy setup command")
                        .variant(ButtonVariant::Ghost)
                        .on_press_tx(move |s: &mut Viewer, tx| {
                            if owns(s, &owner, epoch) {
                                tx.copy(command.clone());
                            }
                        })
                        .build(k),
                );
        } else {
            body = body.child(body_text("Loading the command from the agent…", k));
        }
    } else if dialog == "export" {
        body=body.child(heading("Export profile archive",k)).child(body_text("The agent exports config, SOUL, skills and supported profile files. Credentials are excluded by the agent's archive format.",k)).child(input(s,"Output archive path on agent (blank: staging directory)",&b.export_path,|b|&mut b.export_path,k)).child(row().w_full().flex_wrap().gap(8.0).child(action(s,"Export on agent",Action::Export,b.busy,k)).child(files::export(s,k)));
    } else if dialog == "import" {
        body=body.child(heading("Import profile archive",k)).child(body_text("Use an existing .tar.gz archive on the connected agent. Native file selection is available for a local connection.",k)).child(input(s,"Archive path on agent",&b.archive_path,|b|&mut b.archive_path,k)).child(input(s,"New profile ID (optional)",&b.archive_name,|b|&mut b.archive_name,k)).child(row().w_full().flex_wrap().gap(8.0).child(action(s,"Import archive",Action::Import,b.busy,k)).child(files::import(s,k)));
    }
    body.child(action(
        s,
        if b.busy { "Working…" } else { "Close" },
        Action::CancelDialog,
        b.busy,
        k,
    ))
}
