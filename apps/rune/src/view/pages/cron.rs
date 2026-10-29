//! Native scheduled-job list, typed editor, blueprint forms and run history.
use crate::{
    app::Viewer,
    pages::cron::{Action, Editor, Job},
};
use rune::{El, ScrollSpec, TextEdit, col, el, row, text};
use rune_kit::{
    Button, ButtonVariant, Checkbox, Kit, Select, SelectAppearance, TextArea, TextInput,
};

fn action(
    label: impl Into<String>,
    key: impl Into<String>,
    value: Action,
    disabled: bool,
    k: &Kit,
) -> El {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled)
        .on_press_tx(move |s: &mut Viewer, tx| {
            s.cron_action(value.clone());
            tx.instant();
        })
        .build(k)
        .key(key)
}
fn label(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.small_medium).color(k.c.text_muted)
}
fn body(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.ui).color(k.c.text_base).wrap()
}
fn field(label_text: &str, input: El, k: &Kit) -> El {
    col()
        .w_full()
        .gap(6.0)
        .child(label(label_text, k))
        .child(input)
}
fn text_field(
    s: &Viewer,
    key: &str,
    label_text: &str,
    k: &Kit,
    read: fn(&Editor) -> &TextEdit,
    write: fn(&mut Editor) -> &mut TextEdit,
) -> El {
    let editor = s.cron.editor.as_ref().expect("mounted cron editor");
    field(
        label_text,
        TextInput::new(read(editor))
            .width(None)
            .on_edit(move |s: &mut Viewer| {
                write(s.cron.editor.as_mut().expect("mounted cron editor"))
            })
            .build(k)
            .key(key),
        k,
    )
}

pub fn page(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let busy = s.cron.busy.contains("editor");
    let header = row()
        .w_full()
        .center_y()
        .gap(8.0)
        .child(
            col()
                .grow(1.0)
                .gap(3.0)
                .child(
                    text("Scheduled jobs")
                        .font(k.ty.body_medium)
                        .color(k.c.text_base),
                )
                .child(label(
                    format!("{} jobs · {}", s.cron.jobs.len(), s.profile),
                    k,
                )),
        )
        .child(action("Refresh", "cron-refresh", Action::Refresh, false, k))
        .child(action("New job", "cron-create", Action::Create, busy, k));
    let search = TextInput::new(&s.cron.query)
        .placeholder("Search jobs and schedules…")
        .width(None)
        .on_edit(|s: &mut Viewer| &mut s.cron.query)
        .build(k)
        .key("cron-search");
    let mut list = col()
        .w_full()
        .gap(3.0)
        .growth(rune::GrowthSpec::default())
        .virtualize(62.0);
    for job in s.cron.visible() {
        let id = job.id.clone();
        list = list.child(
            Button::new(job.title())
                .variant(ButtonVariant::Ghost)
                .label_with({
                    let title = job.title().to_owned();
                    let meta = format!("{} · {}", job.state(), job.schedule());
                    let small = k.ty.small;
                    let faint = k.c.text_faint;
                    move |font, ink| {
                        col()
                            .w_full()
                            .min_w(0.0)
                            .gap(4.0)
                            .child(text(title).font(font).color(ink).truncate())
                            .child(text(meta).font(small).color(faint).truncate())
                    }
                })
                .on_press(move |s: &mut Viewer| s.cron_action(Action::Select(id.clone())))
                .build(k)
                .key(format!("cron-job-{}", job.id))
                .w_full()
                .min_h(56.0)
                .justify_start()
                .fill(if s.cron.selected.as_deref() == Some(job.id.as_str()) {
                    k.c.bg_base
                } else {
                    rune::Color::TRANSPARENT
                }),
        );
    }
    if s.cron.visible().is_empty() {
        list = list.child(
            body(
                if !s.cron.loaded {
                    "Loading scheduled jobs…"
                } else if s.cron.query.text.trim().is_empty() {
                    "No scheduled jobs yet"
                } else {
                    "No matching jobs"
                },
                k,
            )
            .px(8.0)
            .py(12.0),
        );
    }
    if let Some(error) = &s.cron.blueprints_error {
        list = list.child(
            label(format!("Blueprints unavailable: {error}"), k)
                .wrap()
                .px(8.0)
                .py(8.0),
        );
    }
    if !s.cron.blueprints.is_empty() {
        list = list.child(label("Automation blueprints", k).px(8.0).pt(18.0).pb(5.0));
        let query = s.cron.query.text.trim().to_lowercase();
        for blueprint in &s.cron.blueprints {
            if !query.is_empty()
                && !format!(
                    "{} {} {}",
                    blueprint.title,
                    blueprint.description,
                    blueprint.tags.join(" ")
                )
                .to_lowercase()
                .contains(&query)
            {
                continue;
            }
            let title = if let Some(plugin) = &blueprint.plugin {
                format!("{} · {plugin}", blueprint.title)
            } else {
                blueprint.title.clone()
            };
            list = list.child(
                action(
                    title,
                    format!("cron-blueprint-{}", blueprint.key),
                    Action::Blueprint(blueprint.key.clone()),
                    busy,
                    k,
                )
                .w_full()
                .justify_start()
                .min_h(40.0),
            );
        }
    }
    let left = col()
        .w(270.0)
        .min_w(210.0)
        .h_full()
        .gap(12.0)
        .child(search)
        .child(
            el().grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list),
        );
    let content = if s.cron.editor.is_some() {
        editor(s, k, layer)
    } else if let Some(job) = s.cron.selected_job() {
        detail(s, job, k)
    } else {
        col().w_full().gap(12.0).child(body(
            "Select a job to inspect its schedule and run history, or create one from a blueprint.",
            k,
        ))
    };
    let mut page = col()
        .key("cron-page")
        .w_full()
        .h_full()
        .min_h(0.0)
        .gap(16.0)
        .px(24.0)
        .py(20.0)
        .child(header);
    if let Some(error) = &s.cron.error {
        page = page.child(
            text(error)
                .key("cron-error")
                .role("alert")
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if let Some(notice) = &s.cron.notice {
        page = page.child(
            text(notice)
                .key("cron-notice")
                .role("status")
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .wrap(),
        );
    }
    page.child(
        row()
            .grow(1.0)
            .w_full()
            .min_h(0.0)
            .gap(24.0)
            .child(left)
            .child(
                el().grow(1.0)
                    .min_w(0.0)
                    .h_full()
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(content),
            ),
    )
}

fn detail(s: &Viewer, job: &Job, k: &Kit) -> El {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let overdue = crate::pages::cron::next_run_overdue(job, now);
    let busy = s.cron.busy.contains(&job.id);
    let triggering = s.cron.triggering.contains(&job.id);
    let mut detail = col()
        .key("cron-detail")
        .w_full()
        .gap(16.0)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text(job.title())
                        .font(k.ty.body_medium)
                        .color(k.c.text_base)
                        .wrap()
                        .grow(1.0),
                )
                .child(label(job.state(), k)),
        )
        .child(
            row()
                .gap(6.0)
                .child(action(
                    if matches!(job.state(), "paused" | "disabled") {
                        "Resume"
                    } else {
                        "Pause"
                    },
                    "cron-pause-resume",
                    Action::PauseResume(job.id.clone()),
                    busy,
                    k,
                ))
                .child(action(
                    if triggering { "Running…" } else { "Run now" },
                    "cron-run",
                    Action::Trigger(job.id.clone()),
                    busy,
                    k,
                ))
                .child(action(
                    "Edit",
                    "cron-edit",
                    Action::Edit(job.id.clone()),
                    busy || s.cron.busy.contains("editor"),
                    k,
                ))
                .child(action(
                    "Delete",
                    "cron-delete",
                    Action::AskDelete(job.id.clone()),
                    busy,
                    k,
                )),
        );
    for (name, value) in [
        ("Schedule", job.schedule()),
        ("Delivery", job.deliver.as_deref().unwrap_or("local")),
        (
            "Last run",
            job.last_run_at.as_deref().unwrap_or("Not run yet"),
        ),
        (
            if overdue { "Overdue since" } else { "Next run" },
            job.next_run_at.as_deref().unwrap_or("Not scheduled"),
        ),
        (
            "Model override",
            job.model.as_deref().unwrap_or("Agent default at run time"),
        ),
    ] {
        detail = detail.child(
            row()
                .w_full()
                .gap(16.0)
                .child(label(name, k).w(100.0))
                .child(body(value, k).grow(1.0)),
        );
    }
    if let Some(error) = &job.last_error {
        detail = detail.child(
            text(format!("Last run failed: {error}"))
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if s.cron.delete.as_deref() == Some(&job.id) {
        detail = detail.child(
            col()
                .key("cron-delete-confirmation")
                .w_full()
                .gap(8.0)
                .px(12.0)
                .py(12.0)
                .radius(6.0)
                .border(1.0, k.c.border_base)
                .child(body(
                    format!(
                        "Delete ‘{}’? Its schedule will be removed from this agent.",
                        job.title()
                    ),
                    k,
                ))
                .child(
                    row()
                        .gap(8.0)
                        .child(action(
                            "Cancel",
                            "cron-delete-cancel",
                            Action::CancelDelete,
                            busy,
                            k,
                        ))
                        .child(action(
                            "Delete job",
                            "cron-delete-confirm",
                            Action::ConfirmDelete,
                            busy,
                            k,
                        )),
                ),
        );
    }
    detail = detail
        .child(label(
            if job.script_only() {
                "Script-only job"
            } else {
                "Prompt"
            },
            k,
        ))
        .child(body(job.description(), k));
    if triggering {
        detail=detail.child(text("Run requested. The backend executes this job; other jobs and navigation remain available.").key("cron-pending-run").role("status").font(k.ty.ui).color(k.c.text_muted).wrap());
    }
    detail = detail.child(
        row()
            .w_full()
            .center_y()
            .child(label("Run history", k).grow(1.0))
            .child(action(
                "Refresh runs",
                "cron-refresh-runs",
                Action::Select(job.id.clone()),
                false,
                k,
            )),
    );
    if s.cron.runs_loading.as_deref() == Some(job.id.as_str()) {
        detail = detail.child(body("Loading run history…", k));
    } else if s.cron.runs_job.as_deref() != Some(job.id.as_str()) {
        detail = detail.child(body("Refresh runs to load the history", k));
    } else if s.cron.runs.is_empty() {
        detail = detail.child(body("No runs yet", k));
    }
    for (index, run) in s.cron.runs.iter().enumerate() {
        let title = run
            .title
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or(run.preview.as_deref())
            .unwrap_or(&run.session_id);
        let caption = if run.source.as_deref() == Some("cron") {
            format!(
                "{title}{}",
                if s.cron.run_read_only(index) {
                    " · View only"
                } else {
                    ""
                }
            )
        } else {
            format!("{title} · Output")
        };
        let control = if run.source.as_deref() == Some("cron") {
            action(
                caption,
                format!("cron-run-history-{index}"),
                Action::OpenRun(index),
                false,
                k,
            )
            .justify_start()
            .w_full()
        } else {
            body(caption, k).px(8.0).py(6.0)
        };
        detail = detail.child(control);
    }
    detail
}

fn editor(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let e = s.cron.editor.as_ref().expect("mounted cron editor");
    let busy = s.cron.busy.contains("editor");
    let mut form = col()
        .key("cron-editor")
        .w_full()
        .max_w(720.0)
        .gap(16.0)
        .child(
            text(if e.job.is_some() {
                "Edit scheduled job"
            } else {
                "New scheduled job"
            })
            .font(k.ty.body_medium)
            .color(k.c.text_base),
        );
    if e.job.is_none() && !s.cron.blueprints.is_empty() {
        let mut choices = vec![("Custom job".to_owned(), String::new())];
        choices.extend(s.cron.blueprints.iter().map(|b| {
            (
                if let Some(plugin) = &b.plugin {
                    format!("{} · {plugin}", b.title)
                } else {
                    b.title.clone()
                },
                b.key.clone(),
            )
        }));
        let labels: Vec<_> = choices.iter().map(|(label, _)| label.as_str()).collect();
        let current = choices
            .iter()
            .find(|(_, key)| Some(key.as_str()).filter(|s| !s.is_empty()) == e.blueprint.as_deref())
            .map(|(label, _)| label.as_str());
        let select = Select::new("cron-template", &labels)
            .appearance(SelectAppearance::Field)
            .width(400.0)
            .current(current)
            .open(e.open_select.as_deref() == Some("template"))
            .on_toggle(|s: &mut Viewer| {
                if let Some(e) = &mut s.cron.editor {
                    e.open_select = if e.open_select.as_deref() == Some("template") {
                        None
                    } else {
                        Some("template".into())
                    };
                }
            })
            .on_close(|s: &mut Viewer| {
                if let Some(e) = &mut s.cron.editor {
                    e.open_select = None;
                }
            })
            .on_choose(move |s: &mut Viewer, value, tx| {
                let key = choices
                    .iter()
                    .find(|(label, _)| label == value)
                    .map(|(_, key)| key.clone())
                    .unwrap_or_default();
                if key.is_empty() {
                    s.cron_action(Action::Create);
                } else {
                    s.cron_action(Action::Blueprint(key));
                }
                tx.instant();
            })
            .build(k, layer);
        form = form.child(field("Start from", select, k));
    }
    if let Some(key) = &e.blueprint {
        if let Some(blueprint) = s.cron.blueprints.iter().find(|b| &b.key == key) {
            form = form.child(body(&blueprint.description, k));
            for slot in &blueprint.fields {
                if slot.name == "deliver" {
                    form = form.child(delivery(s, k, Some(&slot.name)));
                    continue;
                }
                let Some(value) = e.slots.get(&slot.name) else {
                    continue;
                };
                let name = slot.name.clone();
                let input = if matches!(slot.kind.as_str(), "enum" | "weekdays")
                    && slot.strict != Some(false)
                {
                    let key = format!("slot-{}", slot.name);
                    let toggle_key = key.clone();
                    let close_key = key.clone();
                    Select::new(format!("cron-blueprint-{}", slot.name), &slot.options)
                        .appearance(SelectAppearance::Field)
                        .width(400.0)
                        .current(Some(value.text.as_str()))
                        .open(e.open_select.as_deref() == Some(key.as_str()))
                        .on_toggle(move |s: &mut Viewer| {
                            if let Some(e) = &mut s.cron.editor {
                                e.open_select =
                                    if e.open_select.as_deref() == Some(toggle_key.as_str()) {
                                        None
                                    } else {
                                        Some(toggle_key.clone())
                                    };
                            }
                        })
                        .on_close(move |s: &mut Viewer| {
                            if let Some(e) = &mut s.cron.editor {
                                if e.open_select.as_deref() == Some(close_key.as_str()) {
                                    e.open_select = None;
                                }
                            }
                        })
                        .on_choose(move |s: &mut Viewer, v, tx| {
                            if let Some(e) = &mut s.cron.editor {
                                if let Some(slot) = e.slots.get_mut(&name) {
                                    slot.replace_text(v);
                                }
                                e.open_select = None;
                            }
                            tx.instant();
                        })
                        .build(k, layer)
                } else {
                    TextInput::new(value)
                        .width(None)
                        .placeholder(if slot.kind == "time" {
                            "HH:MM"
                        } else {
                            &slot.help
                        })
                        .on_edit(move |s: &mut Viewer| {
                            s.cron
                                .editor
                                .as_mut()
                                .expect("mounted cron editor")
                                .slots
                                .get_mut(&name)
                                .expect("mounted blueprint slot")
                        })
                        .build(k)
                        .key(format!("cron-slot-{}", slot.name))
                };
                form = form.child(field(
                    &format!(
                        "{}{}",
                        slot.label,
                        if slot.optional { " (optional)" } else { "" }
                    ),
                    input,
                    k,
                ));
                if !slot.help.is_empty() && slot.kind != "text" {
                    form = form.child(label(&slot.help, k).wrap());
                }
            }
        }
    } else {
        form = form.child(text_field(
            s,
            "cron-name",
            "Name (optional)",
            k,
            |e| &e.name,
            |e| &mut e.name,
        ));
        let prompt = TextArea::new(&e.prompt)
            .width(None)
            .auto_grow(4, 10)
            .max_height(220.0)
            .on_edit(|s: &mut Viewer| {
                &mut s.cron.editor.as_mut().expect("mounted cron editor").prompt
            })
            .disabled(busy)
            .build(k)
            .key("cron-prompt");
        form = form.child(field(
            if e.job.as_ref().is_some_and(Job::script_only) {
                "Prompt (optional for this script-only job)"
            } else {
                "Prompt"
            },
            prompt,
            k,
        ));
        if let Some(script) = e
            .job
            .as_ref()
            .filter(|j| j.script_only())
            .and_then(|j| j.script.as_deref())
        {
            form = form.child(body(format!("Stored script: {script}"), k));
        }
        let presets = [
            ("Daily at 09:00", "0 9 * * *"),
            ("Weekdays at 09:00", "0 9 * * 1-5"),
            ("Weekly, Monday 09:00", "0 9 * * 1"),
            ("Monthly, day 1 at 09:00", "0 9 1 * *"),
            ("Hourly", "0 * * * *"),
            ("Every 15 minutes", "*/15 * * * *"),
        ];
        let options: Vec<_> = presets.iter().map(|(label, _)| *label).collect();
        let current = presets
            .iter()
            .find(|(_, expr)| *expr == e.schedule.text.trim())
            .map(|(label, _)| *label);
        let picker = Select::new("cron-frequency", &options)
            .appearance(SelectAppearance::Field)
            .width(400.0)
            .placeholder("Custom schedule")
            .current(current)
            .open(e.open_select.as_deref() == Some("schedule"))
            .on_toggle(|s: &mut Viewer| {
                if let Some(e) = &mut s.cron.editor {
                    e.open_select = if e.open_select.as_deref() == Some("schedule") {
                        None
                    } else {
                        Some("schedule".into())
                    };
                }
            })
            .on_close(|s: &mut Viewer| {
                if let Some(e) = &mut s.cron.editor {
                    e.open_select = None;
                }
            })
            .on_choose(move |s: &mut Viewer, value, tx| {
                if let Some((_, expr)) = presets.iter().find(|(label, _)| *label == value) {
                    s.cron_action(Action::Preset((*expr).into()));
                }
                tx.instant();
            })
            .build(k, layer);
        form = form.child(field("Frequency", picker, k));
        form = form.child(text_field(
            s,
            "cron-schedule",
            "Cron expression or schedule",
            k,
            |e| &e.schedule,
            |e| &mut e.schedule,
        ));
        form = form.child(delivery(s, k, None));
        if !e.job.as_ref().is_some_and(Job::script_only) {
            form = form.child(model(s, k, layer));
        }
    }
    if let Some(error) = &e.error {
        form = form.child(
            text(error)
                .key("cron-editor-error")
                .role("alert")
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    form.child(
        row()
            .w_full()
            .gap(8.0)
            .child(action(
                "Cancel",
                "cron-editor-cancel",
                Action::CancelEditor,
                busy,
                k,
            ))
            .child(action(
                if busy {
                    "Saving…"
                } else if e.job.is_some() {
                    "Save changes"
                } else {
                    "Create job"
                },
                "cron-editor-save",
                Action::Save,
                busy,
                k,
            )),
    )
}

fn delivery(s: &Viewer, k: &Kit, slot: Option<&str>) -> El {
    let e = s.cron.editor.as_ref().expect("mounted cron editor");
    let value = slot
        .and_then(|name| e.slots.get(name))
        .map(|v| v.text.as_str())
        .unwrap_or(&e.deliver);
    let selected: Vec<_> = value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let mut options: Vec<_> = s
        .cron
        .delivery
        .iter()
        .map(|t| {
            (
                t.id.clone(),
                if t.id == "local" {
                    "This desktop".into()
                } else if !t.home_target_set {
                    format!("{} · Set a home channel first", t.name)
                } else {
                    t.name.clone()
                },
            )
        })
        .collect();
    for value in &selected {
        if !options.iter().any(|(id, _)| id == value) {
            options.push(((*value).into(), (*value).into()));
        }
    }
    let mut controls = col().w_full().gap(8.0);
    if let Some(error) = &s.cron.delivery_error {
        controls = controls.child(
            label(
                format!("Delivery discovery failed: {error}. Stored targets are preserved."),
                k,
            )
            .wrap(),
        );
    }
    for (id, name) in options {
        let checked = selected.contains(&id.as_str());
        let slot = slot.map(str::to_owned);
        let key = format!("cron-delivery-{id}");
        controls = controls.child(
            Checkbox::new(checked)
                .label(name)
                .disabled(s.cron.busy.contains("editor"))
                .on_toggle(move |s: &mut Viewer| {
                    if let Some(slot) = &slot {
                        if let Some(e) = &mut s.cron.editor {
                            if let Some(value) = e.slots.get_mut(slot) {
                                let mut targets: Vec<_> = value
                                    .text
                                    .split(',')
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_owned)
                                    .collect();
                                if targets.contains(&id) {
                                    if targets.len() > 1 {
                                        targets.retain(|s| s != &id);
                                    }
                                } else {
                                    targets.push(id.clone());
                                }
                                value.replace_text(targets.join(","));
                            }
                        }
                    } else {
                        s.cron_action(Action::ToggleDelivery(id.clone()));
                    }
                })
                .build(k)
                .key(key),
        );
    }
    field("Deliver results to", controls, k)
}

fn model(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let e = s.cron.editor.as_ref().expect("mounted cron editor");
    let mut choices = vec![(
        "Agent default at run time".to_owned(),
        String::new(),
        String::new(),
    )];
    if let Some(catalog) = &s.cron.models {
        for provider in &catalog.providers {
            if provider.authenticated == Some(false) {
                continue;
            }
            for model in &provider.models {
                if model.available {
                    choices.push((
                        format!("{} / {}", provider.label, model.label),
                        provider.id.clone(),
                        model.id.clone(),
                    ));
                }
            }
        }
    }
    if !e.model.is_empty()
        && !choices
            .iter()
            .any(|(_, p, m)| p == &e.provider && m == &e.model)
    {
        choices.push((
            format!("Stored override: {} / {}", e.provider, e.model),
            e.provider.clone(),
            e.model.clone(),
        ));
    }
    let options: Vec<_> = choices.iter().map(|(label, _, _)| label.as_str()).collect();
    let current = choices
        .iter()
        .find(|(_, provider, model)| provider == &e.provider && model == &e.model)
        .map(|(label, _, _)| label.as_str());
    let picker = Select::new("cron-model", &options)
        .appearance(SelectAppearance::Field)
        .width(400.0)
        .current(current)
        .open(e.open_select.as_deref() == Some("model"))
        .on_toggle(|s: &mut Viewer| {
            if let Some(e) = &mut s.cron.editor {
                e.open_select = if e.open_select.as_deref() == Some("model") {
                    None
                } else {
                    Some("model".into())
                };
            }
        })
        .on_close(|s: &mut Viewer| {
            if let Some(e) = &mut s.cron.editor {
                e.open_select = None;
            }
        })
        .on_choose(move |s: &mut Viewer, value, tx| {
            if let Some((_, provider, model)) = choices.iter().find(|(label, _, _)| label == value)
            {
                s.cron_action(Action::Model(provider.clone(), model.clone()));
            }
            tx.instant();
        })
        .build(k, layer);
    let mut field = field("Model override", picker, k);
    if let Some(error) = &s.cron.models_error {
        field = field.child(
            label(
                format!("Model catalog unavailable: {error}. A stored override is preserved."),
                k,
            )
            .wrap(),
        );
    }
    field
}
