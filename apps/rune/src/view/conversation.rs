//! Pending agent decisions and delivery recovery retain their conversation owner.
use crate::app::Viewer;
use rune::{El, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, Kit, TextInput};
fn button(label: impl Into<String>, f: impl Fn(&mut Viewer) + 'static, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .on_press_tx(move |s: &mut Viewer, tx| {
            f(s);
            tx.instant();
        })
        .build(k)
}
pub fn pending(s: &Viewer, k: &Kit) -> El {
    let scope = s.composer_scope();
    let mut box_ = col()
        .key("conversation-decisions")
        .w_full()
        .px(20.0)
        .gap(8.0)
        .max_h(360.0)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        });
    if let Some(reason) = s.read_only_reason() {
        box_ = box_.child(text(reason).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if let Some(error) = &s.compose_error {
        box_ = box_.child(
            text(error)
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    for (id, value) in &s.recoverable_sends {
        let owner = scope.clone();
        let retry = id.clone();
        let discard = id.clone();
        let mut saved = row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                text(format!(
                    "Saved send · {}",
                    value.chars().take(120).collect::<String>()
                ))
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap()
                .grow(1.0),
            )
            .child(button(
                "Retry",
                move |s| s.retry_send(owner.clone(), retry.clone()),
                k,
            ))
            .child(button(
                "Discard",
                move |s| s.discard_send_confirm = Some(discard.clone()),
                k,
            ));
        if s.discard_send_confirm.as_ref() == Some(id) {
            let owner = scope.clone();
            let id = id.clone();
            saved = saved
                .child(button(
                    "Confirm discard",
                    move |s| s.discard_send(owner.clone(), id.clone()),
                    k,
                ))
                .child(button("Keep", |s| s.discard_send_confirm = None, k));
        }
        box_ = box_.child(saved);
    }
    let Some(view) = s.conversation.view(&scope) else {
        return box_;
    };
    if let Some(error) = &view.error {
        box_ = box_
            .child(
                text(error)
                    .role("alert")
                    .font(k.ty.small)
                    .color(k.c.danger.fg)
                    .wrap(),
            )
            .child(button(
                "Reconnect conversation",
                |s| s.reconnect_conversation(),
                k,
            ));
    }
    if let Some(confirmation) = &view.model_confirmation {
        let owner = scope.clone();
        let id = confirmation.submission_id.clone();
        let cancel_owner = scope.clone();
        let cancel_id = id.clone();
        box_ = box_.child(
            col()
                .gap(6.0)
                .child(
                    text(&confirmation.message)
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .wrap(),
                )
                .child(
                    row()
                        .gap(8.0)
                        .child(button(
                            "Confirm model change",
                            move |s| s.confirm_send_model(owner.clone(), id.clone()),
                            k,
                        ))
                        .child(button(
                            "Cancel send",
                            move |s| s.discard_send(cancel_owner.clone(), cancel_id.clone()),
                            k,
                        )),
                ),
        );
    }
    if let Some(id) = &view.uncertain
        && view.model_confirmation.is_none()
    {
        let id = id.clone();
        let owner = scope.clone();
        box_ = box_.child(
            row()
                .gap(8.0)
                .child(
                    text("Delivery is not confirmed. Retry retains the original submission ID.")
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .wrap(),
                )
                .child(button(
                    "Retry saved send",
                    move |s| s.retry_send(owner.clone(), id.clone()),
                    k,
                )),
        );
    }
    for prompt in &view.prompts {
        box_ = box_.child(prompt_card(s, prompt, &scope, k));
    }
    for admission in &view.admissions {
        let mut line = row().w_full().gap(8.0).center_y().child(
            text(format!(
                "{} · {}",
                admission.status,
                admission.text.chars().take(140).collect::<String>()
            ))
            .font(k.ty.small)
            .color(k.c.text_muted)
            .wrap()
            .grow(1.0),
        );
        if admission
            .controls
            .iter()
            .any(|c| c == "cancel" || c == "resolve")
        {
            let id = admission.admission_id.clone();
            let owner = scope.clone();
            line = line.child(button(
                if admission.status == "unknown" {
                    "Resolve"
                } else {
                    "Cancel queued input"
                },
                move |s| s.cancel_admission(owner.clone(), id.clone()),
                k,
            ));
        }
        if admission.controls.iter().any(|c| c == "steer") {
            let id = admission.admission_id.clone();
            let owner = scope.clone();
            line = line.child(button(
                "Steer now",
                move |s| s.steer_admission(owner.clone(), id.clone()),
                k,
            ));
        }
        box_ = box_.child(line);
    }
    if view.preparing {
        box_ = box_.child(
            text("Preparing durable input…")
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    }
    box_.child(el().h(4.0))
}

fn prompt_card(
    s: &Viewer,
    prompt: &crate::conversation::PendingPrompt,
    scope: &crate::composer::DraftScope,
    k: &Kit,
) -> El {
    let mut card = col()
        .w_full()
        .px(12.0)
        .py(10.0)
        .gap(8.0)
        .radius(8.0)
        .border(1.0, k.c.border_base)
        .child(
            text(
                prompt
                    .question
                    .as_deref()
                    .or(prompt.description.as_deref())
                    .unwrap_or("The agent needs your response"),
            )
            .font(k.ty.ui_medium)
            .color(k.c.text_base)
            .wrap(),
        );
    if let Some(command) = &prompt.command {
        card = card.child(
            text(command)
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .wrap()
                .selectable(true),
        );
    }
    if prompt.server_request {
        for question in &prompt.questions {
            let qid = question.qid.clone();
            let id = prompt.prompt_id.clone();
            let mut question_box = col().gap(6.0).child(
                text(&question.question)
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .wrap(),
            );
            if let Some(answer) = prompt.answers.get(&qid) {
                question_box = question_box.child(
                    text(answer.as_deref().unwrap_or("Skipped"))
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .wrap(),
                );
            } else {
                question_box = question_box.child(answer_input(
                    s,
                    scope,
                    &id,
                    &qid,
                    &question.choices,
                    question.multi_select,
                    k,
                ));
                let owner = scope.clone();
                let send_id = id.clone();
                let send_qid = qid.clone();
                let multi = question.multi_select;
                let skip_owner = scope.clone();
                question_box = question_box.child(
                    row()
                        .gap(6.0)
                        .child(button(
                            "Confirm answer",
                            move |s| {
                                let answer = s.question_answer(&owner, &send_id, &send_qid, multi);
                                s.lock_question(
                                    owner.clone(),
                                    send_id.clone(),
                                    send_qid.clone(),
                                    Some(answer),
                                );
                            },
                            k,
                        ))
                        .child(button(
                            "Skip question",
                            move |s| {
                                s.lock_question(skip_owner.clone(), id.clone(), qid.clone(), None)
                            },
                            k,
                        )),
                );
            }
            card = card.child(question_box);
        }
        let owner = scope.clone();
        let id = prompt.prompt_id.clone();
        return card.child(button(
            "Skip remaining questions",
            move |s| s.skip_questions(owner.clone(), id.clone()),
            k,
        ));
    }
    if prompt.kind == "approval" {
        let mut choices = row().w_full().gap(6.0);
        for choice in &prompt.choices {
            let answer = choice.clone();
            let id = prompt.prompt_id.clone();
            let owner = scope.clone();
            choices = choices.child(button(
                choice,
                move |s| s.respond_prompt(owner.clone(), id.clone(), answer.clone()),
                k,
            ));
        }
        return card.child(choices);
    }
    card = card.child(answer_input(
        s,
        scope,
        &prompt.prompt_id,
        "",
        &prompt.choices,
        prompt.multi_select,
        k,
    ));
    let owner = scope.clone();
    let id = prompt.prompt_id.clone();
    let multi = prompt.multi_select;
    card.child(button(
        "Respond",
        move |s| {
            let answer = s.question_answer(&owner, &id, "", multi);
            s.respond_prompt(owner.clone(), id.clone(), answer);
        },
        k,
    ))
}
fn answer_input(
    s: &Viewer,
    scope: &crate::composer::DraftScope,
    id: &str,
    qid: &str,
    choices: &[String],
    multi: bool,
    k: &Kit,
) -> El {
    let key = (scope.clone(), serde_json::to_string(&(id, qid)).unwrap());
    let edit = s.prompt_responses.get(&key).cloned().unwrap_or_default();
    let choice_key = (scope.clone(), id.to_owned(), qid.to_owned());
    let mut options = col().w_full().max_w(480.0).gap(4.0);
    for choice in choices {
        let choice = choice.clone();
        let choice_key = choice_key.clone();
        let edit_key = key.clone();
        let selected = if multi {
            s.prompt_choices
                .get(&choice_key)
                .is_some_and(|v| v.contains(&choice))
        } else {
            edit.text == choice
        };
        let label = choice.clone();
        let toggle = move |s: &mut Viewer| {
            if multi {
                let set = s.prompt_choices.entry(choice_key.clone()).or_default();
                if !set.remove(&choice) {
                    set.insert(choice.clone());
                }
            } else {
                s.prompt_responses
                    .entry(edit_key.clone())
                    .or_default()
                    .replace_text(choice.clone());
            }
        };
        options = options.child(
            Checkbox::new(selected)
                .label(label)
                .on_toggle(toggle)
                .build(k),
        );
    }
    if !multi {
        options = options.child(
            TextInput::new(&edit)
                .width(None)
                .placeholder("Your answer")
                .on_edit(move |s: &mut Viewer| s.prompt_responses.entry(key.clone()).or_default())
                .build(k),
        );
    }
    options
}
