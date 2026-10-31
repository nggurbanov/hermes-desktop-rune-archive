use rune::{Cx, El, Env, Runtime, col, text};
use rune_kit::{Kit, Markdown, markdown::Highlight};

#[derive(Default)]
struct State {
    src: String,
    clicks: usize,
    links: usize,
    mounts: usize,
    unmounts: usize,
}

fn view(s: &State, _: &mut Cx<State>) -> El {
    let k = Kit::neutral(true);
    let start = s.src.find("after").unwrap_or(0);
    col().child(
        Markdown::new(s.src.clone())
            .highlights([(start..start + 5, Highlight::new().key("after-mark"))])
            .on_link(|s: &mut State, _| s.links += 1)
            .embed_links(|occ, label| {
                if occ.destination != "media" {
                    return None;
                }
                assert_eq!(label, "same");
                Some(
                    text(format!("control-{}", occ.source.start))
                        .h(24.0)
                        .w(100.0)
                        .focusable(true)
                        .on_press(|s: &mut State| s.clicks += 1)
                        .on_mount_change(|s: &mut State, mounted| {
                            if mounted {
                                s.mounts += 1
                            } else {
                                s.unmounts += 1
                            }
                            false
                        }),
                )
            })
            .build(&k),
    )
}

#[test]
fn embedded_controls_preserve_order_links_highlight_and_mounts() {
    let src = "é **bold** [same](media) middle [same][ref] after [ordinary](https://example.test)\n\n[ref]: media";
    let mut rt =
        Runtime::new(State { src: src.into(), ..Default::default() }, view, (500.0, 500.0), Env::new(true, 1.0));
    rt.frame(0.0);
    let elements = rt.elements();
    let controls: Vec<_> =
        elements.iter().filter(|e| e.key.as_deref().is_some_and(|key| key.starts_with("md-embed-"))).collect();
    assert_eq!(controls.len(), 2);
    let first = src.find("[same](media)").unwrap();
    let second = src.find("[same][ref]").unwrap();
    assert_eq!(controls[0].key.as_deref(), Some(format!("md-embed-{first}").as_str()));
    assert_eq!(controls[1].key.as_deref(), Some(format!("md-embed-{second}").as_str()));
    assert_eq!(controls[0].text.as_deref(), Some(format!("control-{first}").as_str()));
    assert_eq!(controls[1].text.as_deref(), Some(format!("control-{second}").as_str()));
    assert!(controls[0].rect[1] < controls[1].rect[1]);
    let mark = elements.iter().find(|e| e.key.as_deref() == Some("after-mark")).unwrap();
    assert_eq!(mark.label.as_deref(), Some("after"));
    assert!(mark.rect[2] > 0.0 && mark.rect[1] > controls[1].rect[1]);
    let c = controls[0].rect;
    rt.pointer_down(c[0] + 2.0, c[1] + 2.0);
    rt.pointer_up(c[0] + 2.0, c[1] + 2.0);
    rt.frame(0.0);
    assert_eq!(rt.state.clicks, 1);
    assert_eq!(rt.state.links, 0);
    let link = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("https://example.test")).unwrap();
    rt.pointer_down(link.rect[0] + 2.0, link.rect[1] + 2.0);
    rt.pointer_up(link.rect[0] + 2.0, link.rect[1] + 2.0);
    rt.frame(0.0);
    assert_eq!(rt.state.links, 1);
    assert_eq!((rt.state.mounts, rt.state.unmounts), (2, 0));
    rt.invalidate();
    rt.frame(0.0);
    assert_eq!((rt.state.mounts, rt.state.unmounts), (2, 0));
    rt.state.src = format!("{src}\n\nappended");
    rt.invalidate();
    rt.frame(0.0);
    assert_eq!((rt.state.mounts, rt.state.unmounts), (2, 0));
    rt.state.src = "after".into();
    rt.invalidate();
    rt.frame(0.0);
    assert_eq!(rt.state.unmounts, 2);
}

#[test]
fn empty_authored_links_are_embeddable() {
    let occurrences = rune_kit::markdown::link_occurrences_cached("[](media)");
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].source, 0..9);
    assert_eq!(occurrences[0].label, 0..0);
    assert_eq!(occurrences[0].destination, "media");
    fn sole(_: &(), _: &mut Cx<()>) -> El {
        Markdown::new("[](media)").embed_links(|_, _| Some(text("sole"))).build(&Kit::neutral(true))
    }
    let mut solo = Runtime::new((), sole, (500., 300.), Env::new(true, 1.));
    solo.frame(0.);
    assert!(solo.elements().iter().any(|e| e.text.as_deref() == Some("sole")));
    fn scene(_: &(), _: &mut Cx<()>) -> El {
        Markdown::new("x [](media) [](media)")
            .embed_links(|_, label| {
                assert_eq!(label, "");
                Some(text("embedded"))
            })
            .build(&Kit::neutral(true))
    }
    let mut rt = Runtime::new((), scene, (500., 300.), Env::new(true, 1.));
    rt.frame(0.);
    assert_eq!(
        rt.elements().iter().filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("md-embed-"))).count(),
        2
    );
}

#[test]
fn copying_only_visible_fragment_excludes_embedded_source() {
    use rune::{Key, KeyEvent, Mods};
    fn scene(_: &(), _: &mut Cx<()>) -> El {
        Markdown::new("before [media](media)")
            .selectable(true)
            .copy_source(true)
            .embed_links(|_, _| Some(text("player")))
            .build(&Kit::neutral(true))
            .key("note")
    }
    let mut rt = Runtime::new((), scene, (500., 300.), Env::new(true, 1.));
    rt.frame(0.);
    let clip = rune::clipboard::memory();
    rt.use_clipboard(&clip);
    let path = rt.elements().into_iter().find(|e| e.text.as_deref() == Some("before ")).unwrap().path;
    rt.update(|_, tx| tx.select_all_text(path));
    rt.frame(0.);
    rt.key(KeyEvent { key: Key::Command('c'), mods: Mods { cmd: true, ..Mods::default() } });
    rt.frame(0.);
    assert_eq!(clip.text().as_deref().map(String::as_str), Some("before "));
}

#[test]
fn unreplaced_empty_link_adds_no_blank_layout() {
    fn scene(src: &&str, _: &mut Cx<&str>) -> El {
        Markdown::new(*src).embed_links(|_, _| None).build(&Kit::neutral(true))
    }
    fn positions(src: &'static str) -> Vec<f32> {
        let mut rt = Runtime::new(src, scene, (500., 300.), Env::new(true, 1.));
        rt.frame(0.);
        rt.elements()
            .iter()
            .filter(|e| matches!(e.text.as_deref(), Some("before" | "after")))
            .map(|e| e.rect[1])
            .collect()
    }
    assert_eq!(positions("before\n\n[](media)\n\nafter"), positions("before\n\nafter"));
}

#[test]
fn empty_labels_in_nonempty_prose_invoke_each_callback() {
    fn scene(_: &(), _: &mut Cx<()>) -> El {
        Markdown::new("x [](media) [](media)")
            .embed_links(|_, label| {
                assert_eq!(label, "");
                Some(text("player"))
            })
            .build(&Kit::neutral(true))
    }
    let mut rt = Runtime::new((), scene, (500., 300.), Env::new(true, 1.));
    rt.frame(0.);
    assert_eq!(
        rt.elements().iter().filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("md-embed-"))).count(),
        2
    );
}
