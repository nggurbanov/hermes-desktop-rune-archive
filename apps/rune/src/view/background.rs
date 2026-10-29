use crate::{
    app::Viewer,
    background::{Item, State},
    composer::DraftScope,
};
use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit};
fn item(s: &Viewer, owner: &DraftScope, item: &Item, k: &Kit) -> El {
    let title = if item.stopping {
        "Stopping…".to_string()
    } else {
        match item.state {
            State::Running => "Running".into(),
            State::Done => "Finished".into(),
            State::Failed => format!(
                "Failed{}",
                item.exit_code
                    .map(|code| format!(" · exit {code}"))
                    .unwrap_or_default()
            ),
        }
    };
    let output_owner = owner.clone();
    let output_id = item.id.clone();
    let mut controls = row()
        .w_full()
        .flex_wrap()
        .gap(8.)
        .child(
            text(&item.title)
                .grow(1.)
                .shrink()
                .font(k.ty.ui)
                .color(k.c.text_base)
                .truncate(),
        )
        .child(
            text(title)
                .font(k.ty.small)
                .color(if item.state == State::Failed {
                    k.c.danger.fg
                } else {
                    k.c.text_muted
                }),
        )
        .child(
            Button::new("Output")
                .variant(ButtonVariant::Ghost)
                .on_press(move |s: &mut Viewer| {
                    s.toggle_background_output(output_owner.clone(), output_id.clone())
                })
                .build(k)
                .key(format!("background-output-{}", item.id)),
        );
    let action_owner = owner.clone();
    let id = item.id.clone();
    controls = controls.child(if item.state == State::Running {
        Button::new(if item.stopping { "Stopping…" } else { "Stop" })
            .disabled(item.stopping || !s.background.initialized)
            .variant(ButtonVariant::Ghost)
            .on_press(move |s: &mut Viewer| s.stop_background(action_owner.clone(), id.clone()))
            .build(k)
            .key(format!("background-stop-{}", item.id))
    } else {
        Button::new("Dismiss")
            .variant(ButtonVariant::Ghost)
            .on_press(move |s: &mut Viewer| s.dismiss_background(action_owner.clone(), id.clone()))
            .build(k)
            .key(format!("background-dismiss-{}", item.id))
    });
    let mut body = col().w_full().gap(6.).child(controls);
    if s.background
        .expanded
        .as_ref()
        .is_some_and(|(scope, id)| scope == owner && id == &item.id)
    {
        body = body.child(
            col()
                .w_full()
                .max_h(150.)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    text(if item.output.is_empty() {
                        "No output yet."
                    } else {
                        &item.output
                    })
                    .font(k.ty.mono)
                    .color(k.c.text_muted)
                    .wrap()
                    .selectable(true),
                )
                .key(format!("background-output-body-{}", item.id)),
        );
    }
    body.key(format!("background-process-{}", item.id))
}
pub fn strip(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let items = s.background.items(&owner);
    if items.is_empty() && s.background.error(&owner).is_none() {
        return col();
    }
    let mut body = col()
        .key("background-status-stack")
        .w_full()
        .min_h(44.)
        .shrink()
        .max_h(220.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .gap(8.)
        .padding(10.)
        .border(1., k.c.border_muted)
        .radius(6.)
        .child(
            text(format!("Background processes · {}", items.len()))
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        );
    for process in items {
        body = body.child(item(s, &owner, process, k));
    }
    if let Some(error) = s.background.error(&owner) {
        body = body.child(
            text(error)
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    body
}
