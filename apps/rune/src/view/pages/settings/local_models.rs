use crate::{
    app::Viewer,
    pages::settings::{
        Action,
        local_models::{Action as LocalAction, LocalModels},
    },
};
use rune::{El, FileDialogOptions, TextEdit, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Progress, TextInput};
use serde_json::Value;
fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn action(s: &Viewer, label: &str, a: LocalAction, disabled: bool, k: &Kit) -> El {
    let owner = s.composer_scope();
    let profile = s.settings.local_models.profile.clone();
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled || s.settings.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            let now = s.composer_scope();
            if now.connection == owner.connection
                && now.profile == owner.profile
                && s.settings.local_models.profile == profile
            {
                s.settings_action(Action::LocalModels(a.clone()));
                tx.instant();
            }
        })
        .build(k)
}
fn input(
    s: &Viewer,
    label: &str,
    value: &TextEdit,
    access: impl Fn(&mut LocalModels) -> &mut TextEdit + 'static,
    k: &Kit,
) -> El {
    let owner = s.composer_scope();
    let profile = s.settings.local_models.profile.clone();
    col()
        .w_full()
        .gap(4.0)
        .child(text(label).font(k.ty.small).color(k.c.text_muted))
        .child(
            TextInput::new(value)
                .width(None)
                .disabled(s.settings.local_models.busy || s.settings.demo)
                .on_edit(move |s: &mut Viewer| {
                    let now = s.composer_scope();
                    if now.connection != owner.connection
                        || now.profile != owner.profile
                        || s.settings.local_models.profile != profile
                    {
                        return s.settings.edit_mut("");
                    }
                    access(&mut s.settings.local_models)
                })
                .build(k),
        )
}
fn picker(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let profile = s.settings.local_models.profile.clone();
    Button::new("Choose a GGUF file")
        .variant(ButtonVariant::Ghost)
        .disabled(!s.is_local_connection() || s.settings.local_models.busy || s.settings.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            let now = s.composer_scope();
            if now.connection != owner.connection
                || now.profile != owner.profile
                || s.settings.local_models.profile != profile
                || !s.is_local_connection()
            {
                return;
            }
            let owner = owner.clone();
            let profile = profile.clone();
            tx.open_file(
                FileDialogOptions {
                    title: "Import local GGUF model".into(),
                    ..Default::default()
                }
                .filter("GGUF models", &["gguf"]),
                move |s: &mut Viewer, result, tx| {
                    let now = s.composer_scope();
                    if now.connection != owner.connection
                        || now.profile != owner.profile
                        || s.settings.local_models.profile != profile
                        || !s.is_local_connection()
                    {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            s.settings
                                .local_models
                                .sideload
                                .replace_text(path.to_string_lossy());
                            s.settings_action(Action::LocalModels(LocalAction::Sideload));
                            tx.instant();
                        }
                        Ok(None) => {}
                        Err(_) => {
                            s.settings.local_models.error =
                                Some("The file chooser is unavailable".into())
                        }
                    }
                },
            );
        })
        .build(k)
}
pub(super) fn rows(s: &Viewer, k: &Kit) -> Vec<El> {
    let l = &s.settings.local_models;
    let status = &l.status;
    let mut out=vec![row().w_full().gap(8.0).center_y().child(text("Local model runtime").font(k.ty.body_medium).color(k.c.text_base).grow(1.0)).child(action(s,"Refresh runtime",LocalAction::Load,l.loading||l.busy,k)),text(format!("Managed on active agent profile: {}",if l.profile.is_empty(){&s.profile}else{&l.profile})).font(k.ty.small).color(k.c.text_muted),text("Runtime installation, model files and GPU memory belong to the connected agent. A remote agent uses its own hardware and filesystem.").font(k.ty.small).color(k.c.text_muted).wrap()];
    if let Some(e) = &l.error {
        out.push(
            text(e)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .role("alert")
                .wrap(),
        );
    }
    if let Some(n) = &l.notice {
        out.push(text(n).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if !status.is_object() {
        out.push(
            text(if l.loading {
                "Loading runtime, catalog and hardware…"
            } else {
                "The agent has not supplied local model management. Refresh to retry."
            })
            .font(k.ty.ui)
            .color(k.c.text_muted)
            .wrap(),
        );
        return out;
    }
    let installed = status["runtime_installed"].as_bool() == Some(true);
    let running = status["server_running"].as_bool() == Some(true);
    let installing = l.jobs.iter().any(|j| {
        j["kind"] == "runtime-install" && matches!(j["status"].as_str(), Some("running" | "paused"))
    });
    let downloading = l.jobs.iter().any(|j| {
        j["kind"] == "model-download" && matches!(j["status"].as_str(), Some("running" | "paused"))
    });
    let activating = l.jobs.iter().any(|j| {
        j["kind"] == "model-activate" && matches!(j["status"].as_str(), Some("running" | "paused"))
    });
    out.push(
        text(format!(
            "{} · {} · {}",
            if installed {
                "Runtime installed"
            } else {
                "Runtime not installed"
            },
            string(status, "runtime_backend"),
            if running {
                "Server running"
            } else {
                "Server stopped"
            }
        ))
        .font(k.ty.ui_medium)
        .color(k.c.text_base),
    );
    out.push(
        text(format!(
            "Runtime {} · Configured {}",
            string(status, "tag"),
            string(status, "configured_tag")
        ))
        .font(k.ty.small)
        .color(k.c.text_muted),
    );
    if !installed || status["update_available"].as_bool() == Some(true) {
        out.push(input(
            s,
            "Runtime backend (blank: automatic)",
            &l.backend,
            |l| &mut l.backend,
            k,
        ));
        out.push(action(
            s,
            if installed {
                "Update runtime"
            } else {
                "Install runtime"
            },
            LocalAction::Install,
            l.busy || installing,
            k,
        ));
    }
    if installed {
        out.push(action(
            s,
            if running {
                "Stop model server"
            } else {
                "Start model server"
            },
            LocalAction::Server(!running),
            l.busy,
            k,
        ));
    }
    if (!installed || status["models"].as_array().is_some_and(Vec::is_empty))
        && l.catalog
            .iter()
            .any(|m| m["recommended"].as_bool() == Some(true) && m["fits"].as_bool() == Some(true))
    {
        out.push(action(
            s,
            "Set up the recommended model",
            LocalAction::Quickstart,
            l.busy || l.active_jobs(),
            k,
        ));
    }
    let hardware = &l.hardware;
    if hardware.is_object() {
        out.push(
            text("Agent hardware")
                .font(k.ty.body_medium)
                .color(k.c.text_base)
                .pt(16.0),
        );
        out.push(
            text(format!(
                "{} · GPU memory {}{}",
                hardware["gpu_name"].as_str().unwrap_or("CPU"),
                string(hardware, "vram_label"),
                if hardware["uma"].as_bool() == Some(true) {
                    " · Unified memory"
                } else {
                    ""
                }
            ))
            .font(k.ty.ui)
            .color(k.c.text_base),
        );
        if let Some(percent) = hardware["gpu_util_percent"].as_f64() {
            out.push(
                text(format!("GPU utilization: {percent:.0}%"))
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            );
        }
    }
    if !l.jobs.is_empty() {
        out.push(
            text("Jobs")
                .font(k.ty.body_medium)
                .color(k.c.text_base)
                .pt(16.0),
        );
        for job in &l.jobs {
            let id = string(job, "job_id");
            let phase = string(job, "status");
            let mut line = col()
                .w_full()
                .gap(6.0)
                .py(10.0)
                .child(
                    text(format!(
                        "{} · {} · {}",
                        string(job, "kind"),
                        string(job, "target"),
                        phase
                    ))
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
                )
                .child(
                    text(format!(
                        "{} · {}",
                        string(job, "phase"),
                        string(job, "detail")
                    ))
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap(),
                );
            if matches!(phase, "running" | "paused") {
                line = line
                    .child(
                        Progress::new(job["percent"].as_f64().map(|p| p as f32 / 100.0))
                            .show_value()
                            .build(k),
                    )
                    .child(
                        row()
                            .gap(8.0)
                            .child(action(
                                s,
                                "Pause download",
                                LocalAction::Pause(id.into(), true),
                                l.busy || job["can_pause"].as_bool() != Some(true),
                                k,
                            ))
                            .child(action(
                                s,
                                "Resume download",
                                LocalAction::Pause(id.into(), false),
                                l.busy || job["can_resume"].as_bool() != Some(true),
                                k,
                            )),
                    );
            }
            if let Some(error) = job["error"].as_str().filter(|s| !s.is_empty()) {
                line = line.child(text(error).font(k.ty.small).color(k.c.danger.fg).wrap());
            }
            out.push(line);
        }
    }
    out.push(
        text("Downloaded models")
            .font(k.ty.body_medium)
            .color(k.c.text_base)
            .pt(16.0),
    );
    for model in status["models"].as_array().into_iter().flatten() {
        let id = string(model, "id");
        let active = status["active_model_id"] == id;
        let loaded = status["loaded_models"].get(id).is_some();
        let mut line = col().w_full().gap(6.0).py(10.0).child(
            text(format!(
                "{} · {}{}{}",
                id,
                string(model, "size_label"),
                if active { " · Active" } else { "" },
                if loaded { " · Loaded" } else { "" }
            ))
            .font(k.ty.ui_medium)
            .color(k.c.text_base),
        );
        if let Some(progress) = status["loading"].get(id) {
            line = line
                .child(
                    text(format!("Loading: {}", string(progress, "stage")))
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                )
                .child(
                    Progress::new(progress["percent"].as_f64().map(|p| p as f32 / 100.0))
                        .show_value()
                        .build(k),
                );
        }
        if let Some(placement) = status["placement"].get(id) {
            line = line.child(
                text(format!(
                    "Context {}{}",
                    string(placement, "window_label"),
                    if placement["spilled"].as_bool() == Some(true) {
                        " · Spills to system RAM"
                    } else {
                        ""
                    }
                ))
                .font(k.ty.small)
                .color(k.c.text_muted),
            );
        }
        out.push(
            line.child(
                row()
                    .gap(8.0)
                    .child(action(
                        s,
                        "Use model",
                        LocalAction::Activate(id.into()),
                        l.busy || active || activating,
                        k,
                    ))
                    .child(action(
                        s,
                        "Eject",
                        LocalAction::Eject(id.into()),
                        l.busy || !loaded,
                        k,
                    ))
                    .child(action(
                        s,
                        "Delete files",
                        LocalAction::Delete(id.into()),
                        l.busy,
                        k,
                    )),
            ),
        );
    }
    if let Some(id) = &l.confirm_delete {
        out.push(
            col()
                .w_full()
                .gap(8.0)
                .px(12.0)
                .py(12.0)
                .fill(k.c.layer_01)
                .child(
                    text(format!("Delete model files for {id} from the agent?"))
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .wrap(),
                )
                .child(
                    row()
                        .gap(8.0)
                        .child(action(
                            s,
                            "Delete model files",
                            LocalAction::ConfirmDelete,
                            l.busy,
                            k,
                        ))
                        .child(action(
                            s,
                            "Keep model",
                            LocalAction::CancelDelete,
                            l.busy,
                            k,
                        )),
                ),
        );
    }
    out.push(
        text("Recommended catalog")
            .font(k.ty.body_medium)
            .color(k.c.text_base)
            .pt(16.0),
    );
    for model in &l.catalog {
        let mut line = row().w_full().gap(12.0).center_y().py(12.0).child(
            col()
                .grow(1.0)
                .gap(4.0)
                .child(
                    text(format!(
                        "{}{}",
                        string(model, "display_name"),
                        if model["recommended"].as_bool() == Some(true) {
                            " · Recommended"
                        } else {
                            ""
                        }
                    ))
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
                )
                .child(
                    text(string(model, "description"))
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .wrap(),
                )
                .child(
                    text(format!(
                        "{} · Context {} · {}",
                        string(model, "size_label"),
                        string(model, "native_context_label"),
                        string(model, "fit_summary")
                    ))
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap(),
                ),
        );
        if model["downloaded"].as_bool() == Some(true) {
            line = line.child(text("Downloaded").font(k.ty.small).color(k.c.text_muted));
        } else {
            line = line.child(action(
                s,
                "Download",
                LocalAction::Download(string(model, "id").into()),
                l.busy || downloading || !installed || model["fits"].as_bool() != Some(true),
                k,
            ));
        }
        out.push(line);
    }
    out.push(
        text("Browse Hugging Face")
            .font(k.ty.body_medium)
            .color(k.c.text_base)
            .pt(16.0),
    );
    out.push(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(input(s, "Search GGUF models", &l.query, |l| &mut l.query, k).grow(1.0))
            .child(action(
                s,
                "Search models",
                LocalAction::Search,
                l.searching || l.query.text.trim().len() < 2,
                k,
            )),
    );
    if l.searching {
        out.push(
            text("Searching models…")
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    }
    for hit in &l.hits {
        let repo = string(hit, "repo");
        out.push(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .py(8.0)
                .child(
                    col()
                        .grow(1.0)
                        .gap(4.0)
                        .child(text(repo).font(k.ty.ui_medium).color(k.c.text_base))
                        .child(
                            text(format!(
                                "{} downloads · {} likes{}",
                                hit["downloads"].as_u64().unwrap_or(0),
                                hit["likes"].as_u64().unwrap_or(0),
                                if hit["gated"].as_bool() == Some(true) {
                                    " · Gated access"
                                } else {
                                    ""
                                }
                            ))
                            .font(k.ty.small)
                            .color(k.c.text_muted),
                        ),
                )
                .child(action(
                    s,
                    "Show files",
                    LocalAction::Repository(repo.into()),
                    l.listing,
                    k,
                )),
        );
    }
    if !l.repository.is_empty() {
        out.push(
            text(&l.repository)
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        );
        if l.listing {
            out.push(
                text("Loading repository files…")
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            );
        }
        for group in &l.files {
            out.push(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .py(8.0)
                    .child(
                        text(format!(
                            "{} · {:.2} GB · {}",
                            string(group, "label"),
                            group["total_bytes"].as_f64().unwrap_or(0.0) / 1e9,
                            string(group, "fit")
                        ))
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .grow(1.0)
                        .wrap(),
                    )
                    .child(action(
                        s,
                        "Download files",
                        LocalAction::BrowseDownload(
                            l.repository.clone(),
                            group["paths"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect(),
                        ),
                        l.busy || group["fit"] == "too-big",
                        k,
                    )),
            );
        }
    }
    out.push(
        text("Import existing GGUF")
            .font(k.ty.body_medium)
            .color(k.c.text_base)
            .pt(16.0),
    );
    out.push(text("Enter a file path that exists on the connected agent. The file chooser is available for a local agent connection.").font(k.ty.small).color(k.c.text_muted).wrap());
    out.push(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                input(
                    s,
                    "Existing GGUF path on agent",
                    &l.sideload,
                    |l| &mut l.sideload,
                    k,
                )
                .grow(1.0),
            )
            .child(action(
                s,
                "Import model",
                LocalAction::Sideload,
                l.busy || l.sideload.text.trim().is_empty(),
                k,
            ))
            .child(picker(s, k)),
    );
    out
}
