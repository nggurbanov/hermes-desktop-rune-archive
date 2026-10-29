use crate::composer::DraftScope;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Approval,
    BackgroundDone,
    Credits,
    Input,
    Plugin,
    TurnDone,
    TurnError,
}
impl Kind {
    pub const ALL: [Self; 7] = [
        Self::Approval,
        Self::BackgroundDone,
        Self::Credits,
        Self::Input,
        Self::Plugin,
        Self::TurnDone,
        Self::TurnError,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Approval => "approval",
            Self::BackgroundDone => "backgroundDone",
            Self::Credits => "credits",
            Self::Input => "input",
            Self::Plugin => "plugin",
            Self::TurnDone => "turnDone",
            Self::TurnError => "turnError",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Approval => "Approval requests",
            Self::BackgroundDone => "Background tasks",
            Self::Credits => "Credits and access",
            Self::Input => "Input needed",
            Self::Plugin => "Plugin notifications",
            Self::TurnDone => "Completed replies",
            Self::TurnError => "Turn errors",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Approval => {
                "When another conversation needs permission, or Hermes is in the background."
            }
            Self::Input => {
                "When another conversation needs your answer, or Hermes is in the background."
            }
            Self::Credits => "Urgent account access changes while Hermes is in the background.",
            Self::Plugin => "Plugin notices while Hermes is in the background.",
            Self::BackgroundDone => {
                "Tasks in your active conversation while Hermes is in the background."
            }
            Self::TurnDone => {
                "Replies in your active conversation while Hermes is in the background."
            }
            Self::TurnError => {
                "Errors in your active conversation while Hermes is in the background."
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct PromptAction {
    pub prompt_id: String,
    pub execution_generation: u64,
    pub answer: String,
}
#[derive(Clone, Debug)]
pub struct Button {
    pub label: String,
    pub action: PromptAction,
}
#[derive(Clone, Debug)]
pub struct PluginButton {
    pub id: String,
    pub label: String,
    pub activate: Option<String>,
}
#[derive(Clone, Debug)]
pub struct PluginTarget {
    pub lease: Option<crate::connections::LeaseGuard>,
    pub window_token: String,
    pub lifecycle_key: String,
    pub source_id: String,
    pub source_principal: String,
    pub profile: String,
    pub request_id: String,
    pub notify_id: Option<String>,
    pub tag: String,
    pub activate: Option<String>,
    pub actions: Vec<PluginButton>,
    pub icon: Option<String>,
    pub background: bool,
}
#[derive(Clone, Debug)]
pub struct PluginActivation {
    pub target: PluginTarget,
    pub action_id: Option<String>,
}
#[derive(Clone, Debug)]
pub struct PluginDelivery {
    pub request_id: String,
    pub target: PluginTarget,
    pub result: Result<bool, String>,
}
#[derive(Clone, Debug)]
pub struct Notice {
    pub plugin: Option<PluginTarget>,
    pub kind: Kind,
    pub scope: DraftScope,
    pub runtime: String,
    pub title: String,
    pub body: String,
    pub global: bool,
    pub tag: Option<String>,
    pub silent: bool,
    pub buttons: Vec<Button>,
}
#[derive(Clone, Debug)]
pub struct Activation {
    pub plugin: Option<PluginActivation>,
    pub principal: String,
    pub scope: DraftScope,
    pub runtime: String,
    /// Never forwarded to a fallback window after the originating client closes.
    pub action: Option<PromptAction>,
    pub token: Option<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Availability {
    NotStarted,
    Checking,
    Ready { actions: bool },
    Unavailable(String),
}
impl Availability {
    pub fn label(&self) -> String {
        match self {
            Self::NotStarted => "Native notifications have not started".into(),
            Self::Checking => "Checking the desktop notification service…".into(),
            Self::Ready { actions: true } => "Connected to the desktop notification service".into(),
            Self::Ready { actions: false } => {
                "Connected; this desktop does not offer notification action buttons".into()
            }
            Self::Unavailable(error) => error.clone(),
        }
    }
}
pub enum Action {
    Enabled(bool),
    Kind(Kind, bool),
    Test,
    Refresh,
}

#[derive(Clone)]
pub(super) struct Request {
    pub id: u64,
    pub client: u64,
    pub principal: String,
    pub notice: Notice,
    pub test: bool,
}
impl Request {
    pub fn key(&self) -> String {
        serde_json::to_string(&(
            &self.principal,
            &self.notice.scope.profile,
            &self.notice.scope.session,
            self.notice.kind.id(),
            &self.notice.runtime,
            &self.notice.tag,
            self.notice
                .plugin
                .as_ref()
                .map(|p| (&p.window_token, &p.lifecycle_key, &p.notify_id)),
        ))
        .expect("notification key")
    }
}
pub(super) enum Reply {
    PluginClosed(PluginTarget),
    Availability(Availability),
    Delivered { id: u64, result: Result<(), String> },
    Activated(Activation),
}
