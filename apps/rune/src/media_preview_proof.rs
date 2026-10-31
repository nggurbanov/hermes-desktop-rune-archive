//! Actual native controls against a private read-only loopback audio peer.
use crate::{
    composer::DraftScope,
    gateway::Connection,
    media_player::{Action, MediaPlayers, PlayerId, State as Playback},
    preview::{Kind, Source, Target},
};
use rune::{App, AppSpec, Cx, El, Tx, col, row};
use rune_kit::{Button, ButtonVariant};
use std::path::PathBuf;
struct State {
    players: MediaPlayers,
    id: Option<PlayerId>,
    endpoint: String,
    output: PathBuf,
    dark: bool,
    last: serde_json::Value,
}
fn state() -> State {
    State {
        players: MediaPlayers::new(),
        id: None,
        endpoint: String::new(),
        output: PathBuf::new(),
        dark: false,
        last: serde_json::Value::Null,
    }
}
fn setup(s: &mut State, waker: rune::Waker) {
    s.players.init(waker);
    s.id = Some(
        s.players
            .open(
                Connection::Url {
                    endpoint: s.endpoint.clone(),
                    token: Some("audio-fixture".into()),
                },
                Target {
                    owner: DraftScope {
                        draft_origin: None,
                        profile_intent: crate::source::ProfileIntent::Backend,
                        connection: 17,
                        profile: "research".into(),
                        session: Some("audio-session".into()),
                    },
                    source: Source::AgentPath("/fixture/voice.wav".into()),
                    label: "Field notes.wav".into(),
                    kind: Kind::Audio,
                },
            )
            .expect("valid isolated audio target"),
    );
}
fn poll(s: &mut State, _now: f64, tx: &mut Tx) -> bool {
    let changed = s.players.poll();
    let value = if let Some(p) = s.id.and_then(|id| s.players.get(id)) {
        serde_json::json!({"ready":p.state!=Playback::Loading,"state":format!("{:?}",p.state),"position":p.position,"duration":p.duration,"seekable":p.seekable,"rate":p.rate,"volume":p.volume,"muted":p.muted,"error":p.error,"save_error":p.save_error,"saved":p.notice.as_deref()==Some("Original saved"),"dark":s.dark})
    } else {
        serde_json::json!({"closed":true,"dark":s.dark})
    };
    if value != s.last {
        let _ = crate::media::write_private(
            &s.output.join("inspection.json"),
            serde_json::to_string(&value).unwrap().as_bytes(),
        );
        s.last = value;
    }
    tx.wake_after(std::time::Duration::from_millis(50));
    changed
}
fn view(s: &State, cx: &mut Cx<State>) -> El {
    let keys = [
        "audio-seek",
        "audio-rate-2",
        "audio-volume-down",
        "audio-mute",
        "proof-theme",
        "proof-save",
        "proof-retire",
    ];
    let rects: std::collections::BTreeMap<_, _> = keys
        .into_iter()
        .filter_map(|key| cx.layout(key).map(|rect| (key, rect)))
        .collect();
    let _ = crate::media::write_private(
        &s.output.join("geometry.json"),
        serde_json::to_string(&rects).unwrap().as_bytes(),
    );
    let k = rune_theme_opencode::kit(s.dark);
    let mut body = col().w_full().h_full().min_h(0.).fill(k.c.bg_deep).child(
        row()
            .w_full()
            .min_h(38.)
            .gap(8.)
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
                    .on_press(|s: &mut State| {
                        if let Some(id) = s.id {
                            s.players.act(
                                id,
                                Action::SaveOriginal(s.output.join("saved-original.wav")),
                            );
                        }
                    })
                    .build(&k)
                    .key("proof-save"),
            )
            .child(
                Button::new("Retire owner")
                    .variant(ButtonVariant::GhostMuted)
                    .on_press(|s: &mut State| s.players.close_connection(17))
                    .build(&k)
                    .key("proof-retire"),
            ),
    );
    if let Some(id) = s.id {
        body = body.child(
            crate::view::media_player::content(&s.players, id, &k, |s: &mut State| &mut s.players)
                .grow(1.)
                .min_h(0.),
        );
    }
    body
}
pub fn run_if_proof_mode() -> bool {
    let args: Vec<_> = std::env::args().collect();
    if !args.iter().any(|a| a == "--audio-preview-proof") {
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
            "Audio acceptance requires a numeric loopback peer and absolute output directory"
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
        ..AppSpec::new("Hermes · Audio acceptance", (960., 720.), state, view)
    };
    App::new(spec)
        .init(move || State {
            endpoint,
            output,
            ..state()
        })
        .run();
    true
}
