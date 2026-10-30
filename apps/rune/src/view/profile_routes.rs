//! The same profile actions belong to the rail and both profile picker presentations.
use crate::app::Viewer;
use rune::El;
use rune_kit::{Kit, Menu, MenuItem};

pub fn menu(s: &Viewer, k: &Kit, layer: &mut Vec<El>) {
    let Some(target) = s.profile_route_menu() else {
        return;
    };
    let is_default = s.is_default_profile(&target.profile);
    let can_set = s.can_set_default_profile(&target.profile);
    let policy_disabled = s.is_fixture()
        || s.connections.busy
        || s.connections.editor.is_some()
        || !s.connections.registry_loaded();
    let mut menu = Menu::new("profile-route-actions")
        .open(true)
        .min_width(260.)
        .at(target.point.x, target.point.y)
        .on_close(|s: &mut Viewer| s.close_profile_route_menu())
        .item(
            MenuItem::new("Open in new window")
                .disabled(!s.can_open_window())
                .on_select(|s: &mut Viewer| s.open_window_from_profile_menu()),
        )
        .item(
            MenuItem::new(if is_default {
                "Default profile"
            } else {
                "Set as default"
            })
            .checked(is_default)
            .disabled(is_default || !can_set || policy_disabled)
            .on_select(|s: &mut Viewer| s.set_default_from_profile_menu()),
        );
    if is_default {
        menu = menu.item(
            MenuItem::new("Clear default")
                .disabled(policy_disabled)
                .on_select(|s: &mut Viewer| s.clear_default_profile()),
        );
    }
    if !is_default
        && !can_set
        && !s.is_fixture()
        && s.current_connection().is_some_and(|connection| {
            !matches!(connection, crate::gateway::Connection::Saved { .. })
        })
    {
        menu = menu.group_label("Save connection to set a default");
    }
    let built = menu.build(k, layer);
    layer.push(built);
}
