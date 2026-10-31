use super::*;
use rune::file_dialog::{FileDialogKind, MultiFileDialogResult, TestPresenter};
use std::{cell::RefCell, future::poll_fn, path::PathBuf, rc::Rc, task::Poll};

fn mounted() -> (Runtime<Viewer>, NativeSelfViewer) {
    mounted_home(
        PathBuf::from(std::env::var_os("TMPDIR").unwrap()),
        "/private/root".into(),
        "fixture".into(),
    )
}

fn mounted_home(
    home: PathBuf,
    profile_id: String,
    instance_id: String,
) -> (Runtime<Viewer>, NativeSelfViewer) {
    use crate::gateway::{
        BackendRootProof,
        prepared::{BackendTarget, SelectedContext, WirePolicy},
        route,
    };
    let policy = route::ProfilePolicy::new("default", route::RouteOptions::default()).unwrap();
    let policy = WirePolicy::native(policy)
        .with_selected(
            SelectedContext::new(
                "default",
                BackendTarget::Root(BackendRootProof {
                    profile: None,
                    profile_id,
                    instance_id,
                    authority_epoch: 7,
                }),
            )
            .unwrap(),
        )
        .unwrap();
    let connection = Connection::Routed {
        actual: Box::new(Connection::Local { home }),
        policy: Arc::new(policy),
        owner: Arc::new(crate::source::RouteOwner::legacy("presenter-fixture").unwrap()),
    };
    let mut fixture = NativeSelfViewer::new(connection);
    fixture
        .viewer
        .fleet_state
        .projects_network
        .init(Arc::new(|| {}));
    fixture
        .viewer
        .fleet_state
        .project_create_network
        .init(Arc::new(|| {}));
    fixture
        .viewer
        .fleet_state
        .project_idea_network
        .init(Arc::new(|| {}));
    let (owner, binding) = fixture.viewer.project_owner().unwrap();
    assert!(matches!(
        binding.connection().transport(),
        Connection::Local { .. }
    ));
    assert!(binding.lease().unwrap().is_valid());
    fixture.viewer.fleet_state.projects.bind(owner);
    let (ticket, _) = fixture
        .viewer
        .fleet_state
        .projects
        .begin(LoadKind::Tree, &binding)
        .unwrap();
    assert!(fixture.viewer.fleet_state.projects.finish(
        ticket,
        &binding,
        Ok(json!({"projects":[]}))
    ));
    fixture.viewer.sidebar_state.prefs.grouping = crate::sidebar::Grouping::Project;
    let viewer = std::mem::replace(&mut fixture.viewer, Viewer::empty());
    (
        Runtime::new(
            viewer,
            native_create_view,
            (1200., 1000.),
            Env::new(true, 1.),
        ),
        fixture,
    )
}

fn choose(
    rt: &mut Runtime<Viewer>,
    presenter: &mut TestPresenter,
) -> (u64, Rc<RefCell<Option<MultiFileDialogResult>>>) {
    click(rt, "project-create-choose");
    rt.update(|v, tx| {
        v.poll_tools(1.0, tx);
    });
    let result = Rc::new(RefCell::new(None));
    let input = result.clone();
    let (id, kind, options) = presenter.open(
        rt,
        poll_fn(move |_| match input.borrow_mut().take() {
            Some(value) => Poll::Ready(value),
            None => Poll::Pending,
        }),
    );
    assert_eq!(kind, FileDialogKind::Folders);
    assert_eq!(options, Default::default());
    assert!(!presenter.answer(rt));
    (id, result)
}

#[test]
fn mounted_local_presenter_multiple_folders() {
    let (mut rt, _fixture) = mounted();
    let mut presenter = TestPresenter::new(Arc::new(|| {}));
    click(&mut rt, "project-create-header");
    let dialog = rt.state.project_create_form().unwrap().dialog;
    let (id, result) = choose(&mut rt, &mut presenter);
    let pick = rt.state.project_create_form().unwrap().active_pick.unwrap();
    assert_eq!(pick.dialog, dialog);
    assert!(id > 0);
    let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
        .join(format!("presenter-{}", std::process::id()));
    let a = root.join("folderA");
    let b = root.join("folderB");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    *result.borrow_mut() = Some(Ok(Some(vec![a.clone(), b.clone(), a.clone()])));
    assert!(presenter.answer(&mut rt));
    rt.frame(1.);
    let form = rt.state.project_create_form().unwrap();
    assert_eq!(form.folders, [a.to_string_lossy(), b.to_string_lossy()]);
    assert_eq!(form.name.text, "folderA");
    assert_eq!(form.active_pick, None);
    let labels: Vec<_> = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.text.or(e.label))
        .collect();
    assert!(labels.iter().any(|s| s.contains(a.to_str().unwrap())));
    assert!(labels.iter().any(|s| s.contains(b.to_str().unwrap())));
    assert!(labels.iter().any(|s| s.contains("Primary")));
}

#[test]
fn mounted_local_presenter_late_result_cannot_change_replacement() {
    let (mut rt, _fixture) = mounted();
    let mut presenter = TestPresenter::new(Arc::new(|| {}));
    click(&mut rt, "project-create-header");
    let (a_id, a_result) = choose(&mut rt, &mut presenter);
    let a_token = rt.state.project_create_form().unwrap().active_pick.unwrap();
    click(&mut rt, "project-create-cancel");
    click(&mut rt, "project-create-header");
    let b_dialog = rt.state.project_create_form().unwrap().dialog;
    assert_ne!(a_token.dialog, b_dialog);
    click(&mut rt, "project-create-name");
    rt.paste("replacement");
    rt.frame(1.);
    *a_result.borrow_mut() = Some(Ok(Some(vec!["/old".into()])));
    assert!(presenter.answer(&mut rt));
    rt.frame(1.);
    let form = rt.state.project_create_form().unwrap();
    assert_eq!(form.dialog, b_dialog);
    assert_eq!(form.name.text, "replacement");
    assert!(form.folders.is_empty());
    assert_eq!(form.active_pick, None);
    let (b_id, b_result) = choose(&mut rt, &mut presenter);
    let b_token = rt.state.project_create_form().unwrap().active_pick.unwrap();
    assert_ne!(a_id, b_id);
    assert_eq!(b_token.dialog, b_dialog);
    assert_ne!(a_token, b_token);
    *b_result.borrow_mut() = Some(Ok(Some(vec!["/new".into()])));
    assert!(presenter.answer(&mut rt));
    rt.frame(1.);
    let form = rt.state.project_create_form().unwrap();
    assert_eq!(form.name.text, "replacement");
    assert_eq!(form.folders, ["/new"]);
    assert_eq!(form.active_pick, None);
}

#[test]
#[ignore = "requires the owned private Local gateway fixture"]
fn mounted_local_presenter_submit_private_peer() {
    use std::time::{Duration, Instant};
    let home = PathBuf::from(std::env::var_os("RUNE_TEST_LOCAL_HOME").unwrap());
    let folder = PathBuf::from(std::env::var_os("RUNE_TEST_LOCAL_FOLDER").unwrap());
    let second = PathBuf::from(std::env::var_os("RUNE_TEST_LOCAL_SECOND_FOLDER").unwrap());
    let (mut rt, _fixture) = mounted_home(
        home.clone(),
        home.to_string_lossy().into_owned(),
        "private-fixture-instance".into(),
    );
    let mut presenter = TestPresenter::new(Arc::new(|| {}));
    click(&mut rt, "project-create-header");
    let (_, result) = choose(&mut rt, &mut presenter);
    *result.borrow_mut() = Some(Ok(Some(vec![
        folder.clone(),
        second.clone(),
        folder.clone(),
    ])));
    assert!(presenter.answer(&mut rt));
    rt.frame(1.);
    let form = rt.state.project_create_form().unwrap();
    assert_eq!(
        form.folders,
        [folder.to_string_lossy(), second.to_string_lossy()]
    );
    assert_eq!(
        form.name.text,
        folder.file_name().unwrap().to_string_lossy()
    );
    click(&mut rt, "project-create-idea");
    rt.paste("  An actual local idea  ");
    rt.frame(1.);
    assert_eq!(
        rt.state.project_create_form().unwrap().idea.text.trim(),
        "An actual local idea"
    );
    click(&mut rt, "project-create-submit");
    assert!(
        rt.state.project_create_busy(),
        "submit did not start: {:?}",
        rt.state.project_create_form().map(|f| &f.error)
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        rt.update(|v, _| {
            v.poll_projects();
        });
        let state = rt.state.projects_state();
        if state.scope == Scope::Project("fixture-project".into())
            && state
                .entered
                .as_ref()
                .is_some_and(|p| p.path.as_deref() == folder.to_str())
            && state.tree.as_ref().is_some_and(|t| {
                t.active_id.as_deref() == Some("fixture-project")
                    && t.projects
                        .iter()
                        .any(|p| p.id == "fixture-project" && p.path.as_deref() == folder.to_str())
            })
            && std::fs::read(folder.join("IDEA.md")).is_ok()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "create/refresh timed out: project={:?}, form={:?}",
            state.error,
            rt.state.project_create_form().map(|f| &f.error)
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        std::fs::read(folder.join("IDEA.md")).unwrap(),
        b"An actual local idea\n"
    );
    let target = rt.state.project_draft_target().unwrap().unwrap();
    assert!(
        matches!(target.cwd, crate::projects::CwdChoice::Path(ref p) if p == folder.to_str().unwrap())
    );
}
