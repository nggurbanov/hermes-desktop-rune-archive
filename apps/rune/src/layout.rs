//! The retained Hermes split/group tree; operations keep pane identity unique.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Row,
    Column,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Node {
    Split {
        id: String,
        orientation: Orientation,
        children: Vec<Node>,
        weights: Vec<f32>,
    },
    Group {
        id: String,
        panes: Vec<String>,
        active: String,
        #[serde(default)]
        minimized: bool,
        #[serde(default, rename = "tabStrip", skip_serializing_if = "Option::is_none")]
        tab_strip: Option<String>,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
impl Node {
    pub fn group(panes: Vec<String>) -> Self {
        Self::Group {
            id: id(),
            active: panes.first().cloned().unwrap_or_default(),
            panes,
            minimized: false,
            tab_strip: None,
        }
    }
    pub fn split(orientation: Orientation, children: Vec<Node>, weights: Vec<f32>) -> Self {
        Self::Split {
            id: id(),
            orientation,
            children,
            weights,
        }
    }
    pub fn id(&self) -> &str {
        match self {
            Self::Split { id, .. } | Self::Group { id, .. } => id,
        }
    }
    pub fn panes(&self) -> Vec<String> {
        match self {
            Self::Group { panes, .. } => panes.clone(),
            Self::Split { children, .. } => children.iter().flat_map(Self::panes).collect(),
        }
    }
    pub fn shows(&self, pane: &str, visible: &impl Fn(&str) -> bool) -> bool {
        match self {
            Self::Group {
                panes,
                active,
                minimized,
                ..
            } => {
                if *minimized {
                    return false;
                }
                let selected = panes
                    .iter()
                    .find(|p| *p == active && visible(p))
                    .or_else(|| panes.iter().find(|p| visible(p)));
                selected.is_some_and(|p| p == pane)
            }
            Self::Split { children, .. } => children.iter().any(|n| n.shows(pane, visible)),
        }
    }
    pub fn groups(&self) -> Vec<String> {
        match self {
            Self::Group { id, .. } => vec![id.clone()],
            Self::Split { children, .. } => children.iter().flat_map(Self::groups).collect(),
        }
    }
    pub fn find_mut(&mut self, key: &str) -> Option<&mut Self> {
        if self.id() == key {
            return Some(self);
        }
        match self {
            Self::Split { children, .. } => children.iter_mut().find_map(|n| n.find_mut(key)),
            _ => None,
        }
    }
    pub fn normalize(self) -> Option<Self> {
        fn walk(
            node: Node,
            seen: &mut std::collections::BTreeSet<String>,
            depth: usize,
        ) -> Option<Node> {
            if depth > 32 {
                return None;
            }
            match node {
                Node::Group {
                    id,
                    panes,
                    active,
                    minimized,
                    tab_strip,
                } => {
                    let panes: Vec<_> = panes
                        .into_iter()
                        .filter(|p| !p.is_empty() && p.len() < 512 && seen.insert(p.clone()))
                        .collect();
                    if panes.is_empty() {
                        return None;
                    }
                    let active = if panes.contains(&active) {
                        active
                    } else {
                        panes[0].clone()
                    };
                    Some(Node::Group {
                        id,
                        panes,
                        active,
                        minimized,
                        tab_strip: tab_strip.filter(|s| s == "always" || s == "never"),
                    })
                }
                Node::Split {
                    id,
                    orientation,
                    children,
                    weights,
                } => {
                    let weights: Vec<_> = (0..children.len())
                        .map(|i| {
                            weights
                                .get(i)
                                .copied()
                                .filter(|w| w.is_finite() && *w > 0.)
                                .unwrap_or(1.)
                        })
                        .collect();
                    let mut out = vec![];
                    let mut sizes = vec![];
                    for (child, weight) in children.into_iter().zip(weights) {
                        if let Some(child) = walk(child, seen, depth + 1) {
                            match child {
                                Node::Split {
                                    orientation: axis,
                                    children,
                                    weights,
                                    ..
                                } if axis == orientation => {
                                    let sum: f32 = weights.iter().sum();
                                    for (n, w) in children.into_iter().zip(weights) {
                                        out.push(n);
                                        sizes.push(weight * w / sum);
                                    }
                                }
                                other => {
                                    out.push(other);
                                    sizes.push(weight);
                                }
                            }
                        }
                    }
                    if out.is_empty() {
                        None
                    } else if out.len() == 1 {
                        out.pop()
                    } else {
                        Some(Node::Split {
                            id,
                            orientation,
                            children: out,
                            weights: sizes,
                        })
                    }
                }
            }
        }
        walk(self, &mut Default::default(), 0)
    }
    fn remove(self, pane: &str) -> Option<Self> {
        match self {
            Self::Group {
                id,
                mut panes,
                active,
                minimized,
                tab_strip,
            } => {
                panes.retain(|p| p != pane);
                Self::Group {
                    id,
                    panes,
                    active,
                    minimized,
                    tab_strip,
                }
                .normalize()
            }
            Self::Split {
                id,
                orientation,
                children,
                weights,
            } => {
                let mut out = vec![];
                let mut sizes = vec![];
                for (n, w) in children.into_iter().zip(weights) {
                    if let Some(n) = n.remove(pane) {
                        out.push(n);
                        sizes.push(w);
                    }
                }
                Self::Split {
                    id,
                    orientation,
                    children: out,
                    weights: sizes,
                }
                .normalize()
            }
        }
    }
    pub fn move_pane(&mut self, pane: &str, target: &str, position: Position) {
        // Removing the only pane of its destination would make that target disappear.
        if matches!(self.find_mut(target),Some(Self::Group{panes,..})if panes.len()==1&&panes[0]==pane)
        {
            return;
        }
        let Some(mut next) = self.clone().remove(pane) else {
            return;
        };
        let Some(group) = next.find_mut(target) else {
            return;
        };
        match position {
            Position::Center => {
                if let Self::Group {
                    panes,
                    active,
                    minimized,
                    ..
                } = group
                {
                    panes.push(pane.into());
                    *active = pane.into();
                    *minimized = false;
                }
            }
            edge => {
                let old = group.clone();
                let new = Self::group(vec![pane.into()]);
                let before = matches!(edge, Position::Left | Position::Top);
                let axis = if matches!(edge, Position::Left | Position::Right) {
                    Orientation::Row
                } else {
                    Orientation::Column
                };
                *group = Self::split(
                    axis,
                    if before {
                        vec![new, old]
                    } else {
                        vec![old, new]
                    },
                    vec![1., 1.],
                );
            }
        }
        if let Some(next) = next.normalize() {
            *self = next;
        }
    }
    pub fn close(&mut self, pane: &str) {
        if pane == "workspace" {
            return;
        }
        if let Some(next) = self.clone().remove(pane) {
            *self = next;
        }
    }
    pub fn open(&mut self, pane: &str) {
        fn reveal(node: &mut Node, pane: &str) -> bool {
            match node {
                Node::Group {
                    panes,
                    active,
                    minimized,
                    ..
                } if panes.iter().any(|p| p == pane) => {
                    *active = pane.into();
                    *minimized = false;
                    true
                }
                Node::Split { children, .. } => children.iter_mut().any(|n| reveal(n, pane)),
                _ => false,
            }
        }
        if reveal(self, pane) {
            return;
        }

        let target = self.groups().last().cloned();
        if let Some(target) = target {
            self.move_pane(pane, &target, Position::Center);
        }
    }
    pub fn resize(&mut self, split: &str, index: usize, delta: f32) {
        if let Some(Self::Split { weights, .. }) = self.find_mut(split) {
            if index + 1 >= weights.len() {
                return;
            }
            let total = weights[index] + weights[index + 1];
            let first = (weights[index] + delta).clamp(total * 0.08, total * 0.92);
            weights[index] = first;
            weights[index + 1] = total - first;
        }
    }
    pub fn activate(&mut self, group: &str, pane: &str) {
        if let Some(Self::Group {
            panes,
            active,
            minimized,
            ..
        }) = self.find_mut(group)
        {
            if panes.iter().any(|p| p == pane) {
                *active = pane.into();
                *minimized = false;
            }
        }
    }
    pub fn minimize(&mut self, group: &str) {
        if let Some(Self::Group { minimized, .. }) = self.find_mut(group) {
            *minimized = !*minimized;
        }
    }
    pub fn strip(&mut self, group: &str) {
        if let Some(Self::Group { tab_strip, .. }) = self.find_mut(group) {
            *tab_strip = match tab_strip.as_deref() {
                None => Some("always".into()),
                Some("always") => Some("never".into()),
                _ => None,
            };
        }
    }
}
impl Default for Node {
    fn default() -> Self {
        Self::split(
            Orientation::Row,
            vec![
                Self::group(vec!["sessions".into()]),
                Self::group(vec!["workspace".into()]),
                Self::split(
                    Orientation::Column,
                    vec![
                        Self::split(
                            Orientation::Row,
                            vec![
                                Self::group(vec!["review".into()]),
                                Self::group(vec!["files".into()]),
                            ],
                            vec![1., 1.2],
                        ),
                        Self::group(vec!["terminal".into()]),
                    ],
                    vec![1.6, 1.],
                ),
            ],
            vec![237., 903., 300.],
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_tabs_and_folded_groups_do_not_count_as_visible_chats() {
        let mut tree = Node::group(vec!["workspace".into(), "files".into()]);
        let group = tree.id().to_owned();
        tree.activate(&group, "files");
        assert!(!tree.shows("workspace", &|_| true));
        assert!(tree.shows("workspace", &|p| p != "files"));
        tree.activate(&group, "workspace");
        assert!(tree.shows("workspace", &|_| true));
        tree.minimize(&group);
        assert!(!tree.shows("workspace", &|_| true));
    }
    #[test]
    fn docking_preserves_every_pane_once_and_removes_empty_zones() {
        let mut tree = Node::default();
        let before: std::collections::BTreeSet<_> = tree.panes().into_iter().collect();
        let target = tree.groups()[1].clone();
        tree.move_pane("terminal", &target, Position::Center);
        tree.move_pane("files", &target, Position::Bottom);
        let after: std::collections::BTreeSet<_> = tree.panes().into_iter().collect();
        assert_eq!(before, after);
        assert_eq!(tree.panes().len(), before.len());
        let json = serde_json::to_value(&tree).unwrap();
        assert_eq!(
            serde_json::from_value::<Node>(json).unwrap().normalize(),
            Some(tree)
        );
    }
    #[test]
    fn malformed_weights_and_duplicate_panes_are_repaired_without_nan() {
        let tree = Node::split(
            Orientation::Row,
            vec![
                Node::group(vec!["workspace".into()]),
                Node::group(vec!["workspace".into(), "files".into()]),
            ],
            vec![f32::NAN, -1.],
        )
        .normalize()
        .unwrap();
        assert_eq!(tree.panes(), vec!["workspace", "files"]);
        assert!(serde_json::to_string(&tree).is_ok());
    }
}
