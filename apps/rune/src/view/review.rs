//! Native Review keeps working-tree actions separate from branch and turn comparisons.
mod files;
use crate::{
    app::Viewer,
    review::{Action, Confirmation, File},
};
use rune::{El, ScrollSpec, TextEdit, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, TextArea, TextInput};
fn owns(s: &Viewer, owner: &Option<crate::composer::DraftScope>, epoch: u64) -> bool {
    s.review.owner == *owner && s.review.revision() == epoch
}
fn native_button(label: &str) -> Button {
    let caption = label.to_owned();
    Button::new(label)
        .label_with(move |font, ink| {
            text(caption.clone())
                .font(font)
                .color(ink)
                .truncate()
                .shrink()
                .help_when_truncated(caption.clone())
                .min_w(0.0)
        })
        .variant(ButtonVariant::Ghost)
}
fn action(s: &Viewer, label: &str, a: Action, disabled: bool, k: &Kit) -> El {
    let owner = s.review.owner.clone();
    let epoch = s.review.revision();
    let preview = matches!(
        a,
        Action::Scope(_)
            | Action::Tree
            | Action::Collapse(_)
            | Action::ClearSelection
            | Action::Cancel
            | Action::DiscardMessage
            | Action::SetCommitPush(_)
            | Action::ToggleWorktrees
            | Action::Close
    );
    native_button(label)
        .disabled(disabled || s.review.demo && !preview)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if owns(s, &owner, epoch) {
                s.review_action(a.clone());
                tx.instant();
            }
        })
        .build(k)
        .shrink()
        .max_w(260.0)
}
fn note(value: impl Into<String>, k: &Kit) -> El {
    text(value.into())
        .font(k.ty.small)
        .color(k.c.text_muted)
        .wrap_anywhere()
        .min_w(0.0)
}
fn heading(value: &str, k: &Kit) -> El {
    text(value).font(k.ty.ui_medium).color(k.c.text_base)
}
fn field(
    s: &Viewer,
    label: &str,
    value: &TextEdit,
    access: impl Fn(&mut crate::review::Review) -> &mut TextEdit + 'static,
    k: &Kit,
) -> El {
    let owner = s.review.owner.clone();
    let epoch = s.review.revision();
    col().w_full().gap(4.0).child(note(label, k)).child(
        TextInput::new(value)
            .width(None)
            .disabled(s.review.busy || s.review.demo)
            .on_edit(move |s: &mut Viewer| {
                if !owns(s, &owner, epoch) || s.review.busy {
                    return s.review.edit_detached();
                }
                access(&mut s.review)
            })
            .build(k),
    )
}
pub fn panel(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let r = &s.review;
    let mut pane = col()
        .key("review-pane")
        .role("region")
        .label("Review")
        .w_full()
        .h_full()
        .min_h(0.0)
        .min_w(0.0)
        .px(14.0)
        .py(14.0)
        .gap(10.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            row()
                .w_full()
                .flex_wrap()
                .w_full()
                .center_y()
                .gap(6.0)
                .child(heading("Review", k).grow(1.0))
                .child(action(
                    s,
                    if r.tree { "List" } else { "Tree" },
                    Action::Tree,
                    false,
                    k,
                ))
                .child(action(s, "Refresh", Action::Refresh, r.busy, k))
                .child(action(s, "Close", Action::Close, r.busy, k)),
        );
    if r.demo {
        pane = pane.child(note("Demonstration data · live Git writes are disabled", k));
    }
    if let Some(owner) = &r.owner {
        pane = pane.child(note(format!("{} · {}", owner.profile, r.root), k).selectable(true));
    } else {
        pane = pane.child(note(
            "Choose a conversation with a workspace to review its changes.",
            k,
        ));
    }
    if let Some(error) = &r.error {
        pane = pane.child(
            text(error)
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap()
                .role("alert"),
        );
    }
    for (op, error) in &r.read_errors {
        pane = pane
            .child(note(format!("{}: {error}", op.trim_start_matches("review.")), k).role("alert"));
    }
    if let Some(n) = &r.notice {
        pane = pane.child(note(n, k));
    }
    if let Some(status) = &r.status {
        pane = pane.child(note(
            format!(
                "{} · {} ahead · {} behind · {} changed",
                status["branch"].as_str().unwrap_or("Detached HEAD"),
                status["ahead"].as_u64().unwrap_or(0),
                status["behind"].as_u64().unwrap_or(0),
                status["changed"].as_u64().unwrap_or(r.files.len() as u64)
            ),
            k,
        ));
    }
    let scopes = ["Uncommitted", "Branch", "Last turn"]
        .map(str::to_owned)
        .to_vec();
    let current = match r.scope.as_str() {
        "branch" => "Branch",
        "lastTurn" => "Last turn",
        _ => "Uncommitted",
    };
    let owner = r.owner.clone();
    let epoch = r.revision();
    pane = pane.child(
        Select::new("review-scope", &scopes)
            .current(Some(current))
            .width(180.0)
            .open(r.open.as_deref() == Some("scope"))
            .disabled(r.busy)
            .on_toggle(|s: &mut Viewer| {
                s.review.open = if s.review.open.as_deref() == Some("scope") {
                    None
                } else {
                    Some("scope".into())
                }
            })
            .on_close(|s: &mut Viewer| s.review.open = None)
            .on_choose(move |s: &mut Viewer, value, tx| {
                if owns(s, &owner, epoch) {
                    s.review.open = None;
                    s.review_action(Action::Scope(
                        match value {
                            "Branch" => "branch",
                            "Last turn" => "lastTurn",
                            _ => "uncommitted",
                        }
                        .into(),
                    ));
                    tx.instant();
                }
            })
            .build(k, layer)
            .w_full(),
    );
    if r.scope == "uncommitted" {
        pane = pane.child(
            row()
                .w_full()
                .flex_wrap()
                .gap(5.0)
                .child(action(
                    s,
                    "Stage all",
                    Action::Stage(None),
                    !r.can_mutate() || r.busy || r.files.is_empty(),
                    k,
                ))
                .child(action(
                    s,
                    "Unstage all",
                    Action::Unstage(None),
                    !r.can_mutate() || r.busy,
                    k,
                ))
                .child(action(
                    s,
                    "Discard all",
                    Action::AskRevert(None),
                    !r.can_mutate() || r.busy || r.files.is_empty(),
                    k,
                )),
        );
    } else {
        pane = pane.child(note(
            if r.scope == "lastTurn" && r.base.is_none() {
                "No turn baseline has been captured for this workspace yet."
            } else {
                "This comparison is read-only. Switch to Uncommitted to stage, discard or commit."
            },
            k,
        ));
    }
    let mut listing = col()
        .key("review-files")
        .w_full()
        .gap(2.0)
        .growth(rune::GrowthSpec::default())
        .virtualize(64.0);
    if r.loading && r.files.is_empty() {
        listing = listing.child(note("Loading actual repository changes…", k).key("loading"));
    } else if r.files.is_empty() {
        listing = listing.child(
            note(
                if r.root.is_empty() {
                    "No workspace is selected."
                } else if !r.is_repo {
                    "This workspace is not a Git repository."
                } else {
                    "No changes in this comparison."
                },
                k,
            )
            .key("empty"),
        );
    } else if r.tree {
        listing = listing.children(tree_rows(s, &r.files, "", 0, k));
    } else {
        for f in &r.files {
            listing = listing.child(file_row(s, f, &f.path, 0, k));
        }
    }
    pane = pane.child(
        col()
            .w_full()
            .h(170.0)
            .min_h(90.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(listing),
    );
    if let Some(file) = r
        .selected
        .as_ref()
        .and_then(|path| r.files.iter().find(|f| &f.path == path))
    {
        pane = pane.child(selected(s, file, k));
    }
    if let Some(confirm) = &r.confirm {
        pane = pane.child(confirm_view(s, confirm, k));
    }
    pane = pane.child(action(
        s,
        if r.show_worktrees {
            "Hide branches & worktrees"
        } else {
            "Branches & worktrees"
        },
        Action::ToggleWorktrees,
        r.busy || r.root.is_empty(),
        k,
    ));
    if r.show_worktrees {
        pane = pane.child(worktrees(s, k, layer));
    }
    if r.scope == "uncommitted" {
        pane = pane.child(ship(s, k));
    }
    pane
}
fn file_row(s: &Viewer, f: &File, label: &str, depth: usize, k: &Kit) -> El {
    let selected = s.review.selected.as_deref() == Some(&f.path);
    col()
        .key(format!("file:{}", f.path))
        .w_full()
        .min_w(0.0)
        .h(64.0)
        .gap(2.0)
        .pl((depth.min(20) * 12) as f32)
        .child(
            action(
                s,
                &format!("{} {label}", f.status),
                Action::Select(f.path.clone()),
                s.review.busy,
                k,
            )
            .w_full()
            .min_w(0.0)
            .justify_start()
            .fill(if selected {
                k.c.layer_01
            } else {
                rune::Color::TRANSPARENT
            }),
        )
        .child(
            row()
                .w_full()
                .min_w(0.0)
                .gap(4.0)
                .flex_wrap()
                .child(
                    text(format!("+{}", f.added))
                        .font(k.ty.small)
                        .color(k.c.success.fg),
                )
                .child(
                    text(format!("−{}", f.removed))
                        .font(k.ty.small)
                        .color(k.c.danger.fg),
                )
                .children(f.staged.then(|| note("Staged", k))),
        )
}
fn tree_rows(s: &Viewer, files: &[File], prefix: &str, depth: usize, k: &Kit) -> Vec<El> {
    let mut groups = std::collections::BTreeMap::<String, Vec<File>>::new();
    let mut leaves = vec![];
    for file in files {
        let path = file.path.strip_prefix(prefix).unwrap_or(&file.path);
        if depth < 20 {
            if let Some((folder, _)) = path.split_once('/') {
                groups.entry(folder.into()).or_default().push(file.clone());
                continue;
            }
        }
        leaves.push(file);
    }
    let mut out = vec![];
    for (folder, children) in groups {
        let key = format!("{prefix}{folder}/");
        let collapsed = s.review.collapsed.contains(&key);
        out.push(
            action(
                s,
                &format!("{} {folder}", if collapsed { "›" } else { "⌄" }),
                Action::Collapse(key.clone()),
                false,
                k,
            )
            .w_full()
            .justify_start()
            .pl((depth.min(20) * 12) as f32)
            .key(format!("folder:{key}")),
        );
        if !collapsed {
            out.extend(tree_rows(s, &children, &key, depth + 1, k));
        }
    }
    for f in leaves {
        out.push(file_row(
            s,
            f,
            f.path.strip_prefix(prefix).unwrap_or(&f.path),
            depth,
            k,
        ));
    }
    out
}
fn selected(s: &Viewer, file: &File, k: &Kit) -> El {
    let r = &s.review;
    let mut panel = col().w_full().min_h(0.0).gap(6.0).child(
        row()
            .w_full()
            .flex_wrap()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(heading(&file.path, k).min_w(0.0).wrap_anywhere().grow(1.0))
            .child(action(s, "Hide diff", Action::ClearSelection, false, k)),
    );
    if r.scope == "uncommitted" {
        panel = panel.child(
            row()
                .w_full()
                .flex_wrap()
                .gap(5.0)
                .child(action(
                    s,
                    if file.staged { "Unstage" } else { "Stage" },
                    if file.staged {
                        Action::Unstage(Some(file.path.clone()))
                    } else {
                        Action::Stage(Some(file.path.clone()))
                    },
                    !r.can_mutate() || r.busy,
                    k,
                ))
                .child(action(
                    s,
                    "Discard",
                    Action::AskRevert(Some(file.path.clone())),
                    !r.can_mutate() || r.busy,
                    k,
                )),
        );
    }
    panel = panel.child(files::actions(s, file, k));
    let mut diff = col()
        .key("review-diff")
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtualize(19.0)
        .gap(0.0);
    if r.diff_loading {
        diff = diff.child(note("Loading diff…", k).key("loading"));
    } else if r.diff.is_empty() {
        diff = diff.child(note("No diff content for this file.", k).key("empty"));
    } else {
        for (index, line) in r.diff.lines().enumerate() {
            let color = if line.starts_with('+') && !line.starts_with("+++") {
                k.c.success.fg
            } else if line.starts_with('-') && !line.starts_with("---") {
                k.c.danger.fg
            } else if line.starts_with("@@") {
                k.c.text_accent
            } else {
                k.c.text_muted
            };
            diff = diff.child(
                text(line)
                    .key(format!("diff-{index}"))
                    .font(k.ty.small)
                    .color(color)
                    .wrap()
                    .selectable(true),
            );
        }
    }
    panel.child(
        col()
            .w_full()
            .min_h(90.0)
            .max_h(310.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(diff),
    )
}
fn ship(s: &Viewer, k: &Kit) -> El {
    let r = &s.review;
    let owner = r.owner.clone();
    let epoch = r.revision();
    let can_commit = r.can_mutate()
        && !r.busy
        && !r.generating
        && !r.files.is_empty()
        && !r.message.text.trim().is_empty()
        && !r.mutation_uncertain;
    let mut bar = col().w_full().gap(7.0);
    if r.mutation_uncertain {
        bar=bar.child(note("A previous commit request has an unconfirmed outcome. Review HEAD and history before retrying.",k)).child(action(s,"I reviewed the result; allow another commit",Action::AcknowledgeCommit,r.busy,k));
    }
    let submit_owner = owner.clone();
    bar = bar.child(
        TextArea::new(&r.message)
            .width(None)
            .placeholder("Commit message · Ctrl/Cmd+Enter")
            .auto_grow(1, 5)
            .max_height(110.0)
            .disabled(r.busy || r.generating || r.demo)
            .on_edit(move |s: &mut Viewer| {
                if !owns(s, &owner, epoch) || s.review.busy || s.review.generating {
                    return s.review.edit_detached();
                }
                &mut s.review.message
            })
            .on_submit(move |s: &mut Viewer, tx| {
                if owns(s, &submit_owner, epoch) {
                    let push = s.review.commit_push;
                    s.review_action(Action::Commit(push));
                    tx.instant();
                }
            })
            .build(k),
    );
    bar = bar.child(
        row()
            .w_full()
            .flex_wrap()
            .gap(5.0)
            .child(action(
                s,
                if r.generating {
                    "Stop generating"
                } else {
                    "Generate message"
                },
                if r.generating {
                    Action::CancelGenerate
                } else {
                    Action::Generate
                },
                r.busy || r.files.is_empty(),
                k,
            ))
            .child(action(
                s,
                "Discard message",
                Action::DiscardMessage,
                r.busy || r.message.text.is_empty(),
                k,
            )),
    );
    bar = bar.child(
        row()
            .w_full()
            .flex_wrap()
            .gap(5.0)
            .child(action(s, "Commit", Action::Commit(false), !can_commit, k))
            .child(action(
                s,
                "Commit & push",
                Action::Commit(true),
                !can_commit,
                k,
            ))
            .child(action(
                s,
                "Push",
                Action::Push,
                !r.can_mutate() || r.busy,
                k,
            )),
    );
    bar = bar.child(
        row()
            .w_full()
            .flex_wrap()
            .w_full()
            .gap(6.0)
            .center_y()
            .child(note("Ctrl/Cmd+Enter action", k).grow(1.0))
            .child(action(
                s,
                if r.commit_push {
                    "Commit & push"
                } else {
                    "Commit"
                },
                Action::SetCommitPush(!r.commit_push),
                r.busy,
                k,
            )),
    );
    let pr = r
        .ship
        .as_ref()
        .and_then(|s| s["pr"]["url"].as_str())
        .is_some();
    bar.child(
        row()
            .w_full()
            .flex_wrap()
            .gap(5.0)
            .child(action(
                s,
                if pr { "Open PR" } else { "Create PR" },
                Action::CreatePr,
                r.busy || r.ship.as_ref().is_none_or(|s| s["ghReady"] != true),
                k,
            ))
            .child(action(
                s,
                "Ask Hermes to open PR",
                Action::AgentShip,
                r.busy
                    || r.files.is_empty()
                    || r.owner.as_ref().is_none_or(|o| o.session.is_none()),
                k,
            )),
    )
}
fn worktrees(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let r = &s.review;
    let mut panel = col()
        .w_full()
        .gap(7.0)
        .max_h(330.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(heading("Branches & worktrees", k));
    for tree in &r.worktrees {
        let path = tree["path"].as_str().unwrap_or("");
        let branch = tree["branch"].as_str().unwrap_or("Detached");
        let mut controls = row().gap(5.0).child(action(
            s,
            "Open",
            Action::PickWorktree(path.into()),
            r.busy,
            k,
        ));
        if tree["isMain"] != true {
            controls = controls
                .child(action(
                    s,
                    "Remove",
                    Action::AskRemoveWorktree(path.into(), false),
                    r.busy,
                    k,
                ))
                .child(action(
                    s,
                    "Force remove…",
                    Action::AskRemoveWorktree(path.into(), true),
                    r.busy,
                    k,
                ));
        }
        panel = panel.child(
            col()
                .gap(3.0)
                .child(note(
                    format!(
                        "{branch}{}{}",
                        if tree["isMain"] == true {
                            " · main checkout"
                        } else {
                            ""
                        },
                        if tree["locked"] == true {
                            " · locked"
                        } else {
                            ""
                        }
                    ),
                    k,
                ))
                .child(note(path, k).selectable(true))
                .child(controls),
        );
    }
    let current = r.status.as_ref().and_then(|s| s["branch"].as_str());
    let switchable = r
        .branches
        .iter()
        .filter(|b| {
            b["isRemote"] != true && (b["checkedOut"] != true || b["name"].as_str() == current)
        })
        .filter_map(|b| b["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if !switchable.is_empty() {
        let owner = r.owner.clone();
        let epoch = r.revision();
        panel = panel.child(note("Switch the current checkout", k)).child(
            Select::new("review-switch-branch", &switchable)
                .current(current)
                .placeholder("Choose a local branch")
                .width(240.0)
                .open(r.open.as_deref() == Some("switch"))
                .disabled(r.busy || r.demo)
                .on_toggle(|s: &mut Viewer| {
                    s.review.open = if s.review.open.as_deref() == Some("switch") {
                        None
                    } else {
                        Some("switch".into())
                    }
                })
                .on_close(|s: &mut Viewer| s.review.open = None)
                .on_choose(move |s: &mut Viewer, v, tx| {
                    if owns(s, &owner, epoch) {
                        s.review.open = None;
                        s.review_action(Action::SwitchBranch(v.into()));
                        tx.instant();
                    }
                })
                .build(k, layer)
                .w_full(),
        );
    }
    let branches = r
        .branches
        .iter()
        .filter_map(|b| b["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if !branches.is_empty() {
        let owner = r.owner.clone();
        let epoch = r.revision();
        panel = panel.child(
            Select::new("review-existing-branch", &branches)
                .placeholder("Open a branch in a worktree")
                .width(240.0)
                .open(r.open.as_deref() == Some("existing"))
                .disabled(r.busy || r.demo)
                .on_toggle(|s: &mut Viewer| {
                    s.review.open = if s.review.open.as_deref() == Some("existing") {
                        None
                    } else {
                        Some("existing".into())
                    }
                })
                .on_close(|s: &mut Viewer| s.review.open = None)
                .on_choose(move |s: &mut Viewer, v, tx| {
                    if owns(s, &owner, epoch) {
                        s.review.open = None;
                        s.review_action(Action::ExistingWorktree(v.into()));
                        tx.instant();
                    }
                })
                .build(k, layer)
                .w_full(),
        );
    }
    panel = panel
        .child(field(s, "Worktree name", &r.name, |r| &mut r.name, k))
        .child(field(
            s,
            "Branch (blank: hermes/name)",
            &r.branch,
            |r| &mut r.branch,
            k,
        ));
    let bases = std::iter::once("HEAD".into())
        .chain(
            r.bases
                .iter()
                .filter_map(|b| b["name"].as_str().map(str::to_owned)),
        )
        .collect::<Vec<_>>();
    let owner = r.owner.clone();
    let epoch = r.revision();
    panel = panel.child(
        Select::new("review-base-branch", &bases)
            .current(Some(if r.source.text.is_empty() {
                "HEAD"
            } else {
                &r.source.text
            }))
            .width(240.0)
            .open(r.open.as_deref() == Some("base"))
            .disabled(r.busy || r.demo)
            .on_toggle(|s: &mut Viewer| {
                s.review.open = if s.review.open.as_deref() == Some("base") {
                    None
                } else {
                    Some("base".into())
                }
            })
            .on_close(|s: &mut Viewer| s.review.open = None)
            .on_choose(move |s: &mut Viewer, v, tx| {
                if owns(s, &owner, epoch) {
                    s.review.source.replace_text(v);
                    s.review.open = None;
                    tx.instant();
                }
            })
            .build(k, layer)
            .w_full(),
    );
    if !r.is_repo {
        panel=panel.child(note("Creating a worktree initializes this workspace as a Git repository with an empty initial commit.",k));
    }
    panel.child(action(
        s,
        "Create worktree",
        Action::AddWorktree,
        r.busy || r.root.is_empty(),
        k,
    ))
}
fn confirm_view(s: &Viewer, confirm: &Confirmation, k: &Kit) -> El {
    let description=match confirm{Confirmation::Revert(Some(path))=>format!("Discard staged and unstaged changes to {path}? Untracked content at this path is removed. This cannot be undone."),Confirmation::Revert(None)=>"Discard every staged, unstaged and untracked change in this repository? This cannot be undone.".into(),Confirmation::RemoveWorktree(path,force)=>format!("{}remove worktree {path}? Its branch is preserved.{}",if *force{"Force "}else{""},if *force{" Uncommitted work in the removed checkout can be lost."}else{""})};
    col()
        .w_full()
        .gap(7.0)
        .px(10.0)
        .py(10.0)
        .fill(k.c.layer_01)
        .child(
            text(description)
                .font(k.ty.small)
                .color(k.c.text_base)
                .wrap(),
        )
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(6.0)
                .child(action(s, "Confirm", Action::Confirm, s.review.busy, k))
                .child(action(s, "Cancel", Action::Cancel, false, k)),
        )
}
