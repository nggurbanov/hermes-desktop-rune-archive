//! Vault metadata stays readable; passwords and payment data only enter scoped RPCs.
use crate::pages::{PageEffect, PageRequest};
use rune::TextEdit;
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Clone)]
pub struct Source {
    pub name: String,
    pub label: String,
    pub installed: bool,
    pub enabled: bool,
    pub unlocked: bool,
    pub needs_unlock: bool,
}
#[derive(Clone)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub origin: String,
    pub identifier: String,
    pub created: String,
    pub backend: String,
    pub otp: bool,
}
#[derive(Clone, Copy)]
pub struct Input {
    pub key: &'static str,
    pub label: &'static str,
    pub secure: bool,
}
pub fn fields(kind: &str) -> Vec<Input> {
    let keys = match kind {
        "payment" => vec![
            ("card_number", "Card number", true),
            ("cardholder_name", "Cardholder name", false),
            ("exp_month", "Expiration month", false),
            ("exp_year", "Expiration year", false),
            ("cvc", "CVC", true),
            ("billing_postal_code", "Billing postal code", false),
        ],
        "address" => vec![
            ("address_line1", "Address line 1", false),
            ("address_line2", "Address line 2", false),
            ("city", "City", false),
            ("state", "State / region", false),
            ("postal_code", "Postal code", false),
            ("country", "Country", false),
        ],
        _ => vec![
            ("identifier", "Email / phone / username", false),
            ("password", "Password", true),
            ("otp_secret", "One-time password secret (optional)", true),
        ],
    };
    keys.into_iter()
        .map(|(key, label, secure)| Input { key, label, secure })
        .collect()
}
pub struct Vault {
    pub sources: Vec<Source>,
    pub items: Vec<Item>,
    pub loading: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub adding: bool,
    pub kind: String,
    pub label: TextEdit,
    pub origin: TextEdit,
    pub identifier_type: String,
    pub inputs: BTreeMap<String, TextEdit>,
    pub open: Option<String>,
    pub unlock: Option<String>,
    pub password: TextEdit,
    pub confirm_delete: Option<String>,
    expected: Option<Value>,
    detached: TextEdit,
}
impl Vault {
    pub fn new() -> Self {
        let mut inputs = BTreeMap::new();
        for kind in ["login", "payment", "address"] {
            for f in fields(kind) {
                inputs.insert(f.key.into(), TextEdit::new(""));
            }
        }
        Self {
            sources: vec![],
            items: vec![],
            loading: false,
            busy: false,
            error: None,
            notice: None,
            adding: false,
            kind: "login".into(),
            label: TextEdit::new(""),
            origin: TextEdit::new(""),
            identifier_type: "email".into(),
            inputs,
            open: None,
            unlock: None,
            password: TextEdit::new(""),
            confirm_delete: None,
            expected: None,
            detached: TextEdit::new(""),
        }
    }
    pub fn dismiss(&mut self) {
        self.adding = false;
        self.unlock = None;
        self.password.replace_text("");
        self.label.replace_text("");
        self.origin.replace_text("");
        for e in self.inputs.values_mut() {
            e.replace_text("");
        }
        self.confirm_delete = None;
        self.open = None;
    }
    pub fn has_draft(&self) -> bool {
        self.adding || self.unlock.is_some()
    }
    pub fn edit(&mut self, key: &str) -> &mut TextEdit {
        self.inputs.get_mut(key).unwrap_or(&mut self.detached)
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        self.loading = true;
        self.error = None;
        vec![
            (
                "settings.vault.items".into(),
                PageRequest::rpc("vault.list", json!({})),
            ),
            (
                "settings.vault.sources".into(),
                PageRequest::rpc("vault.sources", json!({})),
            ),
        ]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let v = match result {
            Ok(v) => v,
            Err(e) => {
                self.loading = false;
                if op == "settings.vault.write" {
                    self.busy = false;
                    self.password.replace_text("");
                }
                self.error = Some(e);
                return vec![];
            }
        };
        match op {
            "settings.vault.items" => {
                self.loading = false;
                let Some(items) = v["items"].as_array() else {
                    self.error = Some("Invalid vault item list".into());
                    return vec![];
                };
                self.items = items
                    .iter()
                    .filter_map(|i| {
                        Some(Item {
                            id: i["id"].as_str()?.into(),
                            kind: i["kind"].as_str().unwrap_or("").into(),
                            label: i["label"].as_str().unwrap_or("").into(),
                            origin: i["origin"].as_str().unwrap_or("").into(),
                            identifier: i["identifier"].as_str().unwrap_or("").into(),
                            created: i["created_at"].as_str().unwrap_or("").into(),
                            backend: i["backend"].as_str().unwrap_or("local").into(),
                            otp: i["has_otp"].as_bool().unwrap_or(false),
                        })
                    })
                    .collect();
            }
            "settings.vault.sources" => {
                let Some(sources) = v["sources"].as_array() else {
                    self.error = Some("Invalid vault source list".into());
                    return vec![];
                };
                self.sources = sources
                    .iter()
                    .filter_map(|s| {
                        Some(Source {
                            name: s["name"].as_str()?.into(),
                            label: s["display_name"].as_str().unwrap_or("").into(),
                            installed: s["installed"].as_bool().unwrap_or(false),
                            enabled: s["enabled"].as_bool().unwrap_or(false),
                            unlocked: s["unlocked"].as_bool().unwrap_or(false),
                            needs_unlock: s["needs_unlock"].as_bool().unwrap_or(false),
                        })
                    })
                    .collect();
            }
            "settings.vault.write" => {
                self.busy = false;
                self.password.replace_text("");
                if v["ok"].as_bool() == Some(false) {
                    self.error = Some("The agent refused the vault change".into());
                    return vec![];
                }
                let ok = match self.expected.take() {
                    Some(expected) => expected
                        .as_object()
                        .is_some_and(|o| o.iter().all(|(k, want)| v[k] == *want)),
                    None => v["id"].as_str().is_some_and(|s| !s.is_empty()),
                };
                if !ok {
                    self.error = Some("The agent did not confirm the vault change".into());
                    return vec![];
                }
                self.dismiss();
                self.notice = Some("Vault updated".into());
                return self.load();
            }
            _ => {}
        }
        vec![]
    }
    fn write(&mut self, method: &str, params: Value, expected: Option<Value>) -> Vec<PageEffect> {
        self.busy = true;
        self.expected = expected;
        vec![(
            "settings.vault.write".into(),
            PageRequest::rpc(method, params),
        )]
    }
    pub fn act(&mut self, a: Action) -> Vec<PageEffect> {
        if self.busy {
            return vec![];
        }
        self.error = None;
        match a {
            Action::Load => self.load(),
            Action::Add => {
                self.dismiss();
                self.adding = true;
                vec![]
            }
            Action::Dismiss => {
                self.dismiss();
                vec![]
            }
            Action::Kind(kind) => {
                if matches!(kind.as_str(), "login" | "payment" | "address") {
                    for e in self.inputs.values_mut() {
                        e.replace_text("");
                    }
                    self.kind = kind;
                    self.open = None;
                }
                vec![]
            }
            Action::IdentifierType(kind) => {
                if matches!(kind.as_str(), "email" | "phone" | "username") {
                    self.identifier_type = kind;
                    self.open = None;
                }
                vec![]
            }
            Action::Save => {
                if self.label.text.trim().is_empty() {
                    self.error = Some("Enter a label for this vault item".into());
                    return vec![];
                }
                let Ok(url) = url::Url::parse(self.origin.text.trim()) else {
                    self.error = Some("Enter an HTTP or HTTPS origin".into());
                    return vec![];
                };
                if !matches!(url.scheme(), "https" | "http")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    self.error =
                        Some("Enter an HTTP or HTTPS origin without embedded credentials".into());
                    return vec![];
                }
                if self.kind == "login"
                    && (self.inputs["identifier"].text.trim().is_empty()
                        || self.inputs["password"].text.is_empty())
                {
                    self.error = Some("Enter a login identifier and password".into());
                    return vec![];
                }
                let mut secret = serde_json::Map::new();
                for f in fields(&self.kind) {
                    let e = &self.inputs[f.key].text;
                    let value = if f.key == "card_number" {
                        e.chars().filter(|c| !c.is_whitespace()).collect::<String>()
                    } else if matches!(f.key, "password" | "cvc") {
                        e.clone()
                    } else {
                        e.trim().into()
                    };
                    if !value.is_empty()
                        || !matches!(f.key, "otp_secret" | "address_line2" | "state")
                    {
                        secret.insert(f.key.into(), json!(value));
                    }
                }
                if self.kind == "login" {
                    secret.insert("identifier_type".into(), json!(self.identifier_type));
                }
                let params = json!({"kind":self.kind,"label":self.label.text.trim(),"origin":self.origin.text.trim(),"secret":secret});
                for f in fields(&self.kind) {
                    if f.secure {
                        self.inputs.get_mut(f.key).unwrap().replace_text("");
                    }
                }
                self.write("vault.add", params, None)
            }
            Action::Delete(id) => {
                if self
                    .items
                    .iter()
                    .any(|i| i.id == id && i.backend == "local")
                {
                    self.confirm_delete = Some(id);
                }
                vec![]
            }
            Action::ConfirmDelete => {
                let Some(id) = self.confirm_delete.take() else {
                    return vec![];
                };
                self.write(
                    "vault.remove",
                    json!({"id":id}),
                    Some(json!({"removed":true})),
                )
            }
            Action::CancelDelete => {
                self.confirm_delete = None;
                vec![]
            }
            Action::Toggle(name, enabled) => {
                if !self.sources.iter().any(|s| s.name == name && s.installed) {
                    return vec![];
                }
                self.write(
                    "vault.source.set",
                    json!({"name":name,"enabled":enabled}),
                    Some(json!({"enabled":enabled})),
                )
            }
            Action::Lock(name) => self.write(
                "vault.lock",
                json!({"name":name}),
                Some(json!({"locked":true})),
            ),
            Action::Unlock(name) => {
                self.password.replace_text("");
                self.unlock = Some(name);
                vec![]
            }
            Action::SubmitUnlock => {
                let Some(name) = self.unlock.clone() else {
                    return vec![];
                };
                if self.password.text.is_empty() {
                    self.error = Some("Enter the vault master password".into());
                    return vec![];
                }
                let password = self.password.text.clone();
                self.password.replace_text("");
                self.write(
                    "vault.unlock",
                    json!({"name":name,"password":password}),
                    Some(json!({"unlocked":true})),
                )
            }
        }
    }
}
#[derive(Clone)]
pub enum Action {
    Load,
    Add,
    Dismiss,
    Kind(String),
    IdentifierType(String),
    Save,
    Delete(String),
    ConfirmDelete,
    CancelDelete,
    Toggle(String, bool),
    Lock(String),
    Unlock(String),
    SubmitUnlock,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_secret_only_enters_scoped_rpc_and_is_wiped_from_draft() {
        let mut v = Vault::new();
        v.act(Action::Add);
        v.label.replace_text("Example");
        v.origin.replace_text("https://example.invalid");
        v.edit("identifier").replace_text("sample@example.invalid");
        v.edit("password").replace_text("secret-value");
        v.edit("otp_secret").replace_text("otp-value");
        let e = v.act(Action::Save);
        assert_eq!(e[0].1.path, "vault.add");
        assert_eq!(
            e[0].1.body["secret"],
            json!({"identifier":"sample@example.invalid","identifier_type":"email","password":"secret-value","otp_secret":"otp-value"})
        );
        assert_eq!(v.edit("password").text, "");
        assert_eq!(v.edit("otp_secret").text, "");
        assert!(v.expected.is_none());
        v.apply("settings.vault.write", Ok(json!({"id":"saved-id"})));
        assert!(!v.adding);
        assert!(v.notice.is_some());
    }
    #[test]
    fn failed_unlock_clears_master_password_and_never_claims_unlock() {
        let mut v = Vault::new();
        v.act(Action::Unlock("bitwarden".into()));
        v.password.replace_text("master");
        let e = v.act(Action::SubmitUnlock);
        assert_eq!(e[0].1.body, json!({"name":"bitwarden","password":"master"}));
        assert_eq!(v.password.text, "");
        v.apply("settings.vault.items", Err("Read failed".into()));
        assert!(v.busy);
        v.apply("settings.vault.write", Ok(json!({"unlocked":false})));
        assert!(!v.busy);
        assert!(v.error.is_some());
        assert!(v.notice.is_none());
        assert_eq!(v.unlock.as_deref(), Some("bitwarden"));
    }
    #[test]
    fn vault_origins_are_required_and_local_delete_is_confirmed() {
        let mut v = Vault::new();
        v.label.replace_text("Card");
        v.kind = "payment".into();
        v.origin.replace_text("file:///tmp/card");
        assert!(v.act(Action::Save).is_empty());
        v.apply("settings.vault.items",Ok(json!({"items":[{"id":"local","backend":"local"},{"id":"remote","backend":"bitwarden"}]})));
        assert!(v.act(Action::Delete("remote".into())).is_empty());
        assert!(v.confirm_delete.is_none());
        assert!(v.act(Action::Delete("local".into())).is_empty());
        assert_eq!(
            v.act(Action::ConfirmDelete)[0].1.body,
            json!({"id":"local"})
        );
    }
}
