//! Rune rendering of the retained split/group model with native tab docking.
use crate::{
    app::Viewer,
    layout::{Node, Orientation, Position},
};
use rune::{Cursor, Cx, Drag, El, Phase, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit};
use std::collections::HashMap;
fn visible(s: &Viewer, pane: &str) -> bool {
    match pane {
        "sessions" | "hermes-bots:pane" => s.sidebar.is_open(),
        "review" | "files" | "terminal" => s.inspector.is_open(),
        "workspace" => true,
        _ => false,
    }
}
fn title(pane: &str) -> &str {
    match pane {
        "sessions" => "Sessions / Bots",
        "hermes-bots:pane" => "Bots",
        "workspace" => "Conversation",
        "review" => "Review",
        "files" => "Files",
        "terminal" => "Terminal",
        _ => pane,
    }
}
pub fn render(s: &Viewer, cx: &mut Cx<Viewer>, panes: HashMap<String, El>, k: &Kit) -> El {
    let targets: Vec<_> = s
        .layout
        .groups()
        .into_iter()
        .filter_map(|id| cx.layout(&format!("dock-{id}")).map(|r| (id, r)))
        .collect();
    node(
        s,
        &s.layout,
        &mut panes.into_iter().collect(),
        &targets,
        k,
        true,
    )
    .unwrap_or_else(el)
}
fn node(
    s: &Viewer,
    n: &Node,
    panes: &mut HashMap<String, El>,
    targets: &[(String, [f32; 4])],
    k: &Kit,
    root: bool,
) -> Option<El> {
    match n {
        Node::Group {
            id,
            panes: ids,
            active,
            minimized,
            tab_strip,
        } => {
            let ids: Vec<_> = ids.iter().filter(|p| visible(s, p)).collect();
            if ids.is_empty() {
                return None;
            }
            let selected = ids
                .iter()
                .find(|p| ***p == *active)
                .copied()
                .unwrap_or(ids[0]);
            let mut group = col()
                .key(format!("dock-{id}"))
                .w_full()
                .h_full()
                .min_w(0.)
                .min_h(0.)
                .fill(k.c.bg_deep);
            let strip = *minimized
                || s.arranging
                || tab_strip.as_deref() == Some("always")
                || ids.len() > 1 && tab_strip.as_deref() != Some("never");
            if strip {
                let mut header = row()
                    .w_full()
                    .h(30.)
                    .min_w(0.)
                    .center_y()
                    .gap(2.)
                    .fill(k.c.layer_01);
                for pane in ids {
                    let gid = id.clone();
                    let p = pane.clone();
                    let rect = rune::memo::layout(&format!("dock-tab-{p}"));
                    let drop_targets = targets.to_vec();
                    let drag_pane = p.clone();
                    let mut tab = Button::new(title(&p))
                        .variant(if p == *selected {
                            ButtonVariant::Neutral
                        } else {
                            ButtonVariant::GhostMuted
                        })
                        .on_press(move |s: &mut Viewer| s.layout.activate(&gid, &p))
                        .build(k)
                        .key(format!("dock-tab-{pane}"));
                    if s.arranging {
                        tab = tab.cursor(Cursor::Grab).on_drag(
                            move |s: &mut Viewer, d: &mut Drag, tx: &mut Tx| {
                                if d.phase == Phase::Up
                                    && !d.cancelled()
                                    && (d.dx.abs() + d.dy.abs() > 4.)
                                {
                                    if let Some([x, y, _, _]) = rect {
                                        let x = x + d.start_x + d.dx;
                                        let y = y + d.start_y + d.dy;
                                        if let Some((target, [gx, gy, w, h])) =
                                            drop_targets.iter().find(|(_, r)| {
                                                x >= r[0]
                                                    && x <= r[0] + r[2]
                                                    && y >= r[1]
                                                    && y <= r[1] + r[3]
                                            })
                                        {
                                            let rx = (x - gx) / w;
                                            let ry = (y - gy) / h;
                                            let position = if rx < 0.22 {
                                                Position::Left
                                            } else if rx > 0.78 {
                                                Position::Right
                                            } else if ry < 0.22 {
                                                Position::Top
                                            } else if ry > 0.78 {
                                                Position::Bottom
                                            } else {
                                                Position::Center
                                            };
                                            s.layout.move_pane(&drag_pane, target, position);
                                            tx.instant();
                                        }
                                    }
                                }
                            },
                        );
                    }
                    header = header.child(tab.min_w(0.));
                }
                let gid = id.clone();
                header = header.child(el().grow(1.)).child(
                    Button::new(if *minimized { "+" } else { "−" })
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(move |s: &mut Viewer| s.layout.minimize(&gid))
                        .build(k),
                );
                if s.arranging && selected != "workspace" {
                    let p = selected.clone();
                    header = header.child(
                        Button::new("×")
                            .variant(ButtonVariant::GhostMuted)
                            .on_press(move |s: &mut Viewer| s.layout.close(&p))
                            .build(k),
                    );
                }
                group = group.child(header);
            }
            if !minimized {
                group = group.child(
                    panes
                        .remove(selected.as_str())
                        .unwrap_or_else(|| {
                            el().child(
                                text("This pane is unavailable")
                                    .font(k.ty.small)
                                    .color(k.c.text_muted),
                            )
                        })
                        .grow(1.)
                        .min_h(0.),
                );
            }
            Some(group)
        }
        Node::Split {
            id,
            orientation,
            children,
            weights,
        } => {
            let shown: Vec<_> = children
                .iter()
                .enumerate()
                .filter(|(_, n)| n.panes().iter().any(|p| visible(s, p)))
                .collect();
            if shown.is_empty() {
                return None;
            }
            let axis = *orientation;
            let mut split = if axis == Orientation::Row {
                row()
            } else {
                col()
            };
            split = split
                .key(format!("dock-{id}"))
                .w_full()
                .h_full()
                .min_w(0.)
                .min_h(0.);
            for (at, (index, child)) in shown.iter().enumerate() {
                if at > 0 {
                    let left = shown[at - 1].1.panes();
                    let right = shown[at].1.panes();
                    let sticky_side = root
                        && axis == Orientation::Row
                        && left
                            .iter()
                            .all(|p| p == "sessions" || p == "hermes-bots:pane");
                    let sticky_tools = root
                        && axis == Orientation::Row
                        && right
                            .iter()
                            .all(|p| ["review", "files", "terminal"].contains(&p.as_str()));
                    let key = id.clone();
                    let before = shown[at - 1].0;
                    let all = weights.clone();
                    let sum: f32 = all.iter().sum();
                    let bounds = rune::memo::layout(&format!("dock-{id}"));
                    let mut grip = el()
                        .role("separator")
                        .label("Resize panes")
                        .fill(k.c.border_muted)
                        .hover(k.c.border_focus)
                        .on_drag(move |s: &mut Viewer, d: &mut Drag, tx: &mut Tx| {
                            if sticky_side || sticky_tools {
                                let pane = if sticky_side {
                                    &mut s.sidebar
                                } else {
                                    &mut s.inspector
                                };
                                match d.phase {
                                    Phase::Down => d.data = pane.width(),
                                    Phase::Move => {
                                        pane.set_width(
                                            d.data + if sticky_side { d.dx } else { -d.dx },
                                        );
                                        tx.instant();
                                    }
                                    Phase::Up => {
                                        if d.cancelled() {
                                            pane.set_width(d.data);
                                            tx.instant();
                                        }
                                    }
                                }
                                return;
                            }
                            if d.phase == Phase::Down {
                                d.data = 0.;
                            } else if d.phase == Phase::Move {
                                let length = bounds
                                    .map(|r| if axis == Orientation::Row { r[2] } else { r[3] })
                                    .unwrap_or(800.)
                                    .max(1.);
                                let total = if axis == Orientation::Row { d.dx } else { d.dy }
                                    * sum
                                    / length;
                                let delta = total - d.data;
                                d.data = total;
                                s.layout.resize(&key, before, delta);
                                tx.instant();
                            } else if d.cancelled() {
                                if let Some(Node::Split { weights, .. }) = s.layout.find_mut(&key) {
                                    *weights = all.clone();
                                }
                                tx.instant();
                            }
                        });
                    grip = if axis == Orientation::Row {
                        grip.w(4.).h_full().cursor(Cursor::ResizeColumn)
                    } else {
                        grip.h(4.).w_full().cursor(Cursor::ResizeRow)
                    };
                    split = split.child(grip);
                }
                if let Some(mut view) = node(s, child, panes, targets, k, false) {
                    let weight = weights.get(*index).copied().unwrap_or(1.);
                    let minimized = matches!(
                        child,
                        Node::Group {
                            minimized: true,
                            ..
                        }
                    );
                    let child_panes = child.panes();
                    let side = root
                        && axis == Orientation::Row
                        && child_panes
                            .iter()
                            .all(|p| p == "sessions" || p == "hermes-bots:pane");
                    let tools = root
                        && axis == Orientation::Row
                        && child_panes
                            .iter()
                            .all(|p| ["review", "files", "terminal"].contains(&p.as_str()));
                    view = if axis == Orientation::Row {
                        if minimized {
                            view.w(40.)
                        } else if side {
                            view.w(s.sidebar.width())
                        } else if tools {
                            view.w(s.inspector.width()).key("tools-zone")
                        } else {
                            view.w(0.).grow(weight)
                        }
                    } else if minimized {
                        view.h(30.)
                    } else {
                        view.h(0.).grow(weight)
                    };
                    split = split.child(view);
                }
            }
            Some(split)
        }
    }
}
pub fn controls(s: &Viewer, k: &Kit) -> El {
    let mut panel = col()
        .key("layout-controls")
        .w_full()
        .gap(6.)
        .padding(12.)
        .fill(k.c.layer_01);
    panel = panel.child(
        text("Arrange panes")
            .font(k.ty.ui_medium)
            .color(k.c.text_base),
    );
    panel=panel.child(text("Drag a pane tab to the center of a group to stack it, or to an edge to split. Drag dividers to resize.").font(k.ty.small).color(k.c.text_muted).wrap());
    let mut buttons = row().w_full().gap(6.);
    for pane in ["sessions", "workspace", "review", "files", "terminal"] {
        let p = pane.to_owned();
        buttons = buttons.child(
            Button::new(format!("Show {}", title(pane)))
                .variant(ButtonVariant::GhostMuted)
                .disabled(s.layout.panes().iter().any(|v| v == pane))
                .on_press(move |s: &mut Viewer| {
                    s.layout.open(&p);
                    if ["files", "review", "terminal"].contains(&p.as_str())
                        && !s.inspector.is_open()
                    {
                        s.inspector.toggle();
                    }
                    if p == "sessions" && !s.sidebar.is_open() {
                        s.sidebar.toggle();
                    }
                })
                .build(k),
        );
    }
    panel.child(buttons).child(
        row()
            .gap(8.)
            .child(
                Button::new("Reset layout")
                    .variant(ButtonVariant::GhostMuted)
                    .on_press(|s: &mut Viewer| s.layout = Node::default())
                    .build(k),
            )
            .child(
                Button::new("Done")
                    .on_press(|s: &mut Viewer| s.arranging = false)
                    .build(k),
            ),
    )
}
