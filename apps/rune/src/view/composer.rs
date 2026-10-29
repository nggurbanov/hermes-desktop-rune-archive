//! T3 Code composer composition requested by the user; Rune/OpenCode controls
//! and palette, Hermes model capabilities and voice semantics.
use std::time::Duration;

use rune::{
    Align, Color, El, FileDialogOptions, Icon, Key, KeyEvent, ScrollSpec, Side, TextStyle, Tx,
    auto_grow, col, el, icon, row, text, text_input,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, TextInput, scrim,
};
use rune_theme_opencode::icons as ic;

use super::palette::Pal;
use crate::app::Viewer;
use crate::composer::{Draft, DraftScope, ModelKey, Popup};

static STAR: Icon = Icon {
    name: "favorite",
    view: 24.0,
    body: r#"<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9Z" fill="currentColor"/>"#,
};
static CLIP: Icon = Icon {
    name: "attachment",
    view: 24.0,
    body: r#"<path d="m21 11-9 9a6 6 0 0 1-8-8L14 2a4 4 0 0 1 6 6L10 18a2 2 0 0 1-3-3l9-9" stroke="currentColor" stroke-width="1.7" fill="none" stroke-linecap="round"/>"#,
};
static MIC: Icon = Icon {
    name: "microphone",
    view: 24.0,
    body: r#"<rect x="9" y="2" width="6" height="13" rx="3" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="M5 10v2a7 7 0 0 0 14 0v-2M12 19v3M8 22h8" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>"#,
};
static SPEAK: Icon = Icon {
    name: "spoken-replies-off",
    view: 24.0,
    body: r#"<path d="m11 5-6 4H2v6h3l6 4V5ZM16 9l5 6m0-6-5 6M3 2l19 20" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
static WAKE: Icon = Icon {
    name: "wake-word-off",
    view: 24.0,
    body: r#"<path d="M6 8a7 7 0 0 1 13 3c0 5-5 4-5 8a3 3 0 0 1-6 0M10 9a3 3 0 0 1 6 2M3 3l18 18" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>"#,
};
static LOCK: Icon = Icon {
    name: "approval-mode",
    view: 24.0,
    body: r#"<rect x="4" y="10" width="16" height="12" rx="2" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="M8 10V6a4 4 0 0 1 8 0" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>"#,
};

fn draft(s: &Viewer) -> Draft {
    s.composer
        .draft(&s.composer_scope())
        .cloned()
        .unwrap_or_default()
}

fn popup_button(key: &str, label: String, glyph: &'static Icon, popup: Popup, k: &Kit) -> El {
    let title = label.clone();
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .label_with(move |font, ink| {
            row()
                .min_w(0.0)
                .gap(6.0)
                .center_y()
                .child(icon(glyph, 16.0).color(ink))
                .child(text(title).font(font).color(ink).truncate().shrink())
                .child(icon(&ic::CHEVRON_DOWN, 12.0).color(ink))
        })
        .on_press(move |s: &mut Viewer| s.open_composer(popup))
        .build(k)
        .key(key)
        .min_w(0.0)
        .justify_start()
}

pub fn dock(s: &Viewer, p: &Pal, k: &Kit, layer: &mut Vec<El>) -> El {
    let scope = s.composer_scope();
    let draft = draft(s);
    let model = draft
        .model
        .as_ref()
        .map(|m| m.id.as_str())
        .unwrap_or("Select model");
    let effort = draft
        .reasoning
        .as_deref()
        .map(effort_label)
        .unwrap_or("Reasoning");
    let permissions = approval_label(draft.approval_mode.as_deref());
    let field = text_input(&draft.text, TextStyle::new(15.0, 400.0).lh(24.0))
        .key("composer-input")
        .label("Message draft")
        .w_full()
        .color(k.c.text_base)
        .placeholder("Start with a goal", k.c.text_faint)
        .caret(k.c.text_base, k.selection())
        .on_edit(|s: &mut Viewer| {
            let scope = s.composer_scope();
            &mut s.composer.draft_mut(scope).text
        })
        .on_submit(|s: &mut Viewer, tx| {
            let scope = s.composer_scope();
            s.composer.draft_mut(scope).text.insert("\n");
            tx.instant();
        });
    let mut content = col()
        .key("composer")
        .role("group")
        .label("Message composer")
        .instant_changes()
        .w_full()
        .px(16.0)
        .pt(16.0)
        .pb(10.0)
        .gap(12.0)
        .radius(16.0)
        .fill(if p.dark { p.layer1 } else { p.bg_base })
        .border(1.0, k.c.border_base)
        .shadow(1.0, 3.0, p.shadow);
    if !draft.attachments.is_empty() {
        let mut attachments = row().key("draft-attachments").w_full().gap(10.0).scroll_x();
        for (index, attachment) in draft.attachments.iter().enumerate() {
            let mut tile = col()
                .key(format!("attachment-{index}"))
                .w(100.0)
                .h(84.0)
                .radius(10.0)
                .fill(k.c.layer_01)
                .border(1.0, k.c.border_muted)
                .clip_rounded(10.0);
            if let Some(image) = &attachment.preview
                && image.error().is_none()
            {
                tile = tile.child(
                    el().w_full()
                        .h(60.0)
                        .pixels_fit(image.pixels(), rune::SurfaceFit::Fit),
                );
            } else {
                tile = tile.child(
                    el().w_full()
                        .h(60.0)
                        .center()
                        .child(icon(&ic::OUTLINE_COPY, 22.0).color(k.c.icon_muted)),
                );
            }
            let owner = scope.clone();
            tile = tile
                .child(
                    text(&attachment.name)
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .truncate()
                        .px(6.0),
                )
                .child(
                    IconButton::new(&ic::XMARK_SMALL)
                        .variant(IconButtonVariant::Contrast)
                        .label(format!("Remove {}", attachment.name))
                        .control_size(ControlSize::Small)
                        .on_press(move |s: &mut Viewer| {
                            s.composer.remove_attachment(&owner, index);
                        })
                        .build(k)
                        .key(format!("remove-attachment-{index}"))
                        .absolute(74.0, 5.0),
                );
            attachments = attachments.child(tile);
        }
        content = content.child(attachments);
    }
    let model_button = popup_button(
        "model-picker-trigger",
        model.to_owned(),
        &ic::BRAIN,
        Popup::Models,
        k,
    )
    .max_w(280.0)
    .shrink()
    .help("Model for the next message; existing history is unchanged");
    let reasoning = popup_button(
        "reasoning-trigger",
        effort.to_owned(),
        &ic::STATUS,
        Popup::Reasoning,
        k,
    )
    .max_w(125.0)
    .shrink();
    let access = popup_button(
        "permissions-trigger",
        permissions.to_owned(),
        &LOCK,
        Popup::Permissions,
        k,
    )
    .max_w(148.0)
    .shrink();
    let actions = row()
        .key("composer-actions")
        .center_y()
        .gap(6.0)
        .child(
            IconButton::new(&CLIP)
                .variant(IconButtonVariant::GhostMuted)
                .label("Attach file")
                .help("Add a local file to this draft")
                .on_press_tx(|s: &mut Viewer, tx| {
                    let owner = s.composer_scope();
                    s.composer.close();
                    tx.open_file(
                        FileDialogOptions {
                            title: "Attach a file".into(),
                            ..Default::default()
                        },
                        move |s: &mut Viewer, result, _| match result {
                            Ok(Some(path)) => {
                                let _ = s.composer.add_attachment(owner, path);
                            }
                            Ok(None) => {}
                            Err(_) => {
                                s.composer.attachment_error =
                                    Some("The file chooser is unavailable on this desktop".into())
                            }
                        },
                    );
                })
                .build(k)
                .key("attach-file"),
        )
        .child(voice_fan(s, k))
        .child(
            IconButton::new(&ic::ARROW_UP)
                .variant(IconButtonVariant::Submit)
                .control_size(ControlSize::Large)
                .disabled(true)
                .label("Send, not yet available")
                .help("Sending and streaming are the next migration increment")
                .build(k)
                .key("composer-send")
                .radius(16.0),
        );
    content = content.child(auto_grow(field, 2, 7).max_h(168.0)).child(
        row()
            .key("composer-toolbar")
            .w_full()
            .min_w(0.0)
            .gap(4.0)
            .center_y()
            .child(model_button)
            .child(reasoning)
            .child(access)
            .child(el().grow(1.0))
            .child(actions),
    );
    if let Some(error) = &s.composer.attachment_error {
        content = content.child(
            text(error)
                .key("attachment-error")
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    overlays(s, k, layer);
    col()
        .key("dock")
        .w_full()
        .px(16.0)
        .pb(10.0)
        .gap(5.0)
        .child(content)
        .child(
            text("Draft only · Sending is not available yet")
                .key("draft-only-status")
                .font(k.ty.small)
                .color(k.c.text_faint)
                .px(4.0),
        )
}

fn voice_fan(s: &Viewer, k: &Kit) -> El {
    let mut fan = el()
        .key("voice-fan")
        .role("button")
        .label("Voice controls")
        .size(28.0, 28.0)
        .focusable(true)
        .focus_ring(2.0, 1.0, k.c.border_focus)
        .on_press(|s: &mut Viewer| s.composer.voice_expanded = true)
        .on_hover_change(|s: &mut Viewer, inside, _| s.composer.voice_expanded = inside)
        .hover_delays(Duration::ZERO, Duration::from_millis(150))
        .child(
            IconButton::new(&MIC)
                .variant(IconButtonVariant::GhostMuted)
                .disabled(true)
                .label("Voice dictation, not yet available")
                .help("Dictation needs native audio capture")
                .build(k)
                .key("voice-dictation"),
        );
    if s.composer.voice_expanded {
        for (key, label, glyph, y) in [
            (
                "voice-speak",
                "Speak replies, not yet available",
                &SPEAK,
                -36.0,
            ),
            (
                "voice-wake",
                "Wake word hey hermes, not yet available",
                &WAKE,
                -72.0,
            ),
        ] {
            fan = fan.child(
                IconButton::new(glyph)
                    .variant(IconButtonVariant::Neutral)
                    .disabled(true)
                    .label(label)
                    .help("Native audio integration is a later increment")
                    .build(k)
                    .key(key)
                    .size(28.0, 28.0)
                    .radius(14.0)
                    .absolute(0.0, y),
            );
        }
    }
    fan
}

fn overlays(s: &Viewer, k: &Kit, layer: &mut Vec<El>) {
    let scope = s.composer_scope();
    if s.composer.popup_scope.as_ref() != Some(&scope) {
        return;
    }
    let Some(popup) = s.composer.popup else {
        return;
    };
    let (key, anchor, surface) = match popup {
        Popup::Models => (
            "model-picker",
            "model-picker-trigger",
            model_picker(s, &scope, k),
        ),
        Popup::Reasoning => (
            "reasoning-menu",
            "reasoning-trigger",
            reasoning_menu(s, &scope, k),
        ),
        Popup::Permissions => (
            "permissions-menu",
            "permissions-trigger",
            permissions_menu(s, &scope, k),
        ),
    };
    layer.push(scrim(format!("{key}-scrim"), |s: &mut Viewer| {
        s.composer.close()
    }));
    layer.push(
        k.elevate(
            surface
                .key(key)
                .role("dialog")
                .focus_group()
                .on_key(on_key)
                .modal(true, |s: &mut Viewer| s.composer.close())
                .focus_return(anchor)
                .fill(k.c.layer_01)
                .border(1.0, k.c.border_base)
                .radius(12.0)
                .anchored_fit(anchor, Side::Top, Align::Start, 8.0),
            &k.elevation.floating,
        ),
    );
}

fn provider_button(label: &str, short: &str, key: String, selected: bool, k: &Kit) -> El {
    Button::new(short)
        .variant(ButtonVariant::Ghost)
        .help(label)
        .build(k)
        .key(key)
        .label(label)
        .size(36.0, 36.0)
        .fill(if selected {
            k.c.bg_base
        } else {
            Color::TRANSPARENT
        })
}

fn visible_models(s: &Viewer, scope: &DraftScope) -> Vec<(usize, usize)> {
    let Some(catalog) = s.composer.catalog_for(scope) else {
        return vec![];
    };
    let query = s.composer.search.text.trim().to_lowercase();
    catalog
        .providers
        .iter()
        .enumerate()
        .flat_map(|(pi, provider)| {
            let query = &query;
            provider
                .models
                .iter()
                .enumerate()
                .filter_map(move |(mi, model)| {
                    let matches_provider = s
                        .composer
                        .provider
                        .as_ref()
                        .is_none_or(|id| id == &provider.id);
                    let matches_favorite = !s.composer.favorites_only
                        || s.composer
                            .is_favorite(scope.connection, &provider.id, &model.id);
                    let matches_query = query.is_empty()
                        || model.label.to_lowercase().contains(query)
                        || provider.label.to_lowercase().contains(query);
                    (matches_provider && matches_favorite && matches_query).then_some((pi, mi))
                })
        })
        .collect()
}

fn selectable_models(s: &Viewer, scope: &DraftScope) -> Vec<(usize, usize)> {
    let Some(catalog) = s.composer.catalog_for(scope) else {
        return vec![];
    };
    visible_models(s, scope)
        .into_iter()
        .filter(|(pi, mi)| catalog.providers[*pi].models[*mi].available)
        .collect()
}

fn model_picker(s: &Viewer, scope: &DraftScope, k: &Kit) -> El {
    let catalog = s.composer.catalog_for(scope);
    let mut rail = col()
        .key("provider-rail")
        .w(52.0)
        .h_full()
        .min_h(0.0)
        .px(8.0)
        .py(10.0)
        .gap(8.0)
        .items_center()
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    rail = rail.child(
        IconButton::new(&STAR)
            .variant(IconButtonVariant::GhostMuted)
            .label("Favorite models")
            .help("Favorites")
            .expanded(s.composer.favorites_only)
            .on_press(|s: &mut Viewer| {
                s.composer.favorites_only = true;
                s.composer.provider = None;
                s.composer.highlighted = 0;
            })
            .build(k)
            .key("models-favorites")
            .size(36.0, 36.0),
    );
    rail = rail.child(
        provider_button(
            "All providers",
            "All",
            "models-all".into(),
            !s.composer.favorites_only && s.composer.provider.is_none(),
            k,
        )
        .on_press(|s: &mut Viewer| {
            s.composer.provider = None;
            s.composer.favorites_only = false;
            s.composer.highlighted = 0;
        }),
    );
    if let Some(catalog) = catalog {
        for (index, provider) in catalog.providers.iter().enumerate() {
            let id = provider.id.clone();
            let short: String = provider
                .label
                .chars()
                .filter(|c| c.is_alphanumeric())
                .take(2)
                .collect();
            rail = rail.child(
                provider_button(
                    &provider.label,
                    &short,
                    format!("provider-{index}"),
                    s.composer.provider.as_ref() == Some(&id),
                    k,
                )
                .on_press(move |s: &mut Viewer| {
                    s.composer.provider = Some(id.clone());
                    s.composer.favorites_only = false;
                    s.composer.highlighted = 0;
                }),
            );
        }
    }
    let search = TextInput::new(&s.composer.search)
        .placeholder("Search models…")
        .leading_icon(&ic::MAGNIFYING_GLASS)
        .width(None)
        .on_edit(|s: &mut Viewer| &mut s.composer.search)
        .on_change(|s: &mut Viewer, _| s.composer.highlighted = 0)
        .on_key(on_key)
        .build(k)
        .key("model-search")
        .label("Search models");
    let mut results = col()
        .key("model-results")
        .role("listbox")
        .label("Agent models")
        .w_full()
        .gap(4.0)
        .focus_group()
        .growth(rune::GrowthSpec::default())
        .reveal_selected(true)
        .virtualize(60.0);
    let visible = visible_models(s, scope);
    let selectable = selectable_models(s, scope);
    let highlighted = s
        .composer
        .highlighted
        .min(selectable.len().saturating_sub(1));
    if let Some(catalog) = catalog {
        let mut ordinal = 0;
        for (pi, mi) in &visible {
            let provider = &catalog.providers[*pi];
            let model = &provider.models[*mi];
            let number = model.available.then(|| {
                let value = ordinal;
                ordinal += 1;
                value
            });
            let key = ModelKey {
                provider: provider.id.clone(),
                id: model.id.clone(),
            };
            let selected = s.composer.draft(scope).and_then(|d| d.model.as_ref()) == Some(&key);
            let owner = scope.clone();
            let provider_id = provider.id.clone();
            let model_id = model.id.clone();
            let favorite = s
                .composer
                .is_favorite(scope.connection, &provider.id, &model.id);
            let mut option = row()
                .key(format!("model-option-{pi}-{mi}"))
                .role("option")
                .label(format!("{} · {}", model.label, provider.label))
                .focusable(model.available)
                .focus_selected(number == Some(highlighted))
                .focus_group()
                .focus_ring(2.0, 1.0, k.c.border_focus)
                .w_full()
                .min_h(56.0)
                .px(10.0)
                .py(8.0)
                .gap(8.0)
                .center_y()
                .radius(7.0)
                .fill(if selected {
                    k.c.bg_base
                } else if number == Some(highlighted) {
                    k.c.hover
                } else {
                    Color::TRANSPARENT
                })
                .hover(k.c.hover)
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .child(
                            text(&model.label)
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base)
                                .truncate(),
                        )
                        .child(
                            text(format!(
                                "{}{}",
                                provider.label,
                                if model.available {
                                    ""
                                } else {
                                    " · Unavailable"
                                }
                            ))
                            .font(k.ty.small)
                            .color(k.c.text_faint)
                            .truncate(),
                        ),
                );
            if let Some(number) = number.filter(|number| *number < 9) {
                option = option.child(
                    text(format!(
                        "{}{}",
                        if cfg!(target_os = "macos") {
                            "⌘"
                        } else {
                            "Ctrl+"
                        },
                        number + 1
                    ))
                    .font(k.ty.small)
                    .color(k.c.text_faint),
                );
            }
            option = option.child(
                IconButton::new(&STAR)
                    .variant(IconButtonVariant::GhostMuted)
                    .label(if favorite {
                        "Remove favorite"
                    } else {
                        "Favorite model"
                    })
                    .on_press(move |s: &mut Viewer| {
                        s.composer
                            .toggle_favorite(owner.connection, &provider_id, &model_id)
                    })
                    .build(k)
                    .key(format!("favorite-{pi}-{mi}"))
                    .dim(if favorite { 1.0 } else { 0.3 }),
            );
            option = option.child(
                el().size(16.0, 16.0)
                    .children(selected.then(|| icon(&ic::CHECK, 16.0).color(k.c.text_base))),
            );
            if model.available {
                let owner = scope.clone();
                option = option.on_press_tx(move |s: &mut Viewer, tx| {
                    let _ = s.composer.select_model(owner.clone(), key.clone());
                    tx.instant();
                });
            } else {
                option = option.dim(0.5);
            }
            results = results.child(option);
        }
    }
    if visible.is_empty() {
        results = results.child(
            text(if s.composer.catalog_loading {
                "Loading agent models…"
            } else if s.composer.favorites_only {
                "No favorites yet. Star a model to keep it here."
            } else if catalog.is_none() {
                "The agent model catalog is not loaded."
            } else {
                "No matching models"
            })
            .font(k.ty.ui)
            .color(k.c.text_muted)
            .wrap()
            .px(8.0)
            .py(16.0),
        );
    }
    if let Some(error) = &s.composer.catalog_error {
        results = results.child(
            text(error)
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    let reload = Button::new("Refresh models")
        .variant(ButtonVariant::GhostMuted)
        .disabled(s.composer.catalog_loading)
        .on_press(|s: &mut Viewer| s.reload_models())
        .build(k)
        .key("reload-models");
    row()
        .w(520.0)
        .instant_changes()
        .h(440.0)
        .max_h(440.0)
        .min_h(160.0)
        .min_w(0.0)
        .child(rail)
        .child(el().w(1.0).h_full().fill(k.c.border_muted))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .h_full()
                .px(12.0)
                .pt(12.0)
                .pb(8.0)
                .gap(8.0)
                .child(search)
                .child(
                    col()
                        .grow(1.0)
                        .min_h(0.0)
                        .w_full()
                        .scroll(ScrollSpec {
                            stick_bottom: false,
                        })
                        .child(results),
                )
                .child(reload),
        )
}

fn effort_label(value: &str) -> &str {
    match value {
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        "ultra" => "Ultra",
        "none" => "Thinking off",
        _ => value,
    }
}
fn approval_label(value: Option<&str>) -> &str {
    match value {
        Some("manual") => "Manual",
        Some("smart") => "Smart",
        Some("off") => "Full access",
        _ => "Agent default",
    }
}

fn choice(label: &str, key: String, selected: bool, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .build(k)
        .key(key)
        .w_full()
        .justify_start()
        .role("option")
        .focus_selected(selected)
        .fill(if selected {
            k.c.bg_base
        } else {
            Color::TRANSPARENT
        })
}

fn reasoning_menu(s: &Viewer, scope: &DraftScope, k: &Kit) -> El {
    let draft = draft(s);
    let caps: Option<&crate::gateway::ModelCapabilities> = s
        .composer
        .selected_model(scope)
        .and_then(|(_, model)| model.capabilities.as_ref());
    let mut menu = col()
        .w(260.0)
        .padding(8.0)
        .gap(3.0)
        .min_h(0.0)
        .shrink()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            text("Reasoning")
                .font(k.ty.small_medium)
                .color(k.c.text_muted)
                .px(8.0)
                .py(6.0),
        );
    for value in [
        None,
        Some("minimal"),
        Some("low"),
        Some("medium"),
        Some("high"),
        Some("xhigh"),
        Some("max"),
        Some("ultra"),
        Some("none"),
    ] {
        let supported = value.is_none()
            || caps.is_some_and(|c| {
                c.reasoning == Some(true)
                    && (value != Some("none") || c.can_disable_reasoning == Some(true))
            });
        if !supported {
            continue;
        }
        let owner = scope.clone();
        menu = menu.child(
            choice(
                value.map(effort_label).unwrap_or("Agent default"),
                format!("effort-{}", value.unwrap_or("default")),
                draft.reasoning.as_deref() == value,
                k,
            )
            .on_press(move |s: &mut Viewer| {
                s.composer.draft_mut(owner.clone()).reasoning = value.map(String::from);
            }),
        );
    }
    menu = menu
        .child(el().w_full().h(1.0).my(6.0).fill(k.c.border_muted))
        .child(
            text("Service tier")
                .font(k.ty.small_medium)
                .color(k.c.text_muted)
                .px(8.0)
                .py(6.0),
        );
    for (value, label) in [
        (None, "Agent default"),
        (Some("normal"), "Standard"),
        (Some("priority"), "Fast"),
        (Some("ultrafast"), "Ultra Fast"),
    ] {
        let supported = value.is_none()
            || value == Some("normal")
            || caps.is_some_and(|c| {
                if value == Some("priority") {
                    c.fast == Some(true)
                } else {
                    c.ultrafast == Some(true)
                }
            });
        if !supported {
            continue;
        }
        let owner = scope.clone();
        menu = menu.child(
            choice(
                label,
                format!("tier-{}", value.unwrap_or("default")),
                draft.service_tier.as_deref() == value,
                k,
            )
            .on_press(move |s: &mut Viewer| {
                s.composer.draft_mut(owner.clone()).service_tier = value.map(String::from);
            }),
        );
    }
    menu
}

fn permissions_menu(s: &Viewer, scope: &DraftScope, k: &Kit) -> El {
    let draft = draft(s);
    let mut menu = col().w(380.0).padding(8.0).gap(4.0).child(
        text("Approval mode for the next message")
            .font(k.ty.small_medium)
            .color(k.c.text_muted)
            .px(8.0)
            .py(6.0),
    );
    for (value, label, detail) in [
        (
            None,
            "Agent default",
            "Use the connected agent's approval policy.",
        ),
        (
            Some("manual"),
            "Manual",
            "Keep Hermes manual approval checks.",
        ),
        (
            Some("smart"),
            "Smart",
            "Let Hermes evaluate which actions need approval.",
        ),
        (
            Some("off"),
            "Full access",
            "Disable Hermes approval prompts.",
        ),
    ] {
        let owner = scope.clone();
        menu = menu.child(
            col()
                .key(format!("approval-{}", value.unwrap_or("default")))
                .role("option")
                .label(label)
                .focusable(true)
                .focus_selected(draft.approval_mode.as_deref() == value)
                .focus_ring(2.0, 1.0, k.c.border_focus)
                .w_full()
                .px(10.0)
                .py(10.0)
                .gap(5.0)
                .radius(7.0)
                .fill(if draft.approval_mode.as_deref() == value {
                    k.c.bg_base
                } else {
                    Color::TRANSPARENT
                })
                .hover(k.c.hover)
                .child(text(label).font(k.ty.ui_medium).color(k.c.text_base))
                .child(text(detail).font(k.ty.small).color(k.c.text_muted).wrap())
                .on_press_tx(move |s: &mut Viewer, tx| {
                    s.composer.draft_mut(owner.clone()).approval_mode = value.map(String::from);
                    s.composer.close();
                    tx.instant();
                }),
        );
    }
    menu
}

pub fn on_key(s: &mut Viewer, event: &KeyEvent, tx: &mut Tx) -> bool {
    let scope = s.composer_scope();
    if s.composer.popup.is_none() || s.composer.popup_scope.as_ref() != Some(&scope) {
        return false;
    }
    if matches!(event.key, Key::Escape) {
        s.composer.close();
        tx.instant();
        return true;
    }
    if s.composer.popup != Some(Popup::Models) {
        return false;
    }
    if matches!(event.key, Key::Down | Key::Up | Key::Enter) {
        let visible = selectable_models(s, &scope);
        if visible.is_empty() {
            return true;
        }
        if matches!(event.key, Key::Down) {
            s.composer.highlighted = (s.composer.highlighted + 1).min(visible.len() - 1);
        } else if matches!(event.key, Key::Up) {
            s.composer.highlighted = s.composer.highlighted.saturating_sub(1);
        } else {
            let (pi, mi) = visible[s.composer.highlighted.min(visible.len() - 1)];
            if let Some(catalog) = s.composer.catalog_for(&scope) {
                let key = ModelKey {
                    provider: catalog.providers[pi].id.clone(),
                    id: catalog.providers[pi].models[mi].id.clone(),
                };
                let _ = s.composer.select_model(scope, key);
            }
        }
        tx.instant();
        return true;
    }
    if let Key::Command(number @ '1'..='9') = event.key {
        let models = selectable_models(s, &scope);
        if let Some((pi, mi)) = models.get((number as u8 - b'1') as usize)
            && let Some(catalog) = s.composer.catalog_for(&scope)
        {
            let provider = &catalog.providers[*pi];
            let model = &provider.models[*mi];
            let key = ModelKey {
                provider: provider.id.clone(),
                id: model.id.clone(),
            };
            let _ = s.composer.select_model(scope, key);
        }
        tx.instant();
        return true;
    }
    false
}
