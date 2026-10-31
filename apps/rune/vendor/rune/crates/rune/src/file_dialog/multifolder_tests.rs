use super::*;
use crate::{Cx, El, Env, Runtime, el};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct State {
    received: Vec<MultiFileDialogResult>,
    single: Vec<FileDialogResult>,
}
fn view(_: &State, _: &mut Cx<State>) -> El {
    el()
}
fn runtime() -> Runtime<State> {
    let mut rt = Runtime::new(State::default(), view, (200.0, 100.0), Env::new(true, 1.0));
    rt.frame(0.0);
    rt
}
fn presenter() -> Presenter {
    Presenter::new(Arc::new(|| {}))
}
fn request(rt: &mut Runtime<State>) {
    rt.update(|_, tx| {
        tx.open_folders(FileDialogOptions::default(), |s: &mut State, result, _| {
            s.received.push(result)
        })
    });
}
struct Controlled(Rc<RefCell<Option<MultiFileDialogResult>>>);
impl Future for Controlled {
    type Output = MultiFileDialogResult;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0.borrow_mut().take() {
            Some(result) => Poll::Ready(result),
            None => Poll::Pending,
        }
    }
}
fn attach(
    p: &mut Presenter,
    rt: &mut Runtime<State>,
) -> (u64, Rc<RefCell<Option<MultiFileDialogResult>>>) {
    let result = Rc::new(RefCell::new(None));
    let id = p.open_test(rt, Controlled(result.clone()));
    (id, result)
}
#[test]
fn presenter_completes_only_its_runtime_once_and_ignores_late_ids() {
    let (mut a, mut b) = (runtime(), runtime());
    let (mut pa, mut pb) = (presenter(), presenter());
    request(&mut a);
    request(&mut b);
    let (id_a, result_a) = attach(&mut pa, &mut a);
    let (id_b, result_b) = attach(&mut pb, &mut b);
    assert_eq!(id_a, id_b);
    *result_a.borrow_mut() = Some(Ok(Some(vec!["/one".into(), "/two".into()])));
    assert!(pa.answer(&mut a));
    assert!(!pa.answer(&mut a));
    a.answer_file_dialog(id_a, Ok(Some(vec!["/duplicate".into()])));
    a.answer_file_dialog(id_a + 100, Ok(None));
    a.frame(0.1);
    b.frame(0.1);
    assert_eq!(
        a.state.received,
        vec![Ok(Some(vec!["/one".into(), "/two".into()]))]
    );
    assert!(b.state.received.is_empty());
    *result_b.borrow_mut() = Some(Ok(Some(vec![])));
    assert!(pb.answer(&mut b));
    b.frame(0.2);
    assert_eq!(b.state.received, vec![Ok(Some(vec![]))]);
}
#[test]
fn dropped_window_and_cancelled_presenter_cannot_deliver_late_result() {
    let (mut a, mut b) = (runtime(), runtime());
    let (mut pa, mut pb) = (presenter(), presenter());
    request(&mut a);
    request(&mut b);
    let (_, late) = attach(&mut pa, &mut a);
    let (_, current) = attach(&mut pb, &mut b);
    drop(pa);
    drop(a);
    *late.borrow_mut() = Some(Ok(Some(vec!["/late".into()])));
    assert!(!pb.answer(&mut b));
    *current.borrow_mut() = Some(Ok(None));
    assert!(pb.answer(&mut b));
    b.frame(0.1);
    assert_eq!(b.state.received, vec![Ok(None)]);
    request(&mut b);
    let (_, cancelled) = attach(&mut pb, &mut b);
    pb.cancel(&mut b);
    *cancelled.borrow_mut() = Some(Ok(Some(vec!["/late".into()])));
    assert!(!pb.answer(&mut b));
    b.frame(0.2);
    assert_eq!(b.state.received, vec![Ok(None), Ok(None)]);
}
#[test]
fn completion_callback_can_open_another_dialog_and_errors_are_delivered() {
    let mut rt = runtime();
    let mut p = presenter();
    rt.update(|_, tx| {
        tx.open_folders(FileDialogOptions::default(), |s: &mut State, result, tx| {
            s.received.push(result);
            tx.open_folders(FileDialogOptions::default(), |s: &mut State, result, _| {
                s.received.push(result)
            });
        })
    });
    let (old, first) = attach(&mut p, &mut rt);
    *first.borrow_mut() = Some(Ok(None));
    assert!(p.answer(&mut rt));
    rt.frame(0.1);
    let (next, second) = attach(&mut p, &mut rt);
    assert_ne!(old, next);
    rt.answer_file_dialog(old, Ok(Some(vec!["/late".into()])));
    *second.borrow_mut() = Some(Err(FileDialogError("no portal".into())));
    assert!(p.answer(&mut rt));
    rt.frame(0.2);
    assert_eq!(
        rt.state.received,
        vec![Ok(None), Err(FileDialogError("no portal".into()))]
    );
}
#[test]
fn single_folder_rejects_multi_result_and_cancel_remains_generic() {
    let mut rt = runtime();
    rt.update(|_, tx| {
        tx.open_folder(FileDialogOptions::default(), |s: &mut State, answer, _| {
            s.single.push(answer)
        })
    });
    assert!(
        rt.resolve_file_dialogs(Some(vec!["/one".into(), "/two".into()]))
            .is_err()
    );
    rt.resolve_file_dialog(Some("/one".into())).unwrap();
    rt.frame(0.1);
    assert_eq!(rt.state.single, vec![Ok(Some("/one".into()))]);
    request(&mut rt);
    assert!(rt.resolve_file_dialog(Some("/single".into())).is_err());
    rt.resolve_file_dialog(None).unwrap();
    rt.frame(0.2);
    assert_eq!(rt.state.received, vec![Ok(None)]);
}
