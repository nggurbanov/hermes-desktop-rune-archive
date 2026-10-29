use super::*;
use crate::{
    decks::{self, Snapshot, TOOLS},
    layout::Node,
};
impl Viewer {
    pub fn pane_visible(&self, pane: &str) -> bool {
        match pane {
            "workspace" => true,
            "sessions" | "hermes-bots:pane" => self.sidebar.is_open(),
            "files" | "review" | "terminal" => {
                !self.simple_mode && !self.decks.hidden.contains(pane)
            }
            _ => false,
        }
    }
    pub fn pane_shown(&self, pane: &str) -> bool {
        self.layout.shows(pane, &|p| self.pane_visible(p))
    }
    fn deck_snapshot(&self) -> Snapshot {
        Snapshot {
            tree: self.layout.clone(),
            hidden: self.decks.hidden.clone(),
            sidebar_open: self.sidebar.is_open(),
            sidebar_width: self.sidebar.width(),
            inspector_width: self.inspector.width(),
            active: self.decks.active.clone(),
        }
    }
    fn restore_deck(&mut self, snapshot: Snapshot) {
        self.layout = snapshot.tree;
        self.decks.hidden = snapshot.hidden;
        self.decks.active = snapshot.active;
        self.sidebar.set_open(snapshot.sidebar_open);
        self.sidebar
            .set_width(snapshot.sidebar_width.clamp(190., 360.));
        self.inspector
            .set_width(snapshot.inspector_width.clamp(240., 520.));
        self.update_tool_gate();
    }
    fn update_tool_gate(&mut self) {
        let panes = self.layout.panes();
        let open = !self.simple_mode
            && TOOLS
                .iter()
                .any(|p| !self.decks.hidden.contains(*p) && panes.iter().any(|id| id == p));
        self.inspector.set_open(open);
        self.decks.tool_gate = open;
        if !self.simple_mode {
            self.advanced_tools_open = open;
        }
    }
    pub(super) fn sync_tool_gate(&mut self) {
        let open = self.inspector.is_open();
        if self.simple_mode {
            self.inspector.set_open(false);
            self.decks.tool_gate = false;
            return;
        }
        if open != self.decks.tool_gate {
            for pane in TOOLS {
                if open {
                    self.decks.hidden.remove(pane);
                } else {
                    self.decks.hidden.insert(pane.into());
                }
            }
            self.decks.tool_gate = open;
            self.advanced_tools_open = open;
        }
    }
    pub fn reveal_pane(&mut self, pane: &str) {
        self.layout.open(pane);
        self.decks.hidden.remove(pane);
        if pane == "sessions" {
            self.sidebar.set_open(true);
        }
        self.update_tool_gate();
    }
    pub fn toggle_sidebar(&mut self) {
        let mut restored = false;
        for id in self.layout.groups() {
            if let Some(Node::Group {
                panes, minimized, ..
            }) = self.layout.find_mut(&id)
            {
                if panes.iter().any(|p| p == "sessions") && *minimized {
                    *minimized = false;
                    restored = true;
                }
            }
        }
        if restored {
            self.sidebar.set_open(true);
        } else {
            self.sidebar.toggle();
        }
    }
    pub fn toggle_right_side(&mut self) {
        let Some(edge) = decks::right_edge(&self.layout).cloned() else {
            self.toggle_tool_pane("terminal");
            return;
        };
        let ids = edge.panes();
        let shown = ids.iter().any(|p| self.pane_visible(p));
        if let Node::Group { id, minimized, .. } = edge {
            if shown && !minimized {
                self.layout.minimize(&id);
            } else {
                for pane in ids {
                    self.decks.hidden.remove(&pane);
                    if pane == "sessions" {
                        self.sidebar.set_open(true);
                    }
                }
                if let Some(Node::Group { minimized, .. }) = self.layout.find_mut(&id) {
                    *minimized = false;
                }
            }
        } else {
            for pane in ids {
                if pane == "sessions" {
                    self.sidebar.set_open(!shown);
                } else if shown {
                    self.decks.hidden.insert(pane);
                } else {
                    self.decks.hidden.remove(&pane);
                    self.layout.open(&pane);
                }
            }
        }
        self.update_tool_gate();
    }
    pub fn toggle_tool_pane(&mut self, pane: &str) {
        if !TOOLS.contains(&pane) {
            return;
        }
        if self.pane_visible(pane) && self.layout.shows(pane, &|p| self.pane_visible(p)) {
            let mut collapsed = false;
            if pane == "terminal" {
                for id in self.layout.groups() {
                    if let Some(Node::Group {
                        panes, minimized, ..
                    }) = self.layout.find_mut(&id)
                    {
                        if panes.len() == 1 && panes[0] == pane {
                            *minimized = true;
                            collapsed = true;
                        }
                    }
                }
            }
            if !collapsed {
                self.decks.hidden.insert(pane.into());
            }
        } else {
            self.layout.open(pane);
            self.decks.hidden.remove(pane);
        }
        self.update_tool_gate();
    }
    pub fn toggle_all_tools(&mut self) {
        if self.inspector.is_open() {
            for p in TOOLS {
                self.decks.hidden.insert(p.into());
            }
        } else {
            for p in TOOLS {
                self.layout.open(p);
                self.decks.hidden.remove(p);
            }
        }
        self.update_tool_gate();
    }
    pub fn set_layout_mode(&mut self, simple: bool) {
        if simple == self.simple_mode {
            return;
        }
        if !self.simple_mode {
            self.sync_tool_gate();
        }
        let snapshot = self.deck_snapshot();
        if self.simple_mode {
            self.decks.simple = Some(snapshot);
        } else {
            self.decks.advanced = Some(snapshot);
        }
        self.simple_mode = simple;
        let target = if simple {
            self.decks.simple.clone()
        } else {
            self.decks.advanced.clone()
        };
        if let Some(target) = target {
            self.restore_deck(target);
        } else {
            let restore_tools = self.advanced_tools_open;
            let id = if simple { "sidebar-left" } else { "default" };
            self.apply_layout_preset(id);
            if !simple && !restore_tools {
                for p in TOOLS {
                    self.decks.hidden.insert(p.into());
                }
                self.update_tool_gate();
            }
        }
    }
    pub fn apply_layout_preset(&mut self, id: &str) {
        let Some((tree, resting)) = self.decks.get(id) else {
            self.client_error = Some("This layout is unavailable".into());
            return;
        };
        let Some(tree) =
            decks::geometry(tree).filter(|n| n.panes().iter().any(|p| p == "workspace"))
        else {
            self.client_error = Some("This layout contains no conversation pane".into());
            return;
        };
        let declared = tree.panes();
        self.layout = decks::adopt(tree, &self.layout);
        for pane in &declared {
            if resting.contains(pane) {
                self.decks.hidden.insert(pane.clone());
            } else {
                self.decks.hidden.remove(pane);
            }
        }
        // A resting terminal occupies its own collapsed rail in the original
        // decks. The other hidden tools keep their slots without painting them.
        if !self.simple_mode && resting.contains("terminal") {
            for id in self.layout.groups() {
                if let Some(Node::Group {
                    panes, minimized, ..
                }) = self.layout.find_mut(&id)
                {
                    if panes.len() == 1 && panes[0] == "terminal" {
                        *minimized = true;
                        self.decks.hidden.remove("terminal");
                    }
                }
            }
        }
        if declared.iter().any(|p| p == "sessions") {
            self.sidebar.set_open(!resting.contains("sessions"));
        }
        self.sidebar.set_width(237.);
        self.inspector.set_width(300.);
        self.decks.active = id.into();
        self.update_tool_gate();
    }
    pub fn save_layout_preset(&mut self) {
        if !self.preferences_writable && !self.fixture {
            self.client_error = Some(
                "The saved client preferences are read-only; their file has been preserved".into(),
            );
            return;
        }
        let hidden = self.decks.hidden.clone();
        if !self.sidebar.is_open() {
            self.decks.hidden.insert("sessions".into());
        }
        for id in self.layout.groups() {
            if let Some(Node::Group {
                panes,
                minimized: true,
                ..
            }) = self.layout.find_mut(&id)
            {
                self.decks.hidden.extend(panes.clone());
            }
        }
        let result = self.decks.save(&self.layout_name.text, &self.layout);
        self.decks.hidden = hidden;
        match result {
            Ok(()) => {
                self.layout_name.replace_text("");
                match self.save_client_preferences() {
                    Ok(()) => {
                        self.client_notice = Some(
                            if self.fixture {
                                "Layout saved for this demo"
                            } else {
                                "Layout saved on this device"
                            }
                            .into(),
                        )
                    }
                    Err(error) => self.client_error = Some(error),
                }
            }
            Err(error) => self.client_error = Some(error),
        }
    }
    pub fn delete_layout_preset(&mut self, id: &str) {
        self.decks.delete(id);
    }
}
