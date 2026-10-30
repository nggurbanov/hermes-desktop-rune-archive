use super::*;
use crate::client::windows::SourceDescriptor;
use crate::connections::{Lease, Resolver};
#[derive(Clone)]
pub(super) struct Retained {
    pub _resolver: Resolver,
    pub lease: Lease,
}
#[derive(Clone)]
pub(super) struct Pinned {
    connection: Connection,
    route: WindowRoute,
    retained: Option<Retained>,
}
pub(super) enum Source {
    Direct(Connection),
    Saved {
        expected_principal: Option<String>,
        resolver: Resolver,
        receive: Receiver<Result<Lease, String>>,
    },
}
impl Source {
    pub fn pending(&self) -> bool {
        matches!(self, Self::Saved { .. })
    }
    pub fn capture(
        connection: Option<&Connection>,
        resolver: Option<Resolver>,
    ) -> Result<Option<Self>, String> {
        let Some(connection) = connection else {
            return Ok(None);
        };
        if let Connection::Saved { id, revision, .. } = connection {
            let resolver = resolver.ok_or("The source resolver is unavailable")?;
            let receive = resolver.resolve(id.clone(), Some(revision.clone()))?;
            Ok(Some(Self::Saved {
                expected_principal: Some(crate::client::connection_key(connection)?),
                resolver,
                receive,
            }))
        } else {
            Ok(Some(Self::Direct(connection.clone())))
        }
    }
}
impl Viewer {
    pub(super) fn start_window_source(&mut self) {
        if self.fixture || self.window.started {
            return;
        }
        if self.window.source.is_none() && self.connection.is_none() {
            let source = self
                .window
                .lease
                .as_ref()
                .and_then(|l| l.record().route.source.clone());
            match source {
                Some(SourceDescriptor::Saved { id, revision }) => {
                    let result = self
                        .connections
                        .resolver()
                        .ok_or("The source resolver is unavailable".to_string())
                        .and_then(|resolver| {
                            let receive = resolver.resolve(id.clone(), Some(revision.clone()))?;
                            Ok(Source::Saved {
                                expected_principal: None,
                                resolver,
                                receive,
                            })
                        });
                    match result {
                        Ok(source) => self.window.source = Some(source),
                        Err(error) => {
                            self.error = Some(error);
                            self.connection_form = true;
                            return;
                        }
                    }
                }
                Some(source) => match source.connection_without_secret() {
                    Some(connection) => self.window.source = Some(Source::Direct(connection)),
                    None => {
                        if let SourceDescriptor::Remote { endpoint, .. } = source {
                            self.connection_url.replace_text(&endpoint);
                        }
                        self.loading = false;
                        self.connection_form = true;
                        self.error=Some("This source needs credentials. Enter them or select its saved connection.".into());
                        return;
                    }
                },
                None => {}
            }
        }
        self.poll_window_source();
        if self.window.source.is_none() && !self.window.started {
            self.window.started = true;
            if self.connection.is_none() {
                self.connection_form = true;
                self.loading = false;
                self.status = "Not connected".into();
            }
            self.begin();
        }
    }
    pub(super) fn poll_window_source(&mut self) -> bool {
        let Some(source) = self.window.source.take() else {
            if let Some(retained) = &self.window.retained {
                if !retained.lease.is_valid() {
                    let message = retained
                        .lease
                        .error()
                        .unwrap_or_else(|| "The captured source is no longer available".into());
                    self.window.retained = None;
                    self.window.source_retired = true;
                    self.connection = None;
                    self.connection_form = true;
                    self.conversation.close_connection(self.connection_epoch);
                    self.chat.close_connection(self.connection_epoch);
                    self.subagents.close_connection(self.connection_epoch);
                    self.audio.close_connection(self.connection_epoch);
                    self.terminal.close_connection(self.connection_epoch);
                    self.background.retire_connection(self.connection_epoch);
                    self.preview.close_connection(self.connection_epoch);
                    self.media_players.close_connection(self.connection_epoch);
                    self.audio_preview = None;
                    self.loading = false;
                    self.history_loading = false;
                    self.error = Some(message);
                    self.status = "Disconnected".into();
                    return true;
                }
            }
            return false;
        };
        let (connection, retained) = match source {
            Source::Direct(connection) => (connection, None),
            Source::Saved {
                expected_principal,
                resolver,
                receive,
            } => match receive.try_recv() {
                Ok(Ok(lease)) => {
                    if !lease.is_valid()
                        || expected_principal.as_deref().is_some_and(|expected| {
                            crate::client::connection_key(lease.connection()).as_deref()
                                != Ok(expected)
                        })
                    {
                        self.error =
                            Some("The captured source changed while opening this window".into());
                        self.loading = false;
                        return true;
                    }
                    let connection = lease.connection().clone();
                    (
                        connection,
                        Some(Retained {
                            _resolver: resolver,
                            lease,
                        }),
                    )
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.loading = false;
                    self.connection_form = true;
                    return true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.window.source = Some(Source::Saved {
                        expected_principal,
                        resolver,
                        receive,
                    });
                    return false;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("The captured source service stopped".into());
                    self.loading = false;
                    return true;
                }
            },
        };
        if let Some(route) = self.window.pending_fresh.take() {
            if !self.use_connection_for_profile(connection, Some(route.profile.clone())) {
                self.loading = false;
                self.status = "Connection change blocked".into();
                return true;
            }
            self.window.retained = retained;
            self.window.boot_target = Some(route);
        } else {
            self.window.retained = retained;
            self.connection = Some(connection);
            self.connection_form = false;
            self.window.started = true;
            self.pin_window_source();
            self.begin();
        }
        true
    }
    fn pin_window_source(&mut self) {
        if self.window.pinned.is_some() {
            return;
        }
        let route = self
            .window
            .lease
            .as_ref()
            .and_then(|l| l.record().pinned_route.clone());
        if let (Some(route), Some(connection)) = (route, self.connection.clone()) {
            self.window.pinned = Some(Pinned {
                connection,
                route,
                retained: self.window.retained.clone(),
            });
        }
    }
    /// A profile-window default is immutable; an ordinary peer only inherits for boot.
    pub(in crate::app) fn prepare_window_new_session(&mut self) -> bool {
        if self.fixture {
            return false;
        }
        let Some(pin) = self.window.pinned.clone() else {
            return false;
        };
        if self
            .connection
            .as_ref()
            .and_then(|c| crate::client::connection_key(c).ok())
            == crate::client::connection_key(&pin.connection).ok()
        {
            if self.profile != pin.route.profile {
                if !self.allow_owner_change(&pin.route.profile) {
                    return true;
                }
                self.select_profile(pin.route.profile);
            }
            return false;
        }
        if !self.allow_owner_change("") {
            return true;
        }
        let resolver = pin
            .retained
            .as_ref()
            .map(|r| r._resolver.clone())
            .or_else(|| self.connections.resolver());
        match Source::capture(Some(&pin.connection), resolver) {
            Ok(source) => {
                self.window.source = source;
                self.window.pending_fresh = Some(pin.route);
                self.loading = true;
                self.status = "Connecting to the profile window agent…".into();
            }
            Err(error) => self.client_error = Some(error),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generic_new_session_uses_profile_pin_but_an_ordinary_peer_only_inherits_for_boot() {
        let connection = Connection::Url {
            endpoint: "https://pin-fixture.invalid".into(),
            token: Some("synthetic".into()),
        };
        let mut pinned = Viewer::empty();
        pinned.connection = Some(connection.clone());
        pinned.profile = "archive".into();
        pinned.selected_session = Some("old-chat".into());
        pinned.window.pinned = Some(Pinned {
            connection: connection.clone(),
            route: WindowRoute {
                profile: "research".into(),
                ..Default::default()
            },
            retained: None,
        });
        pinned.new_draft();
        assert_eq!(pinned.profile, "research");
        assert!(pinned.selected_session.is_none());
        let mut ordinary = Viewer::empty();
        ordinary.connection = Some(connection);
        ordinary.profile = "archive".into();
        ordinary.selected_session = Some("old-chat".into());
        ordinary.new_draft();
        assert_eq!(ordinary.profile, "archive");
        assert!(ordinary.selected_session.is_none());
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    fn connection(endpoint: &str) -> Connection {
        Connection::Url {
            endpoint: endpoint.into(),
            token: None,
        }
    }
    fn pinned() -> Viewer {
        let mut s = Viewer::empty();
        s.profile = "research".into();
        s.connection = Some(connection("https://temporary.fixture.invalid"));
        s.window.pinned = Some(Pinned {
            connection: connection("https://pinned.fixture.invalid"),
            route: WindowRoute {
                profile: "research".into(),
                ..Default::default()
            },
            retained: None,
        });
        s
    }
    #[test]
    fn a_same_profile_does_not_bypass_the_source_change_guard() {
        let mut s = pinned();
        let before = crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap();
        s.chrome.writing = true;
        assert!(s.prepare_window_new_session());
        assert!(s.window.pending_fresh.is_none());
        assert!(s.window.source.is_none());
        assert!(!s.loading);
        assert!(s.client_error.is_some());
        assert_eq!(
            crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap(),
            before
        );
    }
    #[test]
    fn an_owner_change_rejected_after_resolution_does_not_install_the_pin() {
        let mut s = pinned();
        s.profile = "archive".into();
        let epoch = s.connection_epoch;
        let before = crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap();
        assert!(s.prepare_window_new_session());
        assert!(s.loading);
        s.chrome.writing = true;
        assert!(s.poll_window_source());
        assert_eq!(s.profile, "archive");
        assert_eq!(s.connection_epoch, epoch);
        assert_eq!(
            crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap(),
            before
        );
        assert!(s.window.boot_target.is_none());
        assert!(s.window.retained.is_none());
        assert!(!s.loading);
        assert_eq!(s.status, "Connection change blocked");
    }
}
