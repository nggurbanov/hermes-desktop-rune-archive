use rune::{Key, KeyEvent, PhysicalKeyCode, Shortcut, Tx};
pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub defaults: &'static [&'static str],
}
macro_rules! c{($id:literal,$label:literal,$cat:literal,$($key:literal),*)=>{Command{id:$id,label:$label,category:$cat,defaults:&[$($key),*]}};}
const VOICE_KEYS: &[&str] = if cfg!(target_os = "macos") {
    &["ctrl+b"]
} else {
    &["mod+alt+v"]
};
pub const COMMANDS: &[Command] = &[
    c!("session.new", "New conversation", "Sessions", "mod+n"),
    c!(
        "session.next",
        "Next conversation tab",
        "Sessions",
        "ctrl+tab"
    ),
    c!(
        "session.prev",
        "Previous conversation tab",
        "Sessions",
        "ctrl+shift+tab"
    ),
    c!("nav.settings", "Settings", "Navigation", "mod+,"),
    c!(
        "nav.commandCenter",
        "Command Center",
        "Navigation",
        "mod+k",
        "mod+."
    ),
    c!("nav.capabilities", "Capabilities", "Navigation",),
    c!("nav.messaging", "Messaging", "Navigation",),
    c!("nav.artifacts", "Artifacts", "Navigation",),
    c!("nav.cron", "Scheduled jobs", "Navigation",),
    c!("nav.agents", "Bots & profiles", "Navigation",),
    c!("view.toggleSidebar", "Toggle sidebar", "View", "mod+b"),
    c!("view.toggleRightSidebar", "Toggle tools", "View", "mod+j"),
    c!(
        "view.toggleStatusbar",
        "Toggle statusbar",
        "View",
        "mod+shift+s"
    ),
    c!("view.toggleProfileRail", "Toggle profile rail", "View",),
    c!("view.toggleSimpleMode", "Toggle Simple mode", "View",),
    c!("view.toggleReview", "Toggle Review", "View", "mod+g"),
    c!("view.showFiles", "Show Files", "View",),
    c!("view.showTerminal", "Show Terminal", "View", "ctrl+`"),
    c!(
        "appearance.toggleMode",
        "Toggle light/dark",
        "View",
        "shift+x"
    ),
    c!("keybinds.openPanel", "Keyboard shortcuts", "View", "mod+/"),
    c!(
        "composer.modelPicker",
        "Choose model",
        "Composer",
        "mod+shift+m"
    ),
    c!("composer.options", "Model options", "Composer",),
    Command {
        id: "composer.voice",
        label: "Voice conversation",
        category: "Composer",
        defaults: VOICE_KEYS,
    },
    c!("composer.dictate", "Voice dictation", "Composer",),
];
pub fn canonical(value: &str) -> Option<String> {
    let mapped = value
        .split('+')
        .map(|p| {
            if p == "mod" {
                if cfg!(target_os = "macos") {
                    "cmd"
                } else {
                    "ctrl"
                }
            } else {
                p
            }
        })
        .collect::<Vec<_>>()
        .join("+");
    mapped.parse::<Shortcut>().ok().map(|s| s.to_string())
}
pub fn binding_label(value: &str) -> String {
    value
        .parse::<Shortcut>()
        .map(|s| s.keycaps().join(" + "))
        .unwrap_or_else(|_| value.into())
}
pub fn event_chord(event: &KeyEvent, physical: Option<PhysicalKeyCode>) -> Option<String> {
    if let Some(key) = physical {
        return Shortcut::from_physical_key(key, event.mods)
            .ok()
            .map(|s| s.to_string());
    }
    let key = match &event.key {
        Key::Command(c) => c.to_string(),
        Key::Text(t) if t.chars().count() == 1 => t.to_lowercase(),
        Key::Enter => "enter".into(),
        Key::Tab => "tab".into(),
        Key::Left => "left".into(),
        Key::Right => "right".into(),
        Key::Up => "up".into(),
        Key::Down => "down".into(),
        Key::PageUp => "pageup".into(),
        Key::PageDown => "pagedown".into(),
        Key::Function(n) => format!("f{n}"),
        _ => return None,
    };
    let mut parts = vec![];
    if event.mods.ctrl || !cfg!(target_os = "macos") && event.mods.cmd {
        parts.push("ctrl");
    }
    if event.mods.alt {
        parts.push("alt");
    }
    if event.mods.shift {
        parts.push("shift");
    }
    if event.mods.logo || cfg!(target_os = "macos") && event.mods.cmd {
        parts.push("super");
    }
    parts.push(&key);
    canonical(&parts.join("+"))
}
impl crate::app::Viewer {
    pub fn client_capture_key(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        let Some(id) = self.client_settings.capture.clone() else {
            return false;
        };
        if self.route != crate::pages::Route::Settings || self.settings.section != "shortcuts" {
            self.client_settings.capture = None;
            return false;
        }
        match event.key {
            Key::Escape => self.client_settings.capture = None,
            Key::Backspace | Key::Delete => {
                let save = !self.is_fixture();
                self.client_settings.set_binding(id, None, save);
            }
            _ => {
                if let Some(chord) = event_chord(event, tx.physical_key()) {
                    let save = !self.is_fixture();
                    self.client_settings.set_binding(id, Some(chord), save);
                }
            }
        }
        tx.instant();
        true
    }
    pub fn client_shortcut(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        if self.composer.popup.is_some() || self.connection_form {
            return false;
        }
        let Some(chord) = event_chord(event, tx.physical_key()) else {
            return false;
        };
        let Some(command) = self.client_settings.command(&chord, tx.focused().is_some()) else {
            return false;
        };
        if tx.key_repeat() && !matches!(command, "session.next" | "session.prev") {
            return true;
        }
        let handled = self.client_session_command(command)
            || self.client_navigation_command(command)
            || self.client_view_command(command)
            || self.client_composer_command(command);
        if !handled {
            return false;
        }
        tx.instant();
        true
    }
    fn client_session_command(&mut self, command: &str) -> bool {
        match command {
            "session.new" => self.new_draft(),
            "session.next" | "session.prev" => {
                let n = self.tabs.len();
                if n > 0 {
                    let index = self
                        .tabs
                        .iter()
                        .position(|t| {
                            t.profile == self.profile
                                && Some(&t.session) == self.selected_session.as_ref()
                        })
                        .unwrap_or(0);
                    self.activate_tab(if command == "session.next" {
                        (index + 1) % n
                    } else {
                        (index + n - 1) % n
                    });
                }
            }
            _ => return false,
        }
        true
    }
    fn client_navigation_command(&mut self, command: &str) -> bool {
        use crate::pages::Route;
        match command {
            "nav.settings" => self.open_route(Route::Settings),
            "nav.commandCenter" => self.open_route(Route::CommandCenter),
            "nav.capabilities" => self.open_route(Route::Capabilities),
            "nav.messaging" => self.open_route(Route::Messaging),
            "nav.artifacts" => self.open_route(Route::Artifacts),
            "nav.cron" => self.open_route(Route::Cron),
            "nav.agents" => self.open_route(Route::Bots),
            _ => return false,
        }
        true
    }
    fn client_view_command(&mut self, command: &str) -> bool {
        use crate::pages::Route;
        match command {
            "view.toggleSidebar" => self.toggle_sidebar(),
            "view.toggleRightSidebar" => self.toggle_right_side(),
            "view.toggleStatusbar" => self.show_statusbar = !self.show_statusbar,
            "view.toggleProfileRail" => self.show_profile_rail = !self.show_profile_rail,
            "view.toggleSimpleMode" => self.set_simple_mode(!self.simple_mode),
            "view.toggleReview" => self.toggle_tool_pane("review"),
            "view.showFiles" => self.reveal_pane("files"),
            "view.showTerminal" => self.reveal_pane("terminal"),
            "appearance.toggleMode" => self.toggle_theme(self.client_settings.system_dark.get()),
            "keybinds.openPanel" => {
                self.open_route(Route::Settings);
                self.settings_action(crate::pages::settings::Action::Section("shortcuts".into()));
            }
            _ => return false,
        }
        true
    }
    fn client_composer_command(&mut self, command: &str) -> bool {
        match command {
            "composer.modelPicker" => self.open_composer(crate::composer::Popup::Models),
            "composer.options" => self.open_composer(crate::composer::Popup::Settings),
            "composer.dictate" => self.toggle_dictation(),
            "composer.voice" => {
                if self
                    .audio
                    .view(&self.composer_scope())
                    .is_some_and(|v| v.engine.is_some() && v.status != crate::audio::Status::Idle)
                {
                    self.end_voice_chat()
                } else {
                    self.start_voice_chat()
                }
            }
            _ => return false,
        }
        true
    }
    pub fn client_settings_action(&mut self, action: super::Action) {
        let persist = !self.is_fixture();
        self.client_settings.act(action, persist);
    }
}
