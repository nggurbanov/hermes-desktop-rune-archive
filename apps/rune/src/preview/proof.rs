//! Native acceptance uses only a numeric loopback fixture peer and an isolated output directory.
use super::*;
use rune::{App, AppSpec, Cx, El, Tx, col, row};
use rune_kit::{Button, ButtonVariant};
use serde_json::json;
struct State {
    service: Service,
    endpoint: String,
    dark: bool,
    output: PathBuf,
    last: serde_json::Value,
}
fn default_state() -> State {
    State {
        service: Service::new(),
        endpoint: String::new(),
        dark: false,
        output: PathBuf::new(),
        last: serde_json::Value::Null,
    }
}
fn setup(state: &mut State, waker: rune::Waker) {
    state.service.init(waker);
    state.service.open(
        Connection::Url {
            endpoint: state.endpoint.clone(),
            token: Some("preview-fixture".into()),
        },
        Target {
            owner: DraftScope {
                draft_origin: None,
                profile_intent: crate::source::ProfileIntent::Backend,
                connection: 17,
                profile: "research".into(),
                session: Some("preview-session".into()),
            },
            source: Source::AgentPath("/fixture/preview.png".into()),
            label: "Source comparison.png".into(),
            kind: Kind::Image,
        },
    );
}
fn poll(state: &mut State, _now: f64, tx: &mut Tx) -> bool {
    let changed = state.service.poll();
    let report = if let Some(d) = state.service.active() {
        json!({"id":d.id,"loading":d.loading,"ready":d.image.as_ref().is_some_and(|i|i.size().is_some()),
            "scale":d.camera.scale,"pan":d.camera.pan,"viewport":d.camera.viewport,"image":d.camera.image,
            "error":d.error,"saved":d.notice.as_deref()==Some("Original saved"),"dark":state.dark,
            "original":d.original.as_ref().map(|o|o.size)})
    } else {
        json!({"closed":true,"dark":state.dark})
    };
    if report != state.last {
        let _ = crate::media::write_private(
            &state.output.join("inspection.json"),
            serde_json::to_string(&report).unwrap().as_bytes(),
        );
        state.last = report;
    }
    tx.wake_after(std::time::Duration::from_millis(50));
    changed
}
fn view(state: &State, _cx: &mut Cx<State>) -> El {
    let k = rune_theme_opencode::kit(state.dark);
    let id = state.service.active().map(|d| d.id).unwrap_or(0);
    col()
        .w_full()
        .h_full()
        .fill(k.c.bg_deep)
        .child(
            row()
                .w_full()
                .min_h(38.)
                .gap(4.)
                .px(8.)
                .child(
                    Button::new("Theme")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut State| s.dark = !s.dark)
                        .build(&k)
                        .key("proof-theme"),
                )
                .child(
                    Button::new("Save fixture copy")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(move |s: &mut State| {
                            s.service.action(
                                id,
                                Action::SaveOriginal(s.output.join("saved-original.png")),
                            )
                        })
                        .build(&k)
                        .key("proof-save"),
                )
                .child(
                    Button::new("Retire owner")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut State| s.service.close_connection(17))
                        .build(&k)
                        .key("proof-retire"),
                ),
        )
        .child(
            crate::view::preview::content(&state.service, &k, |s: &mut State| &mut s.service)
                .grow(1.)
                .min_h(0.),
        )
}
pub fn run_if_proof_mode() -> bool {
    let args: Vec<_> = std::env::args().collect();
    if !args.iter().any(|a| a == "--preview-proof") {
        return false;
    }
    let option = |name: &str| args.windows(2).find(|p| p[0] == name).map(|p| p[1].clone());
    let endpoint = option("--gateway").unwrap_or_default();
    let output = option("--proof-output")
        .map(PathBuf::from)
        .unwrap_or_default();
    let valid = url::Url::parse(&endpoint).is_ok_and(|u| {
        u.scheme() == "http"
            && u.username().is_empty()
            && u.password().is_none()
            && u.host_str().is_some_and(|h| {
                h.parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
            })
    });
    if !valid || !output.is_absolute() {
        eprintln!(
            "Native preview acceptance requires a numeric loopback peer and absolute output path"
        );
        return true;
    }
    if std::fs::create_dir_all(&output).is_err() {
        return true;
    }
    let spec = AppSpec {
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(setup),
        poll: Some(poll),
        ..AppSpec::new(
            "Hermes · Preview acceptance",
            (960., 720.),
            default_state,
            view,
        )
    };
    App::new(spec)
        .init(move || State {
            service: Service::new(),
            endpoint,
            dark: false,
            output,
            last: serde_json::Value::Null,
        })
        .run();
    true
}
