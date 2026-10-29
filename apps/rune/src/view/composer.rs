//! T3 Code composer composition requested by the user; Rune/OpenCode controls
//! and palette, Hermes model capabilities and voice semantics.
mod settings;
mod voice;

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

fn draft(s: &Viewer) -> Draft {
    s.composer
        .draft(&s.composer_scope())
        .cloned()
        .unwrap_or_default()
}

pub fn dock(s: &Viewer, p: &Pal, k: &Kit, layer: &mut Vec<El>) -> El {
    let scope = s.composer_scope();
    let draft = draft(s);
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
        .on_input_key(|s: &mut Viewer, event, tx| s.chat_completion_key(event, tx))
        .on_submit(|s: &mut Viewer, tx| {
            if !s.chat_complete_submit() {
                s.send_draft();
            }
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
        .radius(20.0)
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
    if let Some(replay) = &draft.replay {
        let mut files = row().key("replay-attachments").w_full().gap(6.0).scroll_x();
        for (index, attachment) in replay.attachments.iter().enumerate() {
            let owner = scope.clone();
            files = files.child(
                Button::new(format!("{}  ×", attachment.name))
                    .variant(ButtonVariant::GhostMuted)
                    .help("Remove this restored attachment from the draft")
                    .on_press(move |s: &mut Viewer| {
                        if let Some(bundle) = &mut s.composer.draft_mut(owner.clone()).replay {
                            if index < bundle.attachments.len() {
                                bundle.attachments.remove(index);
                            }
                        }
                    })
                    .build(k)
                    .key(format!("replay-attachment-{index}")),
            );
        }
        content = content.child(files);
    }
    let mode = Button::new(
        if s.admission_mode == crate::conversation::AdmissionMode::Queue {
            "Queue"
        } else {
            "Steer"
        },
    )
    .variant(ButtonVariant::GhostMuted)
    .on_press(|s: &mut Viewer| {
        s.admission_mode = if s.admission_mode == crate::conversation::AdmissionMode::Queue {
            crate::conversation::AdmissionMode::Steer
        } else {
            crate::conversation::AdmissionMode::Queue
        };
    })
    .build(k)
    .key("admission-mode");
    let attachment = IconButton::new(&CLIP)
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
        .key("attach-file");
    let actions = row()
        .key("composer-actions")
        .min_w(0.0)
        .center_y()
        .gap(6.0)
        .child(settings::model_trigger(s, k))
        .child(settings::trigger(k))
        .children(s.chat_running().then_some(mode))
        .child(voice::fan(s, k))
        .child(voice::primary(
            s,
            k,
            !draft.text.text.trim().is_empty()
                || !draft.attachments.is_empty()
                || draft
                    .replay
                    .as_ref()
                    .is_some_and(|r| !r.attachments.is_empty()),
        ));
    content = content.child(auto_grow(field, 2, 7).max_h(168.0)).child(
        row()
            .key("composer-toolbar")
            .w_full()
            .min_w(0.0)
            .gap(4.0)
            .center_y()
            .child(attachment)
            .child(el().min_w(0.0).grow(1.0))
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
            text(if s.is_fixture() {
                "Synthetic scene"
            } else if s.can_send() {
                "Enter to send · Shift+Enter for a new line"
            } else {
                "Connect an agent to send messages"
            })
            .key("draft-only-status")
            .font(k.ty.small)
            .color(k.c.text_faint)
            .px(4.0),
        )
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
        Popup::Settings => (
            "model-settings-menu",
            "model-settings-trigger",
            settings::menu(s, &scope, k),
        ),
        Popup::VoiceEngine => (
            "voice-engine-menu",
            "voice-engine-trigger",
            voice::engine_menu(s, &scope, k),
        ),
    };
    layer.push(
        scrim(format!("{key}-scrim"), |s: &mut Viewer| s.composer.close())
            .transition(rune::Transition::identity()),
    );
    layer.push(
        k.elevate(
            surface
                .key(key)
                .role("dialog")
                .transition(rune::Transition::identity())
                .focusable(true)
                .focus_group()
                .on_key(on_key)
                .modal(true, |s: &mut Viewer| s.composer.close())
                .focus_return(anchor)
                .fill(k.c.layer_01)
                .border(1.0, k.c.border_base)
                .radius(12.0)
                .anchored_fit(anchor, Side::Top, Align::End, 8.0),
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
