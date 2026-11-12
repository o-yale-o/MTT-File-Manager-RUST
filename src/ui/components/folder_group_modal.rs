//! Modal that names the group when saving all open tabs (页签右键 → 收藏所有页签为分组).
use crate::app::ImageViewerApp;
use eframe::egui;
use rust_i18n::t;

pub fn render_save_group_modal(app: &mut ImageViewerApp, ctx: &egui::Context) {
    if !app.show_save_group_dialog {
        return;
    }
    let tab_count = app.workspace_tab_paths().len();
    let mut confirmed = false;
    let mut cancelled = false;

    egui::Window::new(t!("tabs.save_group_title"))
        .id(egui::Id::new("save_tabs_group_window"))
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.label(t!("tabs.save_group_count", count = tab_count));
            ui.add_space(8.0);
            let name_response = ui.add(
                egui::TextEdit::singleline(&mut app.save_group_name)
                    .hint_text(t!("tabs.group_default_name"))
                    .desired_width(ui.available_width()),
            );
            // Focus once — re-requesting every frame resets IME composition
            // (中文输入法组字会被打断并闪烁), so only steal focus while the
            // dialog just opened and no other widget holds it.
            let never_focused = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(egui::Id::new("save_group_focus")).is_none());
            if never_focused && !ui.memory(|memory| memory.focused().is_some()) {
                name_response.request_focus();
                ui.ctx()
                    .data_mut(|data| data.insert_temp(egui::Id::new("save_group_focus"), true));
            }
            confirmed |= name_response.lost_focus()
                && ui.input(|input| input.key_pressed(egui::Key::Enter));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(t!("tags.confirm")).clicked() {
                    confirmed = true;
                }
                if ui.button(t!("tags.cancel")).clicked() {
                    cancelled = true;
                }
            });
            cancelled |= ui.input(|input| input.key_pressed(egui::Key::Escape));
        });

    if confirmed {
        app.confirm_save_tabs_as_group();
    } else if cancelled {
        app.cancel_save_tabs_as_group();
    }
    if !app.show_save_group_dialog {
        ctx.data_mut(|data| {
            data.remove::<bool>(egui::Id::new("save_group_focus"));
        });
    }
}
