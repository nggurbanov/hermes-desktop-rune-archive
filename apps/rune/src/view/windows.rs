use crate::app::Viewer;
use rune::El;
use rune_kit::{Kit, Menu, MenuItem};
pub fn menu(s: &Viewer, k: &Kit, layer: &mut Vec<El>) {
    let Some(point) = s.window_menu() else { return };
    let menu = Menu::new("hermes-window-actions")
        .open(true)
        .at(point.x, point.y)
        .on_close(|s: &mut Viewer| s.close_window_menu())
        .item(
            MenuItem::new("New conversation").on_select(|s: &mut Viewer| {
                s.close_window_menu();
                s.new_draft();
            }),
        )
        .item(
            MenuItem::new("New window")
                .disabled(!s.can_open_window())
                .on_select(|s: &mut Viewer| s.request_new_window()),
        )
        .item(MenuItem::new("Settings").on_select(|s: &mut Viewer| {
            s.close_window_menu();
            s.open_route(crate::pages::Route::Settings);
        }))
        .item(MenuItem::new("Close window").on_select(|s: &mut Viewer| s.request_window_close()))
        .item(MenuItem::new("Quit Hermes").on_select(|s: &mut Viewer| s.request_application_quit()))
        .build(k, layer);
    layer.push(menu);
}
