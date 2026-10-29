//! Full workspace pages keep the Hermes sidebar and connection/profile owner.
pub mod artifacts;
pub mod bots;
pub mod capabilities;
pub mod cron;
pub mod messaging;
pub mod settings;
use crate::{app::Viewer, pages::Route};
use rune::{El, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit};
pub fn page(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let body = match s.route {
        Route::CommandCenter => super::operations::page(s, k, layer),
        Route::Settings => settings::page(s, k, layer),
        Route::Bots => bots::page(s, k, layer),
        Route::Connections => super::connections::panel(s, k, layer),
        Route::FilePreview => {
            if let Some(id) = s.audio_preview {
                super::media_player::panel(s, id, k)
            } else if s.preview.active().is_some() {
                super::preview::panel(s, k)
            } else {
                super::files::preview(s, k)
            }
        }
        Route::Capabilities => capabilities::page(s, k, layer),
        Route::Cron => cron::page(s, k, layer),
        Route::Messaging => messaging::page(s, k, layer),
        Route::Artifacts => artifacts::page(s, k, layer),
        Route::Chat => el(),
    };
    col()
        .key("workspace-page")
        .w_full()
        .h_full()
        .min_h(0.0)
        .instant_changes()
        .child(
            row()
                .w_full()
                .min_h(42.0)
                .px(16.0)
                .gap(12.0)
                .center_y()
                .child(
                    Button::new("Back to chat")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut Viewer| s.open_route(Route::Chat))
                        .build(k)
                        .key("page-back"),
                )
                .child(el().grow(1.0))
                .child(
                    text(format!("Profile: {}", s.profile))
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                ),
        )
        .child(body.grow(1.0).min_h(0.0))
}
