//! Presets contain geometry, never a snapshot of another conversation's live tiles.
use crate::layout::{Node, Orientation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const TOOLS: [&str; 3] = ["files", "review", "terminal"];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub tree: Node,
    pub hidden: BTreeSet<String>,
    pub sidebar_open: bool,
    pub sidebar_width: f32,
    pub inspector_width: f32,
    pub active: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub id: String,
    pub name: String,
    pub tree: Node,
    pub hidden: BTreeSet<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Decks {
    pub active: String,
    pub hidden: BTreeSet<String>,
    pub simple: Option<Snapshot>,
    pub advanced: Option<Snapshot>,
    pub saved: Vec<Saved>,
    #[serde(skip)]
    pub tool_gate: bool,
}
impl Default for Decks {
    fn default() -> Self {
        Self {
            active: "default".into(),
            hidden: TOOLS.into_iter().map(str::to_owned).collect(),
            simple: None,
            advanced: None,
            saved: vec![],
            tool_gate: false,
        }
    }
}
impl Decks {
    pub fn save(&mut self, name: &str, tree: &Node) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err("Name the layout using 1–80 characters".into());
        }
        let tree = geometry(tree.clone()).ok_or("This layout contains no reusable panes")?;
        let panes: BTreeSet<_> = tree.panes().into_iter().collect();
        let hidden = self.hidden.intersection(&panes).cloned().collect();
        let slug = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>();
        let slug = slug
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let id = if slug.is_empty() {
            format!("user-{}", uuid::Uuid::new_v4())
        } else {
            format!("user-{slug}")
        };
        if let Some(saved) = self.saved.iter_mut().find(|p| p.name == name || p.id == id) {
            saved.name = name.into();
            saved.tree = tree;
            saved.hidden = hidden;
            self.active = saved.id.clone();
        } else {
            self.saved.push(Saved {
                id: id.clone(),
                name: name.into(),
                tree,
                hidden,
            });
            self.active = id;
        }
        Ok(())
    }
    pub fn delete(&mut self, id: &str) {
        self.saved.retain(|p| p.id != id);
        if self.active == id {
            self.active = "custom".into();
        }
    }
    pub fn get(&self, id: &str) -> Option<(Node, BTreeSet<String>)> {
        if let Some(p) = self.saved.iter().find(|p| p.id == id) {
            return Some((p.tree.clone(), p.hidden.clone()));
        }
        builtin(id).map(|(tree, rests)| (tree, rests.into_iter().map(str::to_owned).collect()))
    }
    pub fn normalize(&mut self) {
        self.saved = self
            .saved
            .drain(..)
            .filter_map(|mut p| {
                if !p.id.starts_with("user-") || p.name.trim().is_empty() {
                    return None;
                }
                p.tree = geometry(p.tree)?;
                p.hidden.retain(|id| p.tree.panes().contains(id));
                Some(p)
            })
            .collect();
        let mut ids = BTreeSet::new();
        self.saved.retain(|p| ids.insert(p.id.clone()));
        for slot in [&mut self.simple, &mut self.advanced] {
            if let Some(s) = slot.take() {
                if let Some(tree) = s
                    .tree
                    .clone()
                    .normalize()
                    .filter(|t| t.panes().iter().any(|p| p == "workspace"))
                {
                    *slot = Some(Snapshot { tree, ..s });
                }
            }
        }
    }
}
fn group(ids: &[&str]) -> Node {
    Node::group(ids.iter().map(|s| (*s).into()).collect())
}
fn row(children: Vec<Node>, weights: &[f32]) -> Node {
    Node::split(Orientation::Row, children, weights.into())
}
fn column(children: Vec<Node>, weights: &[f32]) -> Node {
    Node::split(Orientation::Column, children, weights.into())
}
fn basic() -> Node {
    row(
        vec![
            group(&["sessions"]),
            column(vec![group(&["workspace"]), group(&["terminal"])], &[3., 1.]),
            row(vec![group(&["review"]), group(&["files"])], &[1., 1.2]),
        ],
        &[1., 3.4, 1.25],
    )
}
fn mirror(tree: &mut Node) {
    if let Node::Split {
        orientation,
        children,
        weights,
        ..
    } = tree
    {
        for child in children.iter_mut() {
            mirror(child);
        }
        if *orientation == Orientation::Row {
            children.reverse();
            weights.reverse();
        }
    }
}
pub fn choices(simple: bool) -> &'static [(&'static str, &'static str)] {
    if simple {
        &[
            ("sidebar-left", "Sidebar left"),
            ("sidebar-right", "Sidebar right"),
        ]
    } else {
        &[
            ("default", "Default"),
            ("basic", "Basic"),
            ("focus", "Focus"),
            ("terminal-deck", "Terminal deck"),
            ("quad", "Quad"),
        ]
    }
}
pub fn builtin(id: &str) -> Option<(Node, Vec<&'static str>)> {
    let rests = vec!["terminal", "files", "review"];
    Some(match id {
        "default" => (Node::default(), vec![]),
        "basic" | "sidebar-left" => (basic(), rests),
        "sidebar-right" => {
            let mut tree = basic();
            mirror(&mut tree);
            (tree, rests)
        }
        "focus" => (
            row(
                vec![
                    group(&["sessions"]),
                    column(
                        vec![
                            group(&["workspace", "files", "review"]),
                            group(&["terminal"]),
                        ],
                        &[3., 1.],
                    ),
                ],
                &[1., 4.6],
            ),
            vec!["terminal"],
        ),
        "terminal-deck" => (
            column(
                vec![
                    row(
                        vec![
                            group(&["sessions"]),
                            group(&["workspace"]),
                            group(&["files", "review"]),
                        ],
                        &[1., 3.2, 1.2],
                    ),
                    group(&["terminal"]),
                ],
                &[3., 1.],
            ),
            rests,
        ),
        "quad" => (
            column(
                vec![
                    row(
                        vec![group(&["sessions", "files"]), group(&["workspace"])],
                        &[1., 3.],
                    ),
                    row(vec![group(&["terminal"]), group(&["review"])], &[1.4, 1.]),
                ],
                &[3., 1.],
            ),
            vec![],
        ),
        _ => return None,
    })
}
pub fn right_edge(tree: &Node) -> Option<&Node> {
    let main = |n: &Node| {
        n.panes().iter().any(|p| {
            p == "workspace" || p.starts_with("session-tile:") || p.starts_with("route-tile:")
        })
    };
    let Node::Split {
        orientation,
        children,
        ..
    } = tree
    else {
        return None;
    };
    let row = if *orientation == Orientation::Row {
        tree
    } else {
        children.iter().find(|n| {
            matches!(
                n,
                Node::Split {
                    orientation: Orientation::Row,
                    ..
                }
            ) && main(n)
        })?
    };
    let Node::Split { children, .. } = row else {
        return None;
    };
    let edge = children.last()?;
    (!main(edge) && children.iter().any(main)).then_some(edge)
}
pub fn geometry(tree: Node) -> Option<Node> {
    match tree {
        Node::Group {
            id,
            panes,
            active,
            minimized,
            tab_strip,
        } => Node::Group {
            id,
            panes: panes
                .into_iter()
                .filter(|p| {
                    !["session-tile:", "preview-tile:", "route-tile:"]
                        .iter()
                        .any(|prefix| p.starts_with(prefix))
                })
                .collect(),
            active,
            minimized,
            tab_strip,
        }
        .normalize(),
        Node::Split {
            id,
            orientation,
            children,
            weights,
        } => {
            let mut out = vec![];
            let mut sizes = vec![];
            for (i, child) in children.into_iter().enumerate() {
                if let Some(n) = geometry(child) {
                    out.push(n);
                    sizes.push(weights.get(i).copied().unwrap_or(1.));
                }
            }
            Node::Split {
                id,
                orientation,
                children: out,
                weights: sizes,
            }
            .normalize()
        }
    }
}
fn siblings(tree: &Node, pane: &str) -> Option<Vec<String>> {
    match tree {
        Node::Group { panes, .. } if panes.iter().any(|p| p == pane) => Some(panes.clone()),
        Node::Split { children, .. } => children.iter().find_map(|n| siblings(n, pane)),
        _ => None,
    }
}
fn group_of(tree: &Node, pane: &str) -> Option<String> {
    match tree {
        Node::Group { id, panes, .. } if panes.iter().any(|p| p == pane) => Some(id.clone()),
        Node::Split { children, .. } => children.iter().find_map(|n| group_of(n, pane)),
        _ => None,
    }
}
fn selections(tree: &Node, out: &mut Vec<(String, String, bool)>) {
    match tree {
        Node::Group {
            id,
            active,
            minimized,
            ..
        } => out.push((id.clone(), active.clone(), *minimized)),
        Node::Split { children, .. } => {
            for child in children {
                selections(child, out);
            }
        }
    }
}
pub fn adopt(mut next: Node, previous: &Node) -> Node {
    let mut selected = vec![];
    selections(&next, &mut selected);
    for pane in previous.panes() {
        if next.panes().contains(&pane) {
            continue;
        }
        let target = siblings(previous, &pane)
            .unwrap_or_default()
            .iter()
            .find_map(|s| group_of(&next, s))
            .or_else(|| group_of(&next, "workspace"))
            .or_else(|| next.groups().first().cloned());
        if let Some(target) = target {
            next.move_pane(&pane, &target, crate::layout::Position::Center);
        }
    }
    for (id, pane, was_minimized) in selected {
        if let Some(Node::Group {
            active,
            minimized,
            panes,
            ..
        }) = next.find_mut(&id)
        {
            if panes.contains(&pane) {
                *active = pane;
            }
            *minimized = was_minimized;
        }
    }
    next.normalize().unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reusable_decks_never_restore_foreign_live_tiles_but_applying_one_keeps_current_tiles() {
        let current = group(&["workspace", "session-tile:current", "plugin:inspector"]);
        let mut decks = Decks::default();
        decks.save("My deck", &current).unwrap();
        assert!(
            !decks.saved[0]
                .tree
                .panes()
                .iter()
                .any(|p| p.starts_with("session-tile:"))
        );
        let (mut focus, rests) = decks.get("focus").unwrap();
        for id in focus.groups() {
            if let Some(Node::Group {
                panes, minimized, ..
            }) = focus.find_mut(&id)
            {
                if panes == &["terminal".to_owned()] {
                    *minimized = true;
                }
            }
        }
        let next = adopt(focus, &current);
        assert!(next.panes().contains(&"session-tile:current".into()));
        assert!(next.panes().contains(&"plugin:inspector".into()));
        assert_eq!(
            next.panes().len(),
            next.panes().into_iter().collect::<BTreeSet<_>>().len()
        );
        assert!(rests.contains("terminal"));
        assert!(next.shows("workspace", &|_| true));
        assert!(
            !next.shows("terminal", &|_| true),
            "Adopting current panes must preserve a saved collapsed rail"
        );
    }
    #[test]
    fn named_geometry_remembers_resting_and_mirrors_simple_sidebar_without_swapping_vertical_order()
    {
        let (left, _) = builtin("sidebar-left").unwrap();
        let (right, _) = builtin("sidebar-right").unwrap();
        assert_eq!(left.panes().first().map(String::as_str), Some("sessions"));
        assert_eq!(right.panes().last().map(String::as_str), Some("sessions"));
        let mut decks = Decks::default();
        decks.save("Work", &left).unwrap();
        decks.hidden.clear();
        decks.save("Work", &right).unwrap();
        assert_eq!(decks.saved.len(), 1);
        assert!(decks.saved[0].hidden.is_empty());
        let value = serde_json::to_value(&decks).unwrap();
        let mut restored: Decks = serde_json::from_value(value).unwrap();
        restored.normalize();
        assert_eq!(restored.saved[0].tree.panes(), right.panes());
    }
}
