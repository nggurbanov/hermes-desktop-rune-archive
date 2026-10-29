use crate::pages::messaging::Messaging;
use crate::{app::Viewer, pages::messaging::Action};
use rune::{El, ScrollSpec, TextEdit, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, Switch, TextArea, TextInput};
fn btn(s: &Viewer, label: impl Into<String>, action: Action, disabled: bool, k: &Kit) -> El {
    let owner = s.messaging.profile.clone();
    let connection = s.composer_scope().connection;
    let preview = matches!(
        action,
        Action::Select(_)
            | Action::Tab(_)
            | Action::Filter(_)
            | Action::Advanced
            | Action::Cancel
            | Action::DismissSecret
    );

    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled || s.messaging.demo && !preview)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if s.messaging.profile != owner || s.composer_scope().connection != connection {
                return;
            }
            s.messaging_action(action.clone());
            tx.instant();
        })
        .build(k)
}
fn body(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.ui).color(k.c.text_base).wrap()
}
fn small(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.small).color(k.c.text_muted).wrap()
}
fn guarded(
    s: &Viewer,
    access: for<'a> fn(&'a mut Messaging) -> &'a mut TextEdit,
) -> impl for<'a> Fn(&'a mut Viewer) -> &'a mut TextEdit + 'static + use<> {
    let profile = s.messaging.profile.clone();
    let connection = s.composer_scope().connection;
    move |s: &mut Viewer| {
        if s.messaging.profile != profile
            || s.composer_scope().connection != connection
            || s.messaging.busy
            || s.messaging.demo
        {
            return s.messaging.edit_detached();
        }
        access(&mut s.messaging)
    }
}
pub fn page(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let state = &s.messaging;
    let options: Vec<String> = std::iter::once("Current conversation".into())
        .chain(s.profiles.iter().map(|p| p.name.clone()))
        .collect();
    let scope = Select::new("messaging-scope", &options)
        .current(Some(
            s.settings
                .scope_profile
                .as_deref()
                .unwrap_or("Current conversation"),
        ))
        .open(state.open_scope)
        .disabled(state.blocks_owner_change() || s.settings.blocks_owner_change())
        .on_toggle(|s: &mut Viewer| s.messaging.open_scope = !s.messaging.open_scope)
        .on_close(|s: &mut Viewer| s.messaging.open_scope = false)
        .on_choose(|s: &mut Viewer, value, tx| {
            s.messaging.open_scope = false;
            s.messaging_scope(if value == "Current conversation" {
                None
            } else {
                Some(value.into())
            });
            tx.instant();
        })
        .build(k, layer);
    let mut nav = col()
        .w(220.0)
        .min_h(0.0)
        .gap(4.0)
        .px(12.0)
        .py(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            TextInput::new(&state.search)
                .placeholder("Search platforms")
                .width(None)
                .on_edit(|s: &mut Viewer| &mut s.messaging.search)
                .build(k)
                .key("messaging-search"),
        );
    for (i, name) in ["Platforms", "Pairing", "Webhooks", "Telegram setup"]
        .into_iter()
        .enumerate()
    {
        nav = nav.child(
            btn(s, name, Action::Tab(i), false, k)
                .key(format!("messaging-tab-{i}"))
                .w_full()
                .justify_start()
                .fill(if state.tab == i {
                    k.c.layer_01
                } else {
                    rune::Color::TRANSPARENT
                }),
        );
    }
    if state.tab == 0 {
        let filters = ["all", "ready", "setup", "issues", "disabled"]
            .map(str::to_owned)
            .to_vec();
        nav = nav.child(
            Select::new("messaging-status-filter", &filters)
                .current(Some(&state.filter))
                .open(state.open_filter)
                .on_toggle(|s: &mut Viewer| s.messaging.open_filter = !s.messaging.open_filter)
                .on_close(|s: &mut Viewer| s.messaging.open_filter = false)
                .on_choose(|s: &mut Viewer, value, tx| {
                    s.messaging.open_filter = false;
                    s.messaging_action(Action::Filter(value.into()));
                    tx.instant();
                })
                .build(k, layer),
        );
        for p in &state.platforms {
            let id = p["id"].as_str().unwrap_or("").to_owned();
            let name = p["name"].as_str().unwrap_or(&id);
            if state.filter != "all" && crate::pages::messaging::platform_group(p) != state.filter {
                continue;
            }
            if !format!("{id} {name}")
                .to_lowercase()
                .contains(&state.search.text.to_lowercase())
            {
                continue;
            }
            nav = nav.child(
                btn(s, name, Action::Select(id.clone()), state.busy, k)
                    .w_full()
                    .justify_start()
                    .fill(if state.selected == id {
                        k.c.layer_01
                    } else {
                        rune::Color::TRANSPARENT
                    }),
            );
        }
    }
    let mut detail = col()
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .gap(14.0)
        .px(28.0)
        .py(20.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(scope)
        .child(
            row()
                .w_full()
                .center_y()
                .child(
                    text("Messaging")
                        .font(k.ty.body_medium)
                        .color(k.c.text_base),
                )
                .child(el().grow(1.0))
                .child(btn(
                    s,
                    if state.loading {
                        "Refreshing…"
                    } else {
                        "Refresh"
                    },
                    Action::Refresh,
                    state.loading || state.busy,
                    k,
                )),
        );
    if let Some(e) = &state.error {
        detail = detail.child(
            text(e)
                .role("alert")
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if let Some(n) = &state.notice {
        detail = detail.child(small(n, k));
    }
    if state.restart_needed {
        detail = detail
            .child(small(
                "Adapter settings need a gateway restart. This affects every served profile.",
                k,
            ))
            .child(btn(
                s,
                "Restart gateway…",
                Action::Restart,
                state.busy || state.restart.as_ref().is_some_and(|r| !r.finished),
                k,
            ));
    }
    if state.demo {
        detail = detail.child(small(
            "Demonstration data · live messaging writes are disabled",
            k,
        ));
    }
    if let Some(restart) = &state.restart {
        detail = detail.child(small(
            if restart.finished {
                "Gateway restart finished"
            } else {
                "Waiting for the gateway restart…"
            },
            k,
        ));
        if let Some(error) = &restart.error {
            detail = detail.child(text(error).font(k.ty.small).color(k.c.danger.fg).wrap());
        }
        if !restart.lines.is_empty() {
            detail = detail.child(
                text(restart.lines.join("\n"))
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap()
                    .selectable(true),
            );
        }
    }
    if let Some(result) = &state.test_result {
        detail = detail.child(
            text(format!(
                "{} · {}{}",
                if result["ok"] == true {
                    "Connection test passed"
                } else {
                    "Connection test failed"
                },
                result["message"].as_str().unwrap_or(""),
                result["state"]
                    .as_str()
                    .map(|v| format!(" · {v}"))
                    .unwrap_or_default()
            ))
            .font(k.ty.ui)
            .color(if result["ok"] == true {
                k.c.success.fg
            } else {
                k.c.danger.fg
            })
            .wrap(),
        );
    }
    if let Some(confirm) = &state.confirm {
        let label = match confirm {
            Action::Restart => {
                "Restart the gateway for this profile? Active conversations may be interrupted."
            }
            Action::Revoke(_, _) => "Revoke this user's messaging access?",
            _ => "Delete this webhook subscription?",
        };
        detail = detail.child(
            col().gap(8.0).child(body(label, k)).child(
                row()
                    .gap(8.0)
                    .child(btn(s, "Confirm", Action::Confirm, state.busy, k))
                    .child(btn(s, "Cancel", Action::Cancel, false, k)),
            ),
        );
    }
    match state.tab {
        0 => {
            if let Some(p) = state.current() {
                detail = detail
                    .child(
                        text(p["name"].as_str().unwrap_or(&state.selected))
                            .font(k.ty.body_medium)
                            .color(k.c.text_base),
                    )
                    .child(body(p["description"].as_str().unwrap_or(""), k))
                    .child(small(
                        format!(
                            "{} · {}",
                            if p["enabled"] == true {
                                "Enabled"
                            } else {
                                "Disabled"
                            },
                            p["state"].as_str().unwrap_or("Unknown state")
                        ),
                        k,
                    ))
                    .child(
                        row()
                            .gap(8.0)
                            .child(btn(
                                s,
                                if p["enabled"] == true {
                                    "Disable"
                                } else {
                                    "Enable"
                                },
                                Action::Toggle(p["enabled"] != true),
                                state.busy,
                                k,
                            ))
                            .child(btn(s, "Test connection", Action::Test, state.busy, k)),
                    );
                if let Some(channel) = p.get("home_channel").filter(|v| !v.is_null()) {
                    detail = detail.child(small(
                        format!(
                            "Home channel: {}",
                            channel
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| format!(
                                    "{} · {}{}",
                                    channel["name"].as_str().unwrap_or(""),
                                    channel["chat_id"].as_str().unwrap_or(""),
                                    channel["thread_id"]
                                        .as_str()
                                        .map(|t| format!(" · thread {t}"))
                                        .unwrap_or_default()
                                ))
                        ),
                        k,
                    ));
                }
                if let Some(url) = p["ingress_url"].as_str() {
                    detail = detail
                        .child(small(format!("Incoming messages: {url}"), k).selectable(true));
                }
                if let Some(error) = p["error_message"].as_str() {
                    detail = detail.child(text(error).font(k.ty.ui).color(k.c.danger.fg).wrap());
                }
                detail = detail.child(small(platform_hint(&state.selected), k));
                detail = detail.child(btn(
                    s,
                    if state.show_advanced {
                        "Hide advanced fields"
                    } else {
                        "Show advanced fields"
                    },
                    Action::Advanced,
                    false,
                    k,
                ));
                for f in p["env_vars"].as_array().into_iter().flatten() {
                    if f["advanced"] == true && !state.show_advanced {
                        continue;
                    }

                    let key = f["key"].as_str().unwrap_or("");
                    let id = format!("{}:{key}", state.selected);
                    let owner = id.clone();
                    let profile = state.profile.clone();
                    let connection = s.composer_scope().connection;
                    let edit = state.edits.get(&id).cloned().unwrap_or_else(|| {
                        TextEdit::new(if f["is_password"] != true {
                            f["value"].as_str().unwrap_or("")
                        } else {
                            ""
                        })
                    });
                    let mut input = TextInput::new(&edit)
                        .width(None)
                        .placeholder(if f["is_set"] == true {
                            "Saved. Leave untouched to retain it."
                        } else {
                            "Not configured"
                        })
                        .disabled(state.busy || state.demo || state.field_cleared(&id))
                        .on_edit(move |s: &mut Viewer| {
                            if s.messaging.profile != profile
                                || s.composer_scope().connection != connection
                                || s.messaging.busy
                            {
                                return s.messaging.edit_detached();
                            }
                            s.messaging
                                .edits
                                .entry(owner.clone())
                                .or_insert_with(|| edit.clone())
                        });
                    if f["is_password"] == true {
                        input = input.secure();
                    }
                    detail = detail.child(
                        col()
                            .w_full()
                            .gap(5.0)
                            .child(body(
                                format!(
                                    "{}{}",
                                    f["prompt"].as_str().unwrap_or(key),
                                    if f["required"] == true { " *" } else { "" }
                                ),
                                k,
                            ))
                            .child(input.build(k).key(format!("messaging-{key}")))
                            .child(small(f["description"].as_str().unwrap_or(""), k))
                            .children((f["is_set"] == true && f["is_list"] != true).then(|| {
                                btn(
                                    s,
                                    if state.field_cleared(&id) {
                                        "Keep saved value"
                                    } else {
                                        "Clear saved value"
                                    },
                                    Action::Clear(key.into()),
                                    state.busy,
                                    k,
                                )
                            }))
                            .children(f["url"].as_str().filter(|u| u.starts_with("https://")).map(
                                |url| {
                                    let url = url.to_owned();
                                    Button::new("Field documentation")
                                        .variant(ButtonVariant::GhostMuted)
                                        .on_press_tx(move |_: &mut Viewer, tx| {
                                            let _ = tx.open_url(&url);
                                        })
                                        .build(k)
                                },
                            )),
                    );
                }
                detail = detail.child(
                    btn(
                        s,
                        if state.busy {
                            "Saving…"
                        } else {
                            "Save changes"
                        },
                        Action::Save,
                        state.busy,
                        k,
                    )
                    .key("messaging-save"),
                );
                detail = detail.child(btn(
                    s,
                    "Revert unsaved platform changes",
                    Action::Revert,
                    state.busy,
                    k,
                ));
                if let Some(url) = p["docs_url"].as_str().filter(|u| u.starts_with("https://")) {
                    let url = url.to_owned();
                    detail = detail.child(
                        Button::new("Platform documentation")
                            .variant(ButtonVariant::GhostMuted)
                            .on_press_tx(move |_: &mut Viewer, tx| {
                                let _ = tx.open_url(&url);
                            })
                            .build(k),
                    );
                }
            } else {
                detail = detail.child(body(
                    if state.loading {
                        "Loading platforms…"
                    } else {
                        "No messaging platforms reported by this agent."
                    },
                    k,
                ));
            }
        }
        1 => {
            detail = detail.child(body("Pending requests", k));
            for user in state.pairing["pending"].as_array().into_iter().flatten() {
                let platform = user["platform"].as_str().unwrap_or("").to_owned();
                let request = user["request_id"].as_str().unwrap_or("").to_owned();
                detail = detail.child(
                    row()
                        .gap(10.0)
                        .child(body(
                            format!(
                                "{} · {}",
                                platform,
                                user["user_name"]
                                    .as_str()
                                    .or(user["user_id"].as_str())
                                    .unwrap_or("Unknown")
                            ),
                            k,
                        ))
                        .child(btn(
                            s,
                            "Approve",
                            Action::Approve(platform, request.clone()),
                            state.busy || request.is_empty(),
                            k,
                        )),
                );
            }
            detail = detail.child(body("Approved users", k));
            for user in state.pairing["approved"].as_array().into_iter().flatten() {
                let platform = user["platform"].as_str().unwrap_or("").to_owned();
                let id = user["user_id"].as_str().unwrap_or("").to_owned();
                detail = detail.child(
                    row()
                        .gap(10.0)
                        .child(body(
                            format!(
                                "{} · {}",
                                platform,
                                user["user_name"].as_str().unwrap_or(&id)
                            ),
                            k,
                        ))
                        .child(btn(
                            s,
                            "Revoke…",
                            Action::Revoke(platform, id),
                            state.busy,
                            k,
                        )),
                );
            }
        }
        2 => {
            detail = detail.child(btn(
                s,
                "Enable webhook receiver",
                Action::EnableWebhooks,
                state.busy || state.webhooks["enabled"] == true,
                k,
            ));
            for w in state.webhooks["subscriptions"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let name = w["name"].as_str().unwrap_or("").to_owned();
                detail = detail.child(
                    col()
                        .gap(6.0)
                        .child(body(&name, k))
                        .child(small(w["url"].as_str().unwrap_or(""), k))
                        .child(
                            row()
                                .gap(8.0)
                                .child(btn(
                                    s,
                                    if w["enabled"] == true {
                                        "Disable"
                                    } else {
                                        "Enable"
                                    },
                                    Action::ToggleWebhook(name.clone(), w["enabled"] != true),
                                    state.busy,
                                    k,
                                ))
                                .child(btn(
                                    s,
                                    "Delete…",
                                    Action::DeleteWebhook(name),
                                    state.busy,
                                    k,
                                )),
                        ),
                );
            }
            detail = detail
                .child(body("New subscription", k))
                .child(
                    TextInput::new(&state.webhook_name)
                        .placeholder("Name")
                        .width(None)
                        .disabled(state.busy || state.demo)
                        .on_edit(guarded(s, |m| &mut m.webhook_name))
                        .build(k),
                )
                .child(
                    TextArea::new(&state.webhook_prompt)
                        .width(None)
                        .placeholder("Prompt template")
                        .disabled(state.busy || state.demo)
                        .on_edit(guarded(s, |m| &mut m.webhook_prompt))
                        .build(k),
                )
                .child(
                    TextInput::new(&state.webhook_events)
                        .placeholder("Events, comma separated")
                        .width(None)
                        .disabled(state.busy || state.demo)
                        .on_edit(guarded(s, |m| &mut m.webhook_events))
                        .build(k),
                )
                .child(
                    TextInput::new(&state.webhook_delivery)
                        .placeholder("Delivery target")
                        .width(None)
                        .disabled(state.busy || state.demo)
                        .on_edit(guarded(s, |m| &mut m.webhook_delivery))
                        .build(k),
                )
                .child(
                    TextInput::new(&state.webhook_chat)
                        .placeholder("Delivery chat ID (optional)")
                        .width(None)
                        .disabled(state.busy || state.demo)
                        .on_edit(guarded(s, |m| &mut m.webhook_chat))
                        .build(k),
                )
                .child(
                    TextInput::new(&state.webhook_skills)
                        .placeholder("Skills, comma separated")
                        .width(None)
                        .disabled(state.busy || state.demo)
                        .on_edit(guarded(s, |m| &mut m.webhook_skills))
                        .build(k),
                )
                .child(
                    TextInput::new(&state.webhook_description)
                        .placeholder("Description")
                        .width(None)
                        .disabled(state.busy || state.demo)
                        .on_edit(guarded(s, |m| &mut m.webhook_description))
                        .build(k),
                )
                .child(
                    Switch::new(state.webhook_deliver_only)
                        .label("Deliver event payload without an agent turn")
                        .on_toggle({
                            let profile = state.profile.clone();
                            let connection = s.composer_scope().connection;
                            move |s: &mut Viewer| {
                                if s.messaging.profile != profile
                                    || s.composer_scope().connection != connection
                                    || s.messaging.busy
                                    || s.messaging.demo
                                {
                                    return;
                                }
                                s.messaging.webhook_deliver_only = !s.messaging.webhook_deliver_only
                            }
                        })
                        .build(k),
                )
                .child(btn(
                    s,
                    "Discard new subscription",
                    Action::RevertWebhook,
                    state.busy,
                    k,
                ))
                .child(btn(
                    s,
                    "Create subscription",
                    Action::CreateWebhook,
                    state.busy,
                    k,
                ));
            if let Some(secret) = &state.new_secret {
                let copied = secret.clone();
                detail = detail
                    .child(
                        Button::new("Copy one-time secret")
                            .variant(ButtonVariant::Ghost)
                            .on_press_tx(move |_: &mut Viewer, tx| tx.copy(copied.clone()))
                            .build(k),
                    )
                    .child(btn(s, "Dismiss secret", Action::DismissSecret, false, k));
                detail = detail
                    .child(small(
                        "One-time webhook secret. Store it before leaving this page.",
                        k,
                    ))
                    .child(
                        text(secret)
                            .font(k.ty.ui)
                            .color(k.c.text_base)
                            .selectable(true)
                            .wrap(),
                    );
            }
        }
        _ => {
            if let Some(error) = &state.telegram_error {
                detail = detail.child(text(error).font(k.ty.ui).color(k.c.danger.fg).wrap());
            }
            if let Some(remaining) = state.telegram_remaining() {
                detail = detail.child(small(
                    format!(
                        "Setup link expires in {}:{:02}",
                        remaining / 60,
                        remaining % 60
                    ),
                    k,
                ));
            }
            detail = detail.child(body("Create a Telegram bot", k)).child(
                TextInput::new(&state.bot_name)
                    .placeholder("Bot name (optional)")
                    .width(None)
                    .disabled(state.busy || state.demo)
                    .on_edit(guarded(s, |m| &mut m.bot_name))
                    .build(k),
            );
            if state.pairing_id.is_none() {
                detail = detail.child(btn(
                    s,
                    "Start Telegram setup",
                    Action::StartTelegram,
                    state.busy,
                    k,
                ));
            }
            if let Some(id) = &state.pairing_id {
                detail = detail.child(small(format!("Setup ID: {id}"), k));
                if let Some(qr) = &state.qr {
                    detail = detail.child(
                        el().size(240.0, 240.0)
                            .pixels_fit(qr.pixels(), rune::SurfaceFit::Fit),
                    );
                }
                if let Some(url) = state.onboarding["deep_link"]
                    .as_str()
                    .filter(|u| u.starts_with("https://"))
                {
                    let url = url.to_owned();
                    detail = detail.child(
                        Button::new("Open Telegram setup link")
                            .on_press_tx(move |_: &mut Viewer, tx| {
                                let _ = tx.open_url(&url);
                            })
                            .build(k),
                    );
                }
                detail = detail
                    .child(small(
                        format!(
                            "Status: {}",
                            state.onboarding["status"].as_str().unwrap_or("waiting")
                        ),
                        k,
                    ))
                    .child(btn(
                        s,
                        if state.telegram_polling {
                            "Checking…"
                        } else {
                            "Check status"
                        },
                        Action::PollTelegram,
                        state.busy || state.telegram_polling,
                        k,
                    ));
                if state.onboarding["status"] == "ready" {
                    detail = detail
                        .child(
                            TextInput::new(&state.allowed)
                                .placeholder("Allowed user IDs")
                                .width(None)
                                .disabled(state.busy || state.demo)
                                .on_edit(guarded(s, |m| &mut m.allowed))
                                .build(k),
                        )
                        .child(btn(
                            s,
                            "Apply bot to this profile",
                            Action::ApplyTelegram,
                            state.busy,
                            k,
                        ));
                }
                detail = detail.child(btn(s, "Cancel setup", Action::CancelTelegram, false, k));
            }
        }
    }
    row()
        .key("messaging-page")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(nav)
        .child(el().w(1.0).h_full().fill(k.c.border_muted))
        .child(detail)
}

fn platform_hint(id: &str) -> &str {
    match id {
        "telegram" => {
            "Use BotFather to create a bot, or use Telegram setup to pair one. Restrict access to your numeric user IDs."
        }
        "discord" => {
            "Create an application in the Discord Developer Portal, enable the required message intents and invite its bot to your server."
        }
        "slack" => {
            "Use Socket Mode with an app-level token and a bot token. Grant the bot access to the intended channels."
        }
        "mattermost" => "Connect a bot account to your Mattermost server using its access token.",
        "matrix" => "Set the homeserver URL, bot account and access token.",
        "signal" => {
            "Connect the signal-cli REST bridge associated with your registered phone number."
        }
        "whatsapp" => {
            "Run the agent's WhatsApp bridge setup and scan its QR code with WhatsApp before enabling the adapter."
        }
        "bluebubbles" => "Connect to the BlueBubbles server running on your Mac.",
        "homeassistant" => "Use your Home Assistant server URL and a long-lived access token.",
        "email" => {
            "Use a dedicated mailbox and an application password where your email provider requires one."
        }
        "sms" => "Configure the Twilio account, sending number and incoming webhook URL.",
        "dingtalk" => {
            "Configure the application's client ID and secret from the DingTalk developer console."
        }
        "feishu" | "lark" => {
            "Configure the app credentials, event subscription and event encryption settings."
        }
        "wecom" => {
            "A send-only webhook and an application callback use different credentials. Choose the mode supported by your organization."
        }
        "weixin" | "wechat" => {
            "Run hermes gateway setup for this profile and scan the Weixin setup QR code."
        }
        "qqbot" | "qq" => {
            "Create a bot in the QQ developer portal and enter its application credentials."
        }
        "api_server" => {
            "Expose the OpenAI-compatible API with an authentication key, bind host and port appropriate for this machine."
        }
        "webhooks" => {
            "Webhook subscriptions validate signed requests with their one-time HMAC secret."
        }
        _ => "",
    }
}
