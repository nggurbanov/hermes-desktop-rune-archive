//! Resolver senders retain the source service after its original UI has closed.
use super::*;

pub(super) fn run(
    mut state: Result<core::Core, String>,
    requests: Receiver<Work>,
    replies: Sender<Reply>,
    waker: Waker,
) {
    let mut ui = Some(replies);
    let mut next_registry_read = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        match requests.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(Work::RetainPassive {
                version,
                connection,
                reply,
            }) => {
                let result = match &mut state {
                    Ok(core) => core.retain_passive(&version, connection),
                    Err(error) => Err(error.clone()),
                };
                let _ = reply.send(result);
            }
            Ok(Work::Passive { version, reply }) => {
                let result = match &mut state {
                    Ok(core) => core.passive_plan(&version),
                    Err(error) => Err(error.clone()),
                };
                let _ = reply.send(result);
            }
            Ok(Work::ValidatePassive { version, reply }) => {
                let result = match &mut state {
                    Ok(core) => core.validate_passive(&version),
                    Err(error) => Err(error.clone()),
                };
                let _ = reply.send(result);
            }
            Ok(Work::Ui(request)) => {
                let result = match &mut state {
                    Ok(core) => core.execute(request.command),
                    Err(error) => Err(error.clone()),
                };
                reply_ui(
                    &mut ui,
                    Reply {
                        id: request.id,
                        result,
                    },
                    &waker,
                );
            }
            Ok(Work::Observe) => {
                let result = match &mut state {
                    Ok(core) => core.observe_registry(),
                    Err(error) => Err(error.clone()),
                };
                reply_ui(
                    &mut ui,
                    Reply {
                        id: 0,
                        result: Ok(Outcome::Observed(result)),
                    },
                    &waker,
                );
            }
            Ok(Work::Maintain(command)) => {
                let result = match &mut state {
                    Ok(core) => core.execute(command).map(|_| core.visible_registry()),
                    Err(error) => Err(error.clone()),
                };
                reply_ui(
                    &mut ui,
                    Reply {
                        id: 0,
                        result: Ok(Outcome::Maintained(result)),
                    },
                    &waker,
                );
            }
            Ok(Work::ReleaseForeground { id, revision }) => {
                if let Ok(core) = &mut state {
                    core.release_foreground(&id, &revision);
                }
            }
            Ok(Work::CommitRoute {
                lease,
                profile,
                intent,
                alive,
                reply,
            }) => {
                let result = match &mut state {
                    Ok(core) => core
                        .execute(Command::CommitRoute(lease, profile.clone(), alive))
                        .and_then(|outcome| match outcome {
                            Outcome::Ready(_, connection, _) => Ok(connection),
                            _ => Err("The route activation returned an invalid result".into()),
                        }),
                    Err(error) => Err(error.clone()),
                };
                let success = result.is_ok();
                if reply
                    .send(RouteCommit {
                        intent,
                        profile,
                        result,
                    })
                    .is_ok()
                {
                    waker();
                }
                if success {
                    if let Ok(core) = &state {
                        reply_ui(
                            &mut ui,
                            Reply {
                                id: 0,
                                result: Ok(Outcome::Maintained(Ok(core.visible_registry()))),
                            },
                            &waker,
                        );
                    }
                }
            }
            Ok(Work::Resolve {
                id,
                expected_revision,
                reply,
            }) => {
                let result = match &mut state {
                    Ok(core) => core.resolve_lease(&id, expected_revision.as_deref()),
                    Err(error) => Err(error.clone()),
                };
                if reply.send(result).is_ok() {
                    waker();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Ok(core) = &mut state {
            let now = std::time::Instant::now();
            if now >= next_registry_read {
                if core.has_sources() {
                    let _ = core.refresh_registry();
                }
                next_registry_read = now + std::time::Duration::from_secs(1);
            }
            if let Some((id, revision, error)) = core.poll_tunnel() {
                reply_ui(
                    &mut ui,
                    Reply {
                        id: 0,
                        result: Ok(Outcome::Lost(id, revision, error)),
                    },
                    &waker,
                );
            }
        }
    }
    // Dropping Core retires every lease/tunnel once no UI or Resolver sender remains.
}
fn reply_ui(ui: &mut Option<Sender<Reply>>, reply: Reply, waker: &Waker) {
    if let Some(send) = ui {
        if send.send(reply).is_ok() {
            waker();
        } else {
            *ui = None;
        }
    }
}
