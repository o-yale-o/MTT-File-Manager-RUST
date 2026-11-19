use crate::ui::theme;
use eframe::egui::{self, Color32, CornerRadius, Stroke, Vec2, vec2};

use super::TabBarAction;

/// Render window control buttons (Tab list, Minimize, Maximize, Close)
/// Used for borderless window mode where native decorations are disabled.
///
/// The tab-list button (360-browser style) shows the open tab count in a
/// small framed box; clicking it drops a list of every tab — including ones
/// squeezed out of the overflowing strip — so any tab stays reachable.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_window_controls(
    ui: &mut egui::Ui,
    btn_width: f32,
    btn_height: f32,
    top_padding: f32,
    tab_titles: &[String],
    active_tab: usize,
    action: &mut TabBarAction,
) {
    let is_dark = ui.visuals().dark_mode;
    let text_color = if is_dark {
        theme::text_color(ui.ctx())
    } else {
        Color32::from_rgb(30, 30, 30)
    };
    // Win11: min/max buttons are transparent over the caption strip; only
    // hover feedback and the close-red differ.
    let normal_bg = Color32::TRANSPARENT;
    let hover_bg = if is_dark {
        theme::color_dark_hover()
    } else {
        theme::color_hover()
    };
    let close_hover_bg = Color32::from_rgb(232, 17, 35);

    ui.spacing_mut().item_spacing.x = 0.0;

    let is_maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));

    // The list button only makes sense with two or more tabs.
    let list_visible = tab_titles.len() >= 2;

    // --- Tab list button (360-browser style): framed box with tab count ---
    // Same footprint as the "+" pill (36×36) and top-aligned with it, i.e.
    // it hangs inside the 10px caption padding instead of hugging the very
    // top edge like the min/max/close glyph zones.
    let list_id = ui.make_persistent_id("window_tab_list_button");
    let list_open = list_visible && egui::Popup::is_id_open(ui.ctx(), list_id);
    let scope_rect = ui.max_rect();
    let list_rect = egui::Rect::from_min_size(
        egui::pos2(scope_rect.left(), scope_rect.top() + top_padding),
        Vec2::new(super::strip::NEW_TAB_WIDTH, btn_height),
    );
    let list_response = ui
        .interact(list_rect, list_id.with("hit"), egui::Sense::click())
        .on_hover_text(rust_i18n::t!("tab_bar.manage_tabs"));
    // Advance the layout cursor past the logo-sized zone so min/max/close
    // keep their sequential positions to its right (even when hidden).
    ui.allocate_exact_size(
        Vec2::new(
            if list_visible {
                super::strip::NEW_TAB_WIDTH
            } else {
                0.0
            },
            scope_rect.height(),
        ),
        egui::Sense::hover(),
    );

    if list_visible {
        let list_bg = if list_response.hovered() || list_open {
            hover_bg
        } else {
            normal_bg
        };
        ui.painter().rect_filled(list_rect, 0.0, list_bg);

        // Framed square with the tab count inside.
        let count_text = tab_titles.len().to_string();
        let box_size = 16.0;
        let list_center = list_rect.center();
        let count_rect = egui::Rect::from_center_size(list_center, Vec2::splat(box_size));
        ui.painter().rect_stroke(
            count_rect,
            CornerRadius::same(3),
            Stroke::new(1.0, text_color),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            list_center,
            egui::Align2::CENTER_CENTER,
            count_text,
            egui::FontId::proportional(11.0),
            text_color,
        );

        // Overflow indicator: when tabs are squeezed to their 100px minimum
        // the strip scrolls — hint the user that the full list lives here.
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(count_rect.right() - 3.0, count_rect.bottom() - 3.0),
                Vec2::splat(3.0),
            ),
            CornerRadius::same(1),
            theme::accent_color(ui.ctx()),
        );
    }

    let _ = list_response.clicked(); // handled by the overlay toggle below.

    // --- Tab list overlay ---
    // Hand-drawn Foreground layer (egui's Popup::menu ignored the explicit
    // anchor/width). Fixed spec: 320 wide, right edge flush with the window's
    // right edge, height = tab bar bottom -> status bar top, never shrunk to
    // fit a short list.
    let mut open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(list_id).unwrap_or(false));
    if !list_visible && open {
        open = false;
        ui.ctx().data_mut(|d| d.insert_temp(list_id, false));
    }
    if list_visible && list_response.clicked() {
        open = !open;
        ui.ctx().data_mut(|d| d.insert_temp(list_id, open));
    }
    if open {
        let ctx = ui.ctx().clone();
        let screen = ctx.input(|i| i.raw.screen_rect.unwrap_or(egui::Rect::NOTHING));
        let status_bar_rect: Option<egui::Rect> = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("status_bar_rect")));
        const LIST_WIDTH: f32 = 320.0;
        // Anchor just below the button itself (never higher), so the panel
        // cannot cover the button row regardless of the scope's own extent.
        let list_top = list_rect.bottom() + 2.0;
        let list_height = match status_bar_rect {
            Some(bar) => (bar.top() - 2.0 - list_top).max(120.0),
            None => (screen.bottom() - 8.0 - list_top).max(120.0),
        };
        let panel_rect = egui::Rect::from_min_size(
            egui::pos2((screen.right() - LIST_WIDTH).max(screen.left()), list_top),
            egui::vec2(LIST_WIDTH, list_height),
        );

        // Backdrop: any click outside the list closes it.
        let mut close_from_backdrop = false;
        egui::Area::new(list_id.with("backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Foreground)
            .show(&ctx, |ui| {
                ui.set_min_size(screen.size());
                let resp = ui.interact(
                    ui.max_rect(),
                    list_id.with("backdrop_interact"),
                    egui::Sense::click(),
                );
                if resp.clicked() {
                    close_from_backdrop = true;
                }
            });

        let mut picked: Option<usize> = None;
        let mut picked_close: Option<usize> = None;
        let mut close_others = false;
        egui::Area::new(list_id)
            .fixed_pos(panel_rect.min)
            .order(egui::Order::Foreground)
            .show(&ctx, |ui| {
                let (panel_resp_rect, _) = ui.allocate_exact_size(
                    egui::vec2(LIST_WIDTH, list_height),
                    egui::Sense::hover(),
                );
                let is_dark = ui.visuals().dark_mode;
                let bg = if is_dark {
                    Color32::from_rgb(32, 32, 32)
                } else {
                    Color32::from_rgb(249, 249, 249)
                };
                let border = if is_dark {
                    Color32::from_rgb(63, 63, 63)
                } else {
                    Color32::from_rgb(202, 202, 202)
                };
                ui.painter()
                    .rect_filled(panel_resp_rect, CornerRadius::same(6), bg);
                ui.painter().rect_stroke(
                    panel_resp_rect,
                    CornerRadius::same(6),
                    Stroke::new(1.0, border),
                    egui::StrokeKind::Outside,
                );

                let mut list_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(panel_resp_rect.shrink2(egui::vec2(4.0, 4.0))),
                );
                egui::ScrollArea::vertical()
                    .id_salt("window_tab_list_scroll")
                    .max_height(list_height - 8.0)
                    .auto_shrink([false, false])
                    .animated(false)
                    .show(&mut list_ui, |ui| {
                        ui.set_min_width(LIST_WIDTH - 8.0);
                        ui.set_min_height(list_height - 8.0);
                        // Rows are drawn 360-browser style: one unified row —
                        // hover/selection fills the whole row (folder name +
                        // "×" share the same background), the "×" only exists
                        // while the row is hovered, and hovering the "×" turns
                        // it red. The row background is painted through a
                        // row-scoped painter BEFORE the children so it never
                        // covers them, and the children keep the standard
                        // widget hit-testing (no manual detection).
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let content_left = ui.max_rect().left();
                        const ROW_HEIGHT: f32 = 26.0;
                        const X_ZONE: f32 = 26.0;
                        const RED: Color32 = Color32::from_rgb(232, 17, 35);

                        // Fixed first entry: grey "其它" — the label is inert;
                        // its "×" closes every tab except the active one.
                        {
                            let row_rect = egui::Rect::from_min_size(
                                egui::pos2(content_left, ui.cursor().top()),
                                egui::vec2(ui.available_width(), ROW_HEIGHT),
                            );
                            let row_hovered = ui.rect_contains_pointer(row_rect);
                            let painter = ui.painter_at(row_rect);
                            if row_hovered {
                                painter.rect_filled(row_rect, CornerRadius::same(4), hover_bg);
                            }
                            let grey = theme::secondary_text_color(ui.ctx());
                            let x_rect = egui::Rect::from_center_size(
                                egui::pos2(
                                    row_rect.right() - X_ZONE / 2.0,
                                    row_rect.center().y,
                                ),
                                vec2(X_ZONE, ROW_HEIGHT),
                            );
                            let x_color =
                                if row_hovered && ui.rect_contains_pointer(x_rect) {
                                    RED
                                } else {
                                    grey
                                };
                            let text = egui::RichText::new(
                                rust_i18n::t!("tab_bar.others").to_string(),
                            )
                            .color(grey)
                            .size(13.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let label = ui.add_sized(
                                    [row_rect.width() - X_ZONE, ROW_HEIGHT],
                                    egui::Button::selectable(false, text.clone())
                                        .frame(false)
                                        .left_text(text),
                                );
                                let _ = label.clicked(); // inert by design
                                if row_hovered {
                                    let x_btn = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("✕").size(11.0).color(x_color),
                                        )
                                        .small()
                                        .frame(false)
                                        .min_size(vec2(X_ZONE, ROW_HEIGHT)),
                                    );
                                    if x_btn.clicked() {
                                        close_others = true;
                                    }
                                }
                            });
                        }

                        for (index, title) in tab_titles.iter().enumerate() {
                            let is_active = index == active_tab;
                            let row_rect = egui::Rect::from_min_size(
                                egui::pos2(content_left, ui.cursor().top()),
                                egui::vec2(ui.available_width(), ROW_HEIGHT),
                            );
                            let row_hovered = ui.rect_contains_pointer(row_rect);
                            let painter = ui.painter_at(row_rect);
                            if is_active {
                                painter.rect_filled(
                                    row_rect,
                                    CornerRadius::same(4),
                                    theme::selection_color(ui.ctx()),
                                );
                            } else if row_hovered {
                                painter.rect_filled(row_rect, CornerRadius::same(4), hover_bg);
                            }
                            let x_hovered = row_hovered
                                && ui.rect_contains_pointer(egui::Rect::from_center_size(
                                    egui::pos2(
                                        row_rect.right() - X_ZONE / 2.0,
                                        row_rect.center().y,
                                    ),
                                    vec2(X_ZONE, ROW_HEIGHT),
                                ));
                            let base_color = if is_active {
                                theme::selection_text_color(ui.ctx())
                            } else {
                                text_color
                            };
                            let x_color = if x_hovered { RED } else { base_color };
                            let text =
                                egui::RichText::new(title.as_str()).size(13.0).color(base_color);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let label = ui.add_sized(
                                    [row_rect.width() - X_ZONE, ROW_HEIGHT],
                                    egui::Button::selectable(is_active, text.clone())
                                        .frame(false)
                                        .left_text(text),
                                );
                                if label.clicked() {
                                    picked = Some(index);
                                }
                                // The "×" shows on the hovered row and on the
                                // active (selected) row, 360-browser style.
                                if row_hovered || is_active {
                                    let x_btn = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("✕").size(11.0).color(x_color),
                                        )
                                        .small()
                                        .frame(false)
                                        .min_size(vec2(X_ZONE, ROW_HEIGHT)),
                                    );
                                    if x_btn.clicked() {
                                        picked_close = Some(index);
                                    }
                                }
                            });
                        }
                    });

                // egui promotes a clicked Area to the top of its layer order.
                // One stray click on the backdrop (e.g. on a sidebar entry
                // while the list is open) therefore lifts the backdrop ABOVE
                // this panel, and from then on every row click lands on the
                // backdrop — the list only closes and rows go dead. Re-claim
                // the top every frame so the panel always stays above it.
                ctx.memory_mut(|mem| {
                    mem.areas_mut()
                        .move_to_top(egui::LayerId::new(egui::Order::Foreground, list_id))
                });
            });

        // Clicking a row (switch or its "x") keeps the list open; only a
        // click outside the panel (backdrop) or re-clicking the button closes.
        if close_from_backdrop {
            open = false;
            ui.ctx().data_mut(|d| d.insert_temp(list_id, false));
        }
        if let Some(index) = picked {
            if index != active_tab {
                *action = TabBarAction::SwitchTab(index);
            }
        }
        if let Some(index) = picked_close {
            *action = TabBarAction::CloseTab(index);
        }
        if close_others {
            *action = TabBarAction::CloseOtherTabs(active_tab);
        }
    }

    let (min_rect, min_response) =
        ui.allocate_exact_size(Vec2::new(btn_width, btn_height), egui::Sense::click());
    let min_response = min_response.on_hover_text(rust_i18n::t!("tab_bar.minimize"));

    if min_response.clicked() {
        *action = TabBarAction::Minimize;
    }

    let min_bg = if min_response.hovered() {
        hover_bg
    } else {
        normal_bg
    };
    ui.painter().rect_filled(min_rect, 0.0, min_bg);

    let min_center = min_rect.center();
    ui.painter().line_segment(
        [
            min_center + Vec2::new(-5.0, 0.0),
            min_center + Vec2::new(5.0, 0.0),
        ],
        Stroke::new(1.0, text_color),
    );

    let (max_rect, max_response) =
        ui.allocate_exact_size(Vec2::new(btn_width, btn_height), egui::Sense::click());
    let max_response = max_response.on_hover_text(if is_maximized {
        rust_i18n::t!("tab_bar.restore")
    } else {
        rust_i18n::t!("tab_bar.maximize")
    });

    if max_response.clicked() {
        *action = TabBarAction::ToggleMaximize;
    }

    let max_bg = if max_response.hovered() {
        hover_bg
    } else {
        normal_bg
    };
    ui.painter().rect_filled(max_rect, 0.0, max_bg);

    let max_center = max_rect.center();
    let icon_size = 10.0;

    if is_maximized {
        let offset = 2.0;
        ui.painter().rect_stroke(
            egui::Rect::from_center_size(
                max_center + Vec2::new(-offset, offset),
                Vec2::splat(icon_size - 2.0),
            ),
            0.0,
            Stroke::new(1.0, text_color),
            egui::StrokeKind::Inside,
        );
        ui.painter().rect_stroke(
            egui::Rect::from_center_size(
                max_center + Vec2::new(offset, -offset),
                Vec2::splat(icon_size - 2.0),
            ),
            0.0,
            Stroke::new(1.0, text_color),
            egui::StrokeKind::Inside,
        );
    } else {
        ui.painter().rect_stroke(
            egui::Rect::from_center_size(max_center, Vec2::splat(icon_size)),
            0.0,
            Stroke::new(1.0, text_color),
            egui::StrokeKind::Inside,
        );
    }

    let (close_rect, close_response) =
        ui.allocate_exact_size(Vec2::new(btn_width, btn_height), egui::Sense::click());
    let close_response = close_response.on_hover_text(rust_i18n::t!("tab_bar.close"));

    if close_response.clicked() {
        *action = TabBarAction::CloseApp;
    }

    let close_bg = if close_response.hovered() {
        close_hover_bg
    } else {
        normal_bg
    };
    ui.painter().rect_filled(close_rect, 0.0, close_bg);

    let close_center = close_rect.center();
    let x_size = 10.0;
    let x_color = if close_response.hovered() {
        Color32::WHITE
    } else {
        text_color
    };
    let x_stroke = Stroke::new(1.0, x_color);

    ui.painter().line_segment(
        [
            close_center + Vec2::new(-x_size / 2.0, -x_size / 2.0),
            close_center + Vec2::new(x_size / 2.0, x_size / 2.0),
        ],
        x_stroke,
    );
    ui.painter().line_segment(
        [
            close_center + Vec2::new(x_size / 2.0, -x_size / 2.0),
            close_center + Vec2::new(-x_size / 2.0, x_size / 2.0),
        ],
        x_stroke,
    );
}
