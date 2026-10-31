//! Agent-independent Legacy routing forms. Only actual worker receipts confirm a save.
mod form;
use crate::connections::{
    Connections,
    legacy::{AuthMode, SaveTicket, Scope, Snapshot},
};
pub(crate) use form::{Field, Form, FormMode, Header, SecretChoice};
use rune::TextEdit;
use std::sync::mpsc::{Receiver, TryRecvError};
#[cfg(test)]
mod tests;
#[derive(Clone)]
pub(crate) enum Change {
    Mode(FormMode),
    Auth(AuthMode),
    Token(SecretChoice),
    HeaderSecret(u64, SecretChoice),
    AddHeader,
    RemoveHeader(u64),
    Save,
    RemoveOverride,
    Login,
    CancelLogin,
}
#[derive(Clone)]
pub(crate) enum Action {
    Reload,
    Scope(Scope),
    EditProfile,
    Discard,
    Form { generation: u64, change: Change },
}
struct Login {
    scope: Scope,
    generation: u64,
    expected: crate::connections::legacy::ScopeVersion,
    profile_name: String,
    attempt: crate::connections::LoginAttempt,
    authorize_url: Option<String>,
}
struct Read {
    generation: u64,
    scope: Scope,
    receiver: Receiver<Result<Snapshot, String>>,
}
struct Save {
    generation: u64,
    scope: Scope,
    ticket: SaveTicket,
}
pub(crate) struct Settings {
    pub scope: Scope,
    pub profile_name: TextEdit,
    pub form: Option<Form>,
    pub error: Option<String>,
    pub notice: Option<String>,
    generation: u64,
    source_epoch: Option<u64>,
    attempted: bool,
    read: Option<Read>,
    save: Option<Save>,
    snapshot: Option<Snapshot>,
    discard: TextEdit,
    login: Option<Login>,
    retired_logins: Vec<crate::connections::LoginAttempt>,
    login_receipt: Option<(Scope, crate::connections::legacy::ScopeVersion)>,
}
impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}
impl Settings {
    pub(crate) fn new() -> Self {
        Self {
            scope: Scope::Global,
            profile_name: TextEdit::new(""),
            form: None,
            error: None,
            notice: None,
            generation: 0,
            source_epoch: None,
            attempted: false,
            read: None,
            save: None,
            snapshot: None,
            discard: TextEdit::new(""),
            login: None,
            retired_logins: Vec::new(),
            login_receipt: None,
        }
    }
    pub(crate) fn observe_source(&mut self, epoch: u64) {
        if self.source_epoch.is_some_and(|previous| previous != epoch) {
            self.cancel_login();
        }
        self.source_epoch = Some(epoch);
    }
    pub(crate) fn cancel_login(&mut self) {
        self.login_receipt = None;
        if let Some(login) = self.login.take() {
            login.attempt.cancel();
            self.retired_logins.push(login.attempt);
        }
    }
    pub(crate) fn login_pending(&self) -> bool {
        self.login.is_some()
    }
    pub(crate) fn login_work_pending(&self) -> bool {
        self.login.is_some() || !self.retired_logins.is_empty()
    }
    pub(crate) fn authorization_url(&self) -> Option<&str> {
        let login = self.login.as_ref()?;
        if self.loading()
            || self.pending()
            || self.dirty()
            || self.scope != login.scope
            || self.profile_name.text != login.profile_name
            || self.form.as_ref().is_none_or(|form| {
                form.generation != login.generation || form.expected() != login.expected
            })
        {
            return None;
        }
        login.authorize_url.as_deref()
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    pub(crate) fn profile_edit_mut(&mut self, generation: u64) -> &mut TextEdit {
        if let Some(login) = &self.login {
            login.attempt.hold();
        }
        if generation == self.generation && !self.pending() && !self.loading() {
            &mut self.profile_name
        } else {
            &mut self.discard
        }
    }
    pub(crate) fn loading(&self) -> bool {
        self.read.is_some()
    }
    pub(crate) fn pending(&self) -> bool {
        self.save.is_some()
    }
    pub(crate) fn dirty(&self) -> bool {
        self.form.as_ref().is_some_and(Form::dirty)
    }
    pub(crate) fn can_close(&self) -> Result<(), String> {
        if self.pending() {
            Err("Wait for the Legacy settings save to finish".into())
        } else if self.dirty() {
            Err("Save or discard the Legacy settings edits before leaving".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn ensure_loaded(&mut self, connections: &Connections) {
        if !self.attempted {
            self.action(Action::Reload, connections);
        }
    }
    pub(crate) fn profile_names(&self) -> Vec<String> {
        self.snapshot
            .as_ref()
            .map(|s| s.profiles.keys().cloned().collect())
            .unwrap_or_default()
    }
    pub(crate) fn edit_mut(&mut self, generation: u64, field: Field) -> &mut TextEdit {
        if let Some(login) = &self.login {
            login.attempt.hold();
        }
        if self.read.is_none() && self.save.is_none() {
            if let Some(form) = self.form.as_mut().filter(|f| f.generation == generation) {
                if let Some(field) = form.field_mut(field) {
                    return field;
                }
            }
        }
        &mut self.discard
    }
    pub(crate) fn action(&mut self, action: Action, connections: &Connections) {
        if let Action::Form { generation, .. } = &action {
            if self.pending()
                || self.loading()
                || self
                    .form
                    .as_ref()
                    .is_none_or(|f| f.generation != *generation)
            {
                return;
            }
        }
        if !matches!(
            &action,
            Action::Form {
                change: Change::Login,
                ..
            }
        ) {
            self.cancel_login();
        }
        self.error = None;
        self.notice = None;
        match action {
            Action::Reload => {
                if let Err(error) = self.can_close() {
                    self.error = Some(error);
                } else {
                    self.load(connections);
                }
            }
            Action::Discard => {
                if self.pending() {
                    self.error =
                        Some("Wait for the accepted save to finish before discarding".into());
                } else {
                    self.form = None;
                    self.load(connections);
                }
            }
            Action::EditProfile => {
                let scope = Scope::Profile(self.profile_name.text.trim().to_owned());
                self.action(Action::Scope(scope), connections);
            }
            Action::Scope(scope) => {
                if let Err(error) = scope.validate() {
                    self.error = Some(error);
                } else if let Err(error) = self.can_close() {
                    self.error = Some(error);
                } else {
                    self.scope = scope;
                    if let Scope::Profile(name) = &self.scope {
                        self.profile_name.replace_text(name);
                    }
                    self.form = None;
                    self.load(connections);
                }
            }
            Action::Form { generation, change } => {
                if self.pending()
                    || self.loading()
                    || self
                        .form
                        .as_ref()
                        .is_none_or(|f| f.generation != generation)
                {
                    return;
                }
                match change {
                    Change::Login => self.start_login(connections),
                    Change::CancelLogin => self.cancel_login(),
                    Change::Save => self.save(connections, false),
                    Change::RemoveOverride => {
                        if matches!(self.scope, Scope::Global) {
                            self.error = Some("The global route cannot be removed".into());
                        } else if self.dirty() {
                            self.error =
                                Some("Save or discard edits before removing the override".into());
                        } else {
                            self.save(connections, true);
                        }
                    }
                    Change::Mode(mode) => self.form.as_mut().unwrap().mode = mode,
                    Change::Auth(auth) => self.form.as_mut().unwrap().auth_mode = auth,
                    Change::Token(choice) => self.form.as_mut().unwrap().token_choice = choice,
                    Change::HeaderSecret(id, choice) => {
                        if let Some(header) = self
                            .form
                            .as_mut()
                            .unwrap()
                            .headers
                            .iter_mut()
                            .find(|h| h.id == id)
                        {
                            header.choice = choice;
                        }
                    }
                    Change::AddHeader => self.form.as_mut().unwrap().add_header(),
                    Change::RemoveHeader(id) => self.form.as_mut().unwrap().remove_header(id),
                }
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn test_load_snapshot(&mut self, snapshot: Snapshot) {
        self.install(snapshot);
    }
    #[cfg(test)]
    pub(crate) fn test_start_attempt(&mut self, attempt: crate::connections::LoginAttempt) -> bool {
        if self.loading() || self.pending() || self.dirty() || self.login.is_some() {
            return false;
        }
        let Some(form) = &self.form else { return false };
        if form.mode != FormMode::Url
            || form.auth_mode != AuthMode::Oauth
            || form.scope != self.scope
        {
            return false;
        }
        self.login = Some(Login {
            scope: self.scope.clone(),
            generation: form.generation,
            expected: form.expected(),
            profile_name: self.profile_name.text.clone(),
            attempt,
            authorize_url: None,
        });
        true
    }
    fn start_login(&mut self, connections: &Connections) {
        if self.login.is_some() || self.loading() || self.pending() || self.dirty() {
            return;
        }
        let Some(form) = self.form.as_ref() else {
            return;
        };
        if form.mode != FormMode::Url
            || form.auth_mode != AuthMode::Oauth
            || form.scope != self.scope
        {
            return;
        }
        let expected = form.expected();
        let generation = form.generation;
        let scope = self.scope.clone();
        match connections.start_oauth_login(scope.clone(), expected.clone()) {
            Ok(attempt) => {
                self.login = Some(Login {
                    scope,
                    generation,
                    expected,
                    profile_name: self.profile_name.text.clone(),
                    attempt,
                    authorize_url: None,
                })
            }
            Err(error) => self.error = Some(error),
        }
    }
    fn poll_login(&mut self, connections: &Connections) -> bool {
        let Some(mut login) = self.login.take() else {
            return false;
        };
        if self.loading()
            || self.pending()
            || self.dirty()
            || self.scope != login.scope
            || self.profile_name.text != login.profile_name
            || self.form.as_ref().is_none_or(|form| {
                form.generation != login.generation || form.expected() != login.expected
            })
        {
            login.attempt.cancel();
            self.retired_logins.push(login.attempt);
            return true;
        }
        login.attempt.release();
        match login.attempt.poll() {
            Ok(None) => {
                self.login = Some(login);
                false
            }
            Ok(Some(crate::connections::LoginEvent::Authorize(url))) => {
                login.authorize_url = Some(url);
                self.login = Some(login);
                true
            }
            Ok(Some(crate::connections::LoginEvent::Complete(receipt))) => {
                if receipt.scope == login.scope
                    && self.scope == login.scope
                    && receipt.version != login.expected
                {
                    self.load(connections);
                    if self.loading() {
                        self.login_receipt = Some((receipt.scope, receipt.version));
                    }
                } else {
                    self.error = Some("The sign-in receipt did not match these settings.".into());
                }
                true
            }
            Err(error) => {
                self.retired_logins.push(login.attempt);
                self.error = Some(
                    match error {
                        crate::connections::LoginFailure::Timeout => {
                            "Sign-in timed out. Try again."
                        }
                        _ => "Sign-in was cancelled or interrupted.",
                    }
                    .into(),
                );
                true
            }
            Ok(Some(crate::connections::LoginEvent::Failed(error))) => {
                self.error = Some(match error {
                    crate::connections::LoginFailure::Cancelled => "Sign-in cancelled.",
                    crate::connections::LoginFailure::Timeout => "Sign-in timed out. Try again.",
                    crate::connections::LoginFailure::EmbeddedRequired => "This gateway requires access headers in an embedded sign-in window, which is not supported yet.",
                    crate::connections::LoginFailure::Stale => "These settings changed. Reload and try again.",
                    crate::connections::LoginFailure::Store => "The credential could not be saved. Check the credential store and try again.",
                    crate::connections::LoginFailure::Recovery => "Sign-in could not clean up its temporary credential. It remains in the cleanup ledger; check the credential store.",
                    crate::connections::LoginFailure::Unavailable => "Sign-in is unavailable for this gateway.",
                }.into());
                true
            }
        }
    }
    fn load(&mut self, connections: &Connections) {
        self.attempted = true;
        self.generation = self.generation.wrapping_add(1);
        self.notice = None;
        match connections.legacy_snapshot() {
            Ok(receiver) => {
                self.read = Some(Read {
                    generation: self.generation,
                    scope: self.scope.clone(),
                    receiver,
                })
            }
            Err(error) => {
                self.read = None;
                self.error = Some(error);
            }
        }
    }
    fn save(&mut self, connections: &Connections, remove: bool) {
        let form = self.form.as_ref().unwrap();
        match form
            .edit(remove)
            .and_then(|edit| connections.save_legacy(edit))
        {
            Ok(ticket) => {
                self.save = Some(Save {
                    generation: form.generation,
                    scope: form.scope.clone(),
                    ticket,
                });
                self.notice = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
    fn install(&mut self, snapshot: Snapshot) {
        self.generation = self.generation.wrapping_add(1);
        self.form = Some(Form::from_snapshot(
            self.scope.clone(),
            self.generation,
            &snapshot,
        ));
        self.snapshot = Some(snapshot);
    }
    pub(crate) fn poll(&mut self, connections: &Connections) -> bool {
        let mut changed = self.poll_login(connections);
        self.retired_logins.retain(|attempt| match attempt.poll_terminal() {
            None | Some(crate::connections::LoginEvent::Authorize(_)) => true,
            Some(crate::connections::LoginEvent::Failed(crate::connections::LoginFailure::Recovery)) => {
                self.error = Some("Sign-in could not clean up its temporary credential. It remains in the cleanup ledger; check the credential store.".into());
                changed = true; false
            }
            Some(_) => { changed = true; false }
        });
        if self.dirty() && self.notice.take().is_some() {
            changed = true;
        }
        if let Some(read) = self.read.take() {
            match read.receiver.try_recv() {
                Ok(result) => {
                    changed = true;
                    if read.generation == self.generation
                        && read.scope == self.scope
                        && !self.dirty()
                        && !self.pending()
                    {
                        match result {
                            Ok(snapshot) => {
                                let receipt = self.login_receipt.take();
                                let receipt_matches =
                                    receipt.as_ref().is_some_and(|(scope, version)| {
                                        scope == &self.scope
                                            && snapshot.scope_version(scope) == *version
                                    });
                                self.install(snapshot);
                                self.error = None;
                                if receipt_matches {
                                    self.notice = Some("Signed in.".into());
                                } else if receipt.is_some() {
                                    self.error = Some("These settings changed after sign-in. Review the current connection.".into());
                                }
                            }
                            Err(error) => {
                                self.login_receipt = None;
                                self.error = Some(error);
                            }
                        }
                    }
                }
                Err(TryRecvError::Empty) => self.read = Some(read),
                Err(TryRecvError::Disconnected) => {
                    changed = true;
                    if read.generation == self.generation && read.scope == self.scope {
                        self.login_receipt = None;
                        self.error =
                            Some("The Legacy settings service closed before loading".into());
                    }
                }
            }
        }
        if let Some(save) = self.save.take() {
            match save.ticket.receiver.try_recv() {
                Ok(result) => {
                    changed = true;
                    match result {
                        Ok(receipt)
                            if save.scope == receipt.scope
                                && save.scope == self.scope
                                && self
                                    .form
                                    .as_ref()
                                    .is_some_and(|f| f.generation == save.generation) =>
                        {
                            self.install(receipt.snapshot);
                            self.error = None;
                            self.notice = Some(
                                receipt
                                    .warning
                                    .map(|warning| format!("Saved. {warning}"))
                                    .unwrap_or_else(|| "Saved.".into()),
                            );
                        }
                        Ok(_) => {
                            self.error = Some("The settings save returned a different scope".into())
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Err(TryRecvError::Empty) => self.save = Some(save),
                Err(TryRecvError::Disconnected) => {
                    changed = true;
                    self.error = Some(
                        "The Legacy settings service closed before confirming the save".into(),
                    );
                }
            }
        }
        changed
    }
}
