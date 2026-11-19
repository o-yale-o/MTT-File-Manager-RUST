//! Tab bar UI component for MTT File Manager
//!
//! Renders a Windows 11/Files-style tab bar with:
//! - Tab icons and titles
//! - Close buttons
//! - New tab button (+)
//! - Window controls (minimize, maximize, close)
//! - Custom title bar with drag area
//! - Auto-resizing tabs

use crate::tabs::TabManager;
use crate::ui::icon_loader::IconLoader;
use crate::ui::svg_icons::SvgIconManager;
use eframe::egui::{self, Color32};

mod appearance;
pub(crate) mod context_menu;
#[cfg(test)]
mod context_menu_tests;
pub(crate) mod detach;
mod drag_dwell;
mod icon_style;
mod new_tab_area;
mod reorder;
mod strip;
#[cfg(test)]
mod strip_tests;
mod tabs_renderer;
mod window_controls;
#[cfg(test)]
mod window_controls_tests;

pub(crate) use drag_dwell::should_activate_tab_on_drag_hover;
pub(crate) use icon_style::tab_icon_style;

/// Result of tab bar interaction
#[derive(Debug)]
pub enum TabBarAction {
    None,
    SwitchTab(usize),
    CloseTab(usize),
    Context(usize, crate::tabs::TabAction),
    /// Close every tab except the given index (the list's "鍏跺畠" entry).
    CloseOtherTabs(usize),
    MoveTab { from: usize, gap: usize },
    DetachTab(usize),
    NewTab,
    CloseApp,
    ToggleMaximize,
    Minimize,
    ToggleMute(usize), // Tab index
}

/// Render the tab bar with custom title bar (Windows 11 style)
#[allow(clippy::too_many_arguments)]
pub fn render_tab_bar(
    ui: &mut egui::Ui,
    tab_manager: &TabManager,
    tag_definitions: &rustc_hash::FxHashMap<i64, crate::domain::file_tag::FileTag>,
    pinned_folders: &[crate::domain::pinned_folder::PinnedFolder],
    live_path: &str,
    tree_state: &crate::app::state::sidebar_tree_state::SidebarTreeState,
    svg_icons: &mut SvgIconManager,
    _frame: &mut eframe::Frame,
    computer_icon: Option<&egui::TextureHandle>,
    icon_loader: &mut IconLoader,
    media_owner_id: Option<usize>,
    is_playing: bool,
    is_muted: bool,
    is_item_dragging: bool,
    auto_expand_tree: bool,
) -> TabBarAction {
    let ctx = ui.ctx().clone();
    let mut action = TabBarAction::None;

    let tab_height = 36.0; // Slightly taller for integrated title bar
    // Win11 keeps ~10px of caption above the tab pills.
    let tab_top_padding = 10.0;
    let tab_padding = 8.0;
    let close_btn_size = 16.0;

    // Colors based on theme (Windows Explorer style - gray tones).
    // Win11 dark structure: strip empty area + inactive tabs use the CONTENT
    // base (閳湩GB 25/32); the selected tab and the toolbar row below share one
    // brighter block (閳湩GB 44) so the active tab visually merges into it.
    let is_dark = ui.visuals().dark_mode;
    let active_bg = if is_dark {
        // Selected tab: Win11's cool-tinted step (閳湩GB 26,32,44) for the
        // system palette; other themes keep their own surface tone.
        crate::ui::theme::palette::tab_active(ui.ctx(), Color32::from_rgb(44, 44, 44))
    } else {
        crate::ui::theme::palette::tab_active(ui.ctx(), Color32::from_rgb(243, 243, 243))
    };
    let inactive_bg = if is_dark {
        crate::ui::theme::palette::background(ui.ctx(), Color32::from_rgb(25, 25, 25))
    } else {
        crate::ui::theme::palette::background(ui.ctx(), Color32::from_rgb(230, 230, 230))
    };
    // Use opaque hover colors in tab bar to avoid tone mismatch with rounded-tab dual-pass paint.
    let hover_bg = if is_dark {
        crate::ui::theme::palette::active(ui.ctx(), Color32::from_rgb(56, 56, 56))
    } else {
        crate::ui::theme::palette::active(ui.ctx(), Color32::from_rgb(216, 216, 216))
    };
    let text_color = if is_dark {
        crate::ui::theme::text_color(ui.ctx())
    } else {
        Color32::from_rgb(30, 30, 30)
    };
    let inactive_text = if is_dark {
        crate::ui::theme::secondary_text_color(ui.ctx())
    } else {
        Color32::from_rgb(100, 100, 100)
    };

    let num_tabs = tab_manager.tabs.len();
    let (bar_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), tab_height + tab_top_padding),
        egui::Sense::hover(),
    );
    let layout = strip::StripLayout::new(bar_rect, num_tabs, tab_top_padding);
    let tab_reorder_active = ctx
        .data(|data| data.get_temp::<usize>(egui::Id::new("tab_reorder_drag")))
        .is_some();

    let tabs_action = strip::show(
        ui,
        &layout,
        tab_manager.active().id,
        tab_manager.active_tab,
        num_tabs,
        |ui, reveal_active| {
            tabs_renderer::render_tabs(
                ui,
                tab_manager,
                tag_definitions,
                pinned_folders,
                live_path,
                tree_state,
                svg_icons,
                computer_icon,
                icon_loader,
                media_owner_id,
                is_playing,
                is_muted,
                is_item_dragging,
                auto_expand_tree,
                layout.tab_width,
                reveal_active,
                tab_height,
                tab_padding,
                close_btn_size,
                active_bg,
                inactive_bg,
                hover_bg,
                text_color,
                inactive_text,
                is_dark,
            )
        },
    );
    if !matches!(tabs_action, TabBarAction::None) {
        action = tabs_action;
    }

    // App logo pinned to the strip's left edge (Win11/Edge style). It marks
    // the caption zone and guarantees a draggable handle even when
    // overflowing tabs swallow the whole strip.
    ui.scope_builder(egui::UiBuilder::new().max_rect(layout.logo), |ui| {
        ui.shrink_clip_rect(layout.logo);
        use crate::infrastructure::windows::window_subclass::{
            clear_caption_drag_region_alt, is_native_caption_drag_enabled,
            set_caption_drag_region_alt_px,
        };
        let popup_open = egui::Popup::is_any_open(&ctx);
        let native_caption_drag = is_native_caption_drag_enabled();
        let sense = if native_caption_drag {
            egui::Sense::hover()
        } else {
            egui::Sense::click_and_drag()
        };
        let (_logo_rect, logo_response) = ui.allocate_exact_size(
            egui::vec2(layout.logo.width(), layout.logo.height()),
            sense,
        );

        // While a tab reorder or a popup is active the logo stays client area,
        // mirroring the new-tab drag strip.
        if tab_reorder_active || tab_reorder_active || popup_open {
            clear_caption_drag_region_alt();
        } else {
            let ppp = ctx.pixels_per_point();
            set_caption_drag_region_alt_px(
                (layout.logo.min.x * ppp).round() as i32,
                (layout.logo.min.y * ppp).round() as i32,
                (layout.logo.width() * ppp).round() as i32,
                (layout.logo.height() * ppp).round() as i32,
            );
        }

        // Fallback path when native caption drag is disabled.
        if !tab_reorder_active
            && !tab_reorder_active
            && !popup_open
            && !native_caption_drag
            && (logo_response.drag_started() || logo_response.dragged())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        if let Some(icon) =
            crate::app::operations::context_menu::app_icon_texture(&ctx)
        {
            let size = 16.0;
            let center = egui::pos2(
                layout.logo.left() + 10.0 + size * 0.5,
                bar_rect.top() + tab_top_padding + tab_height * 0.5,
            );
            ui.painter().image(
                icon.id(),
                egui::Rect::from_center_size(center, egui::vec2(size, size)),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    });

    ui.scope_builder(
        egui::UiBuilder::new().max_rect(layout.new_tab_and_drag),
        |ui| {
            ui.shrink_clip_rect(layout.new_tab_and_drag);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if new_tab_area::render_new_tab_and_drag_area(
                    ui,
                    &ctx,
                    tab_height,
                    0.0,
                    inactive_bg,
                    hover_bg,
                    text_color,
                    tab_reorder_active,
                ) && !tab_reorder_active
                {
                    action = TabBarAction::NewTab;
                }
            });
        },
    );

    ui.scope_builder(egui::UiBuilder::new().max_rect(layout.controls), |ui| {
        ui.shrink_clip_rect(layout.controls);
        let tab_titles: Vec<String> = tab_manager
            .tabs
            .iter()
            .map(|tab| {
                // Same display mapping the tab strip uses: virtual views
                // (computer, recycle bin, tag filters, OneDrive specials)
                // get their localized names instead of raw ids like
                // "::computer::".
                appearance::display_title_for_tab_path(
                    &tab.path,
                    &tab.title,
                    tag_definitions,
                )
            })
            .collect();
        ui.horizontal(|ui| {
            window_controls::render_window_controls(
                ui,
                strip::WINDOW_BUTTON_WIDTH,
                tab_height,
                tab_top_padding,
                &tab_titles,
                tab_manager.active_tab,
                &mut action,
            )
        });
    });

    action
}
