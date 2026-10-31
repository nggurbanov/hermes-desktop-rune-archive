//! Project Create presentation. The host owns all effects and validates captured identities.
use crate::projects::{Owner, Scope};
use rune::{El, TextEdit, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, Kit, TextArea, TextInput};

#[derive(Clone, PartialEq, Eq)]
pub struct Target {
    pub owner: Owner,
    pub scope: Scope,
    pub generation: u64,
    pub dialog: u64,
}

#[derive(Clone)]
pub struct OpenTarget {
    pub owner: Owner,
    pub scope: Scope,
    pub generation: u64,
    pub focus_return: String,
}

#[derive(Clone)]
pub enum Action {
    Open(OpenTarget),
    Choose(Target),
    Remove(Target, String),
    Submit(Target),
    Retry(Target),
    Edited(Target),
    Template(Target, usize),
    Shuffle(Target),
    Generate(Target),
    Cancel(Target),
}

#[derive(Clone, Copy)]
pub enum Field {
    Name,
    Idea,
}

/// A stale editor must be redirected to a disposable TextEdit, never the current form.
pub trait Host: 'static {
    fn create_action(&mut self, action: Action);
    fn create_edit(&mut self, target: &Target, field: Field) -> &mut TextEdit;
}

pub struct Snapshot<'a> {
    pub target: Target,
    pub name: &'a TextEdit,
    pub idea: &'a TextEdit,
    pub folders: &'a [String],
    pub error: Option<&'a str>,
    pub pending: bool,
    pub generating: bool,
    pub uncertain: bool,
    pub templates: Vec<(usize, String)>,
}

/// Call only in the Project grouping's All scope, with a proved current owner.
pub fn trigger<H: Host>(target: OpenTarget, kit: &Kit, key: &'static str) -> El {
    Button::new("+")
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .on_press(move |host: &mut H| host.create_action(Action::Open(target.clone())))
        .build(kit)
        .label("Create project")
        .key(key)
}

/// Render status independently of the dialog so a retained write remains visible after close.
pub fn status(message: &str, kit: &Kit) -> El {
    text(message.to_owned())
        .font(kit.ty.small)
        .color(kit.c.text_faint)
        .wrap()
        .role("alert")
}

pub fn modal<H: Host>(form: Snapshot<'_>, kit: &Kit, layers: &mut Vec<El>, focus_return: &str) {
    let target = form.target.clone();
    let edit_target = target.clone();
    let submit_target = target.clone();
    let name_changed = target.clone();
    let escape_target = target.clone();
    let mut panel = col()
        .gap(8.0)
        .py(8.0)
        .child(
            text("Create a project to organize sessions across folders.")
                .font(kit.ty.small)
                .color(kit.c.text_faint)
                .wrap(),
        )
        .child(
            TextInput::new(form.name)
                .width(None)
                .placeholder("e.g. Skunkworks")
                .disabled(form.pending)
                .on_edit(move |host: &mut H| host.create_edit(&edit_target, Field::Name))
                .on_change(move |host: &mut H, _| {
                    host.create_action(Action::Edited(name_changed.clone()))
                })
                .on_submit(move |host: &mut H, tx| {
                    host.create_action(Action::Submit(submit_target.clone()));
                    tx.instant();
                })
                .on_key(move |host: &mut H, event, tx| {
                    if event.key == rune::Key::Escape {
                        host.create_action(Action::Cancel(escape_target.clone()));
                        tx.instant();
                        true
                    } else {
                        false
                    }
                })
                .build(kit)
                .key("project-create-name"),
        )
        .child(text("Folders").font(kit.ty.small).color(kit.c.text_faint));
    if form.folders.is_empty() {
        panel = panel.child(
            text("No folders selected")
                .font(kit.ty.small)
                .color(kit.c.text_faint),
        );
    }
    for (index, path) in form.folders.iter().enumerate() {
        let remove_target = target.clone();
        let remove_path = path.clone();
        let mut item = row().w_full().center_y().gap(6.0).child(
            text(path.clone())
                .font(kit.ty.small)
                .color(kit.c.text_faint)
                .wrap()
                .grow(1.0),
        );
        if index == 0 {
            item = item.child(text("Primary").font(kit.ty.small).color(kit.c.text_faint));
        }
        panel = panel.child(
            item.child(
                Button::new("Remove")
                    .variant(ButtonVariant::GhostMuted)
                    .control_size(ControlSize::Small)
                    .disabled(form.pending)
                    .on_press(move |host: &mut H| {
                        host.create_action(Action::Remove(
                            remove_target.clone(),
                            remove_path.clone(),
                        ))
                    })
                    .build(kit)
                    .key(format!("project-create-remove-{index}")),
            ),
        );
    }
    let choose_target = target.clone();
    panel = panel
        .child(
            Button::new("Choose folder")
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .disabled(form.pending)
                .on_press(move |host: &mut H| {
                    host.create_action(Action::Choose(choose_target.clone()))
                })
                .build(kit)
                .key("project-create-choose"),
        )
        .child(
            text("Idea (optional)")
                .font(kit.ty.small)
                .color(kit.c.text_faint),
        );
    let idea_target = target.clone();
    let idea_escape = target.clone();
    let idea_changed = target.clone();
    let idea_enter = target.clone();
    panel = panel.child(
        TextArea::new(form.idea)
            .placeholder("What's this project about? (saved to IDEA.md)")
            .auto_grow(3, 6)
            .disabled(form.pending)
            .on_edit(move |host: &mut H| host.create_edit(&idea_target, Field::Idea))
            .on_change(move |host: &mut H, _| {
                host.create_action(Action::Edited(idea_changed.clone()))
            })
            .on_submit(move |host: &mut H, tx| {
                host.create_edit(&idea_enter, Field::Idea).insert("\n");
                host.create_action(Action::Edited(idea_enter.clone()));
                tx.instant();
            })
            .on_key(move |host: &mut H, event, tx| {
                if event.key == rune::Key::Escape {
                    host.create_action(Action::Cancel(idea_escape.clone()));
                    tx.instant();
                    true
                } else {
                    false
                }
            })
            .build(kit)
            .key("project-create-idea"),
    );
    let mut choices = row().gap(4.0);
    for (index, label) in form.templates {
        let selected = target.clone();
        choices = choices.child(
            Button::new(label)
                .control_size(ControlSize::Small)
                .on_press(move |host: &mut H| {
                    host.create_action(Action::Template(selected.clone(), index))
                })
                .build(kit),
        );
    }
    let shuffle = target.clone();
    panel = panel.child(choices).child(
        Button::new("Shuffle templates")
            .on_press(move |host: &mut H| host.create_action(Action::Shuffle(shuffle.clone())))
            .build(kit),
    );
    let generate = target.clone();
    panel = panel.child(
        Button::new(if form.generating {
            "Generating…"
        } else {
            "Generate"
        })
        .disabled(form.generating)
        .on_press(move |host: &mut H| host.create_action(Action::Generate(generate.clone())))
        .build(kit),
    );
    if form.pending {
        panel = panel.child(status("Creating project…", kit));
    }
    if let Some(error) = form.error {
        panel = panel.child(status(error, kit));
    }
    if form.uncertain {
        let retry = target.clone();
        panel = panel
            .child(status(
                "The agent may have created this project. Retrying can create a duplicate.",
                kit,
            ))
            .child(
                Button::new("Retry anyway")
                    .on_press(move |host: &mut H| host.create_action(Action::Retry(retry.clone())))
                    .build(kit)
                    .key("project-create-retry"),
            );
    }
    let cancel = target.clone();
    let submit = target.clone();
    panel = panel.child(
        row()
            .gap(6.0)
            .child(
                Button::new("Cancel")
                    .on_press(move |host: &mut H| {
                        host.create_action(Action::Cancel(cancel.clone()))
                    })
                    .build(kit)
                    .key("project-create-cancel"),
            )
            .child(
                Button::new("Create")
                    .disabled(
                        form.pending
                            || form.uncertain
                            || form.name.text.trim().is_empty()
                            || form.folders.is_empty(),
                    )
                    .on_press(move |host: &mut H| {
                        host.create_action(Action::Submit(submit.clone()))
                    })
                    .build(kit)
                    .key("project-create-submit"),
            ),
    );
    Dialog::new("project-create", "Create project")
        .open(true)
        .fit()
        .click_outside(false)
        .on_close(move |host: &mut H| host.create_action(Action::Cancel(target.clone())))
        .child(panel.px(16.0).pb(16.0))
        .build(kit, layers);
    if let Some(layer) = layers.pop() {
        layers.push(layer.focus_return(focus_return.to_owned()));
    }
}
