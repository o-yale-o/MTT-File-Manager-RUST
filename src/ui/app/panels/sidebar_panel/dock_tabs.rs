//! Left-dock tab strip: 【列表】 / 【资源管理器】.
//!
//! Visuals follow MadEdit's file-view tabs: a 28 px strip with 2 px top
//! padding, rounded top corners only, no bottom border. The selected tab is
//! filled with the pane background so it merges into the content below, and
//! the strip's baseline is interrupted under that tab.

use crate::app::sidebar_dock::SidebarDockTab;
use crate::app::ImageViewerApp;
use eframe::egui;

pub(super) const TAB_HEIGHT: f32 = 28.0;
pub(super) const TAB_STRIP_HEIGHT: f32 = TAB_HEIGHT + 2.0;

const TAB_PAD: f32 = 12.0;
const TAB_MIN_WIDTH: f32 = 72.0;
const TAB_MAX_TEXT_WIDTH: f32 = 220.0;
const TAB_TOP_RADIUS: u8 = 6;
const STRIP_SIDE_INSET: f32 = 8.0;

fn tab_label(tab: SidebarDockTab) -> String {
    match tab {
        SidebarDockTab::List => rust_i18n::t!("sidebar.dock_list").to_string(),
        SidebarDockTab::Explorer => rust_i18n::t!("sidebar.dock_explorer").to_string(),
    }
}

fn strip_background(ui: &egui::Ui, content_bg: egui::Color32) -> egui::Color32 {
    if ui.visuals().dark_mode {
        content_bg
    } else {
        content_bg.gamma_multiply(0.95)
    }
}

/// Draws the strip and reserves its height so the tab content starts below it.
pub(super) fn render(ui: &mut egui::Ui, app: &mut ImageViewerApp, content_bg: egui::Color32) {
    let full = ui.max_rect();
    let strip = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), TAB_STRIP_HEIGHT));
    let visuals = ui.visuals().clone();
    ui.painter()
        .rect_filled(strip, 0.0, strip_background(ui, content_bg));
    ui.allocate_rect(strip, egui::Sense::hover());

    let tabs_rect = egui::Rect::from_min_max(
        egui::pos2(strip.left() + STRIP_SIDE_INSET, strip.top() + 2.0),
        egui::pos2(strip.right() - STRIP_SIDE_INSET, strip.bottom()),
    );
    let mut tab_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("sidebar_dock_tabs")
            .max_rect(tabs_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    tab_ui.spacing_mut().item_spacing.x = 0.0;

    let mut active_rect = None;
    for tab in SidebarDockTab::ALL {
        let selected = app.sidebar_dock_tab == tab;
        let (response, rect) =
            flat_tab(&mut tab_ui, tab, &tab_label(tab), selected, content_bg, &visuals);
        if response.hovered() {
            tab_ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() && !selected {
            app.sidebar_dock_tab = tab;
            // A rename editor belongs to the page it was opened on.
            app.cancel_sidebar_folder_rename();
            app.save_preferences();
            app.save_preferences_async();
        }
        if selected {
            active_rect = Some(rect);
        }
    }
    tab_strip_baseline(ui, strip, active_rect);
}

fn flat_tab(
    ui: &mut egui::Ui,
    tab: SidebarDockTab,
    label: &str,
    selected: bool,
    content_bg: egui::Color32,
    visuals: &egui::Visuals,
) -> (egui::Response, egui::Rect) {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let color = if selected {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    let measured = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), color);
    let width = (measured.size().x.min(TAB_MAX_TEXT_WIDTH) + TAB_PAD * 2.0).max(TAB_MIN_WIDTH);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, TAB_HEIGHT), egui::Sense::hover());
    let response = ui.interact(
        rect,
        ui.id().with(("sidebar_dock_tab", tab as u8)),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });

    let radius = egui::CornerRadius {
        nw: TAB_TOP_RADIUS,
        ne: TAB_TOP_RADIUS,
        sw: 0,
        se: 0,
    };
    if selected {
        ui.painter().rect_filled(rect, radius, content_bg);
        ui.painter().rect_stroke(
            rect,
            radius,
            visuals.widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        );
        // Erase the bottom edge of the outline so the tab continues into the pane.
        ui.painter().rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 1.5), rect.max),
            0.0,
            content_bg,
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, radius, visuals.widgets.hovered.weak_bg_fill);
    }

    ui.painter()
        .text(rect.center(), egui::Align2::CENTER_CENTER, label, font, color);
    (response, rect)
}

/// Thin line under the strip, with a gap under the selected tab.
fn tab_strip_baseline(ui: &egui::Ui, strip: egui::Rect, active: Option<egui::Rect>) {
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    let y = strip.bottom() - 0.5;
    let painter = ui.painter();
    match active
        .map(|rect| rect.intersect(strip))
        .filter(|rect| rect.width() > 0.0)
    {
        Some(active) => {
            if active.left() > strip.left() {
                painter.hline(strip.left()..=active.left(), y, stroke);
            }
            if active.right() < strip.right() {
                painter.hline(active.right()..=strip.right(), y, stroke);
            }
        }
        None => {
            painter.hline(strip.x_range(), y, stroke);
        }
    }
}
