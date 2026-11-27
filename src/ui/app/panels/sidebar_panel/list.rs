//! 【历史文件夹】 page of the left dock, modeled on MadEdit's file view:
//!
//! * a 快速打开 root that expands into groups of folders,
//! * each group expands into its folders (click navigates, double-click on the
//!   group opens every folder as a tab),
//! * a sibling 最近访问 root with per-row removal.
//!
//! MadEdit records recently opened files; this file manager navigates by
//! folder, so the equivalents here are folders and folder paths.

use crate::app::favorites_state::FavoritesRenameTarget;
use crate::app::ImageViewerApp;
use crate::domain::folder_group::FolderGroup;
use crate::ui::sidebar::SidebarAction;
use eframe::egui;
use rust_i18n::t;

const ROW_HEIGHT: f32 = 24.0;
const GROUP_INDENT: f32 = 12.0;
/// Leaf rows align their text with the category name above them (arrow + folder
/// icon column), so the tree reads as parent/child at a glance.
const ITEM_INDENT: f32 = 46.0;
const RECENT_ITEM_INDENT: f32 = 35.0;
const RECENT_LIMIT: usize = 15;
/// Width of the drawn folder icon column.
const FOLDER_ICON_WIDTH: f32 = 20.0;
/// Space reserved at the right edge so labels, counts and the × never sit on
/// top of the scrollbar.
const SCROLLBAR_GAP: f32 = 14.0;
/// Width of the hover-only "×" hit area, matching MadEdit's close column.
const CLOSE_WIDTH: f32 = 20.0;
/// Red used by MadEdit for the hovered close button.
const CLOSE_HOVER_TINT: egui::Color32 = egui::Color32::from_rgb(232, 17, 35);

enum ListAction {
    ToggleRoot,
    ToggleGroup(i64),
    OpenGroup(i64),
    /// Activate an existing tab for this folder, or open a new one.
    OpenFolder { group_id: Option<i64>, path: String },
    RemoveItem { group_id: i64, path: String },
    BeginRenameGroup(i64),
    BeginRenameItem { group_id: i64, path: String },
    CommitRename,
    CancelRename,
    NewGroup,
    AddCurrent(Option<i64>),
    AddFolder(Option<i64>),
    AddTabs(Option<i64>),
    DeleteGroup(i64),
    ToggleShowNames,
    ToggleRecent,
    RemoveRecent(String),
}

pub(super) fn render(
    app: &mut ImageViewerApp,
    ui: &mut egui::Ui,
    _can_drop: bool,
) -> Option<SidebarAction> {
    let root_expanded = app.favorite_root_expanded;
    let names_only = app.favorites_show_names_only;
    let recent_expanded = app.favorites_recent_expanded;
    let groups: Vec<FolderGroup> = app.folder_groups.clone();
    let recent: Vec<String> = app
        .recent_folders
        .iter()
        .take(RECENT_LIMIT)
        .cloned()
        .collect();
    let rename_target = app.favorites_rename.as_ref().map(|state| state.target.clone());
    let mut rename_text = app
        .favorites_rename
        .as_ref()
        .map(|state| state.text.clone())
        .unwrap_or_default();
    let mut action: Option<ListAction> = None;

    ui.spacing_mut().scroll.fade.strength = 0.0;
    egui::ScrollArea::both()
        .id_salt("sidebar_list_scroll")
        .auto_shrink([false, false])
        .min_scrolled_height(0.0)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .show(ui, |ui| {
            let viewport_width = ui.available_width();
            ui.set_min_width(viewport_width);
            ui.spacing_mut().item_spacing.y = 2.0;

            // ── 快速打开 root ────────────────────────────────────────
            let root_id = egui::Id::new("favorites_root_row");
            let (root, _) = row(
                ui,
                root_id,
                &t!("favorites.title"),
                0.0,
                Some(root_expanded),
                0,
                false,
                true,
            );
            if root.clicked() {
                action = Some(ListAction::ToggleRoot);
            }
            root.context_menu(|ui| {
                add_menu_items(ui, None, &mut action);
                ui.separator();
                let mut names_only = names_only;
                if ui
                    .checkbox(&mut names_only, t!("favorites.show_names_only"))
                    .changed()
                {
                    action = Some(ListAction::ToggleShowNames);
                }
            });

            if root_expanded {
                if groups.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(GROUP_INDENT + 6.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(t!("favorites.empty_hint").to_string()).weak(),
                            )
                            .wrap(),
                        );
                    });
                }
                for group in &groups {
                    let editing_group = rename_target
                        .as_ref()
                        .is_some_and(|target| target_is_group(target, group.id));
                    if editing_group {
                        let focus_key =
                            egui::Id::new(("favorites_rename_focus", group.id as u64));
                        let (commit, cancel) = inline_editor(
                            ui,
                            &mut rename_text,
                            GROUP_INDENT,
                            focus_key,
                        );
                        if commit {
                            action = Some(ListAction::CommitRename);
                        } else if cancel {
                            action = Some(ListAction::CancelRename);
                        }
                    } else {
                        let group_id = group.id;
                        let (group_row, _) = row(
                            ui,
                            egui::Id::new(("favorites_group_row", group_id as u64)),
                            &group.name,
                            GROUP_INDENT,
                            Some(group.expanded),
                            group.items.len(),
                            false,
                            true,
                        );
                        if group_row.double_clicked() {
                            action = Some(ListAction::OpenGroup(group_id));
                        } else if group_row.clicked() {
                            action = Some(ListAction::ToggleGroup(group_id));
                        }
                        group_row.context_menu(|ui| {
                            if ui.button(t!("favorites.open_group")).clicked() {
                                action = Some(ListAction::OpenGroup(group_id));
                                ui.close();
                            }
                            ui.separator();
                            add_menu_items(ui, Some(group_id), &mut action);
                            ui.separator();
                            if ui.button(t!("favorites.rename_group")).clicked() {
                                action = Some(ListAction::BeginRenameGroup(group_id));
                                ui.close();
                            }
                            if ui.button(t!("favorites.delete_group")).clicked() {
                                action = Some(ListAction::DeleteGroup(group_id));
                                ui.close();
                            }
                        });
                    }

                    if group.expanded {
                        for item in &group.items {
                            let editing_item = rename_target.as_ref().is_some_and(|target| {
                                target_is_item(target, group.id, &item.path)
                            });
                            if editing_item {
                                let focus_key = egui::Id::new((
                                    "favorites_rename_focus",
                                    group.id as u64,
                                    &item.path,
                                ));
                                let (commit, cancel) = inline_editor(
                                    ui,
                                    &mut rename_text,
                                    ITEM_INDENT,
                                    focus_key,
                                );
                                if commit {
                                    action = Some(ListAction::CommitRename);
                                } else if cancel {
                                    action = Some(ListAction::CancelRename);
                                }
                                continue;
                            }
                            let label = item.label(names_only);
                            let (item_row, remove) = row(
                                ui,
                                egui::Id::new((
                                    "favorites_item_row",
                                    group.id as u64,
                                    &item.path,
                                )),
                                &label,
                                ITEM_INDENT,
                                None,
                                0,
                                true,
                                false,
                            );
                            let tooltip = item.path.clone();
                            let item_row = item_row.on_hover_text(tooltip);
                            if item_row.clicked() {
                                action = Some(ListAction::OpenFolder {
                                    group_id: Some(group.id),
                                    path: item.path.clone(),
                                });
                            }
                            item_row.context_menu(|ui| {
                                if ui.button(t!("favorites.open")).clicked() {
                                    action = Some(ListAction::OpenFolder {
                                        group_id: Some(group.id),
                                        path: item.path.clone(),
                                    });
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button(t!("favorites.rename_item")).clicked() {
                                    action = Some(ListAction::BeginRenameItem {
                                        group_id: group.id,
                                        path: item.path.clone(),
                                    });
                                    ui.close();
                                }
                                if ui.button(t!("favorites.remove_item")).clicked() {
                                    action = Some(ListAction::RemoveItem {
                                        group_id: group.id,
                                        path: item.path.clone(),
                                    });
                                    ui.close();
                                }
                            });
                            if remove.is_some_and(|remove| remove.clicked()) {
                                action = Some(ListAction::RemoveItem {
                                    group_id: group.id,
                                    path: item.path.clone(),
                                });
                            }
                        }
                    }
                    ui.add_space(2.0);
                }
            }

            // ── 最近访问（与【快速打开】同级的根节点）──────────────
            ui.add_space(8.0);
            let (recent_row, _) = row(
                ui,
                egui::Id::new("favorites_recent_row"),
                &t!("favorites.recent"),
                0.0,
                Some(recent_expanded),
                recent.len(),
                false,
                true,
            );
            if recent_row.clicked() {
                action = Some(ListAction::ToggleRecent);
            }
            if recent_expanded {
                if recent.is_empty() {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(RECENT_ITEM_INDENT);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(t!("favorites.recent_empty").to_string())
                                    .weak(),
                            )
                            .wrap(),
                        );
                    });
                }
                for path in &recent {
                    let label = folder_label(path, names_only);
                    let (recent_item, remove) = row(
                        ui,
                        egui::Id::new(("favorites_recent_item", path.as_str())),
                        &label,
                        RECENT_ITEM_INDENT,
                        None,
                        0,
                        true,
                        false,
                    );
                    let recent_item = recent_item.on_hover_text(path.as_str());
                    if recent_item.clicked() {
                        action = Some(ListAction::OpenFolder {
                            group_id: None,
                            path: path.clone(),
                        });
                    }
                    if remove.is_some_and(|remove| remove.clicked()) {
                        action = Some(ListAction::RemoveRecent(path.clone()));
                    }
                }
            }
            ui.add_space(4.0);
        });

    // Write the editor text back before a commit reads it.
    if let Some(state) = app.favorites_rename.as_mut() {
        state.text = rename_text;
    }

    let mut open_group = None;
    match action {
        Some(ListAction::ToggleRoot) => app.favorites_toggle_root(),
        Some(ListAction::ToggleGroup(id)) => app.favorites_toggle_group(id),
        Some(ListAction::OpenGroup(id)) => open_group = Some(id),
        Some(ListAction::OpenFolder { group_id, path }) => {
            app.request_open_favorite_folder(group_id, path)
        }
        Some(ListAction::RemoveItem { group_id, path }) => {
            app.favorites_remove_item(group_id, &path)
        }
        Some(ListAction::BeginRenameGroup(id)) => app.favorites_begin_rename_group(id),
        Some(ListAction::BeginRenameItem { group_id, path }) => {
            app.favorites_begin_rename_item(group_id, &path)
        }
        Some(ListAction::CommitRename) => app.favorites_commit_rename(),
        Some(ListAction::CancelRename) => app.favorites_cancel_rename(),
        Some(ListAction::NewGroup) => {
            if let Some(id) = app.favorites_create_group() {
                app.favorites_begin_rename_group(id);
            }
        }
        Some(ListAction::AddCurrent(target)) => {
            let path = app.navigation_state.current_path.clone();
            if path.is_empty() || crate::domain::special_paths::is_virtual_path(&path) {
                app.notifications
                    .warning(t!("favorites.no_current_folder").to_string());
            } else {
                app.favorites_add_paths(target, vec![path]);
            }
        }
        Some(ListAction::AddFolder(target)) => {
            if let Some(folder) = rfd::FileDialog::new()
                .set_title(t!("favorites.add_folder").to_string())
                .pick_folder()
            {
                app.favorites_add_paths(
                    target,
                    vec![folder.to_string_lossy().into_owned()],
                );
            }
        }
        Some(ListAction::AddTabs(target)) => {
            let paths = app.workspace_tab_paths();
            if paths.is_empty() {
                app.notifications
                    .warning(t!("favorites.no_open_tabs").to_string());
            } else {
                app.favorites_add_paths(target, paths);
            }
        }
        Some(ListAction::DeleteGroup(id)) => app.favorites_remove_group(id),
        Some(ListAction::ToggleShowNames) => app.favorites_toggle_show_names(),
        Some(ListAction::ToggleRecent) => app.favorites_toggle_recent(),
        Some(ListAction::RemoveRecent(path)) => app.remove_recent_folder(&path),
        None => {}
    }

    if let Some(id) = open_group {
        return Some(SidebarAction::OpenFolderGroup(id));
    }
    None
}

fn target_is_group(target: &FavoritesRenameTarget, group_id: i64) -> bool {
    matches!(target, FavoritesRenameTarget::Group(id) if *id == group_id)
}

fn target_is_item(target: &FavoritesRenameTarget, group_id: i64, path: &str) -> bool {
    matches!(
        target,
        FavoritesRenameTarget::Item { group_id: target_group, path: target_path }
            if *target_group == group_id && target_path == path
    )
}

fn folder_label(path: &str, names_only: bool) -> String {
    if !names_only {
        return path.to_owned();
    }
    let trimmed = path.trim_end_matches(['\\', '/']);
    trimmed
        .rsplit(['\\', '/'])
        .find(|segment| !segment.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.to_owned())
}

/// The "add …" entries shared by the favorites root and group menus.
fn add_menu_items(ui: &mut egui::Ui, target: Option<i64>, action: &mut Option<ListAction>) {
    if ui.button(t!("favorites.new_group")).clicked() {
        *action = Some(ListAction::NewGroup);
        ui.close();
    }
    if ui.button(t!("favorites.add_folder")).clicked() {
        *action = Some(ListAction::AddFolder(target));
        ui.close();
    }
    if ui.button(t!("favorites.add_current")).clicked() {
        *action = Some(ListAction::AddCurrent(target));
        ui.close();
    }
    if ui.button(t!("favorites.add_tabs")).clicked() {
        *action = Some(ListAction::AddTabs(target));
        ui.close();
    }
}

/// One flat row. Returns the row response and, when `removable`, the hover ×.
fn row(
    ui: &mut egui::Ui,
    id_salt: egui::Id,
    label: &str,
    indent: f32,
    expandable: Option<bool>,
    count: usize,
    removable: bool,
    folder_icon: bool,
) -> (egui::Response, Option<egui::Response>) {
    let width = (ui.available_width() - SCROLLBAR_GAP).max(0.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::click());
    let mut close_response = None;
    // Pointer-in-rect instead of `response.hovered()`: the × hit area sits on top
    // of the row and would otherwise steal hover and make the glyph flicker.
    let row_hovered = ui.rect_contains_pointer(rect);

    if ui.is_rect_visible(rect) {
        let weak = ui.visuals().weak_text_color();
        if row_hovered {
            ui.painter().rect_filled(
                rect,
                4.0,
                crate::ui::theme::selection_hover_color(ui.ctx()),
            );
        }
        let font = egui::FontId::proportional(12.5);
        let mut cursor_x = rect.left() + indent + 4.0;
        if let Some(expanded) = expandable {
            ui.painter().text(
                egui::pos2(cursor_x + 5.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                if expanded { "▾" } else { "▸" },
                egui::FontId::proportional(10.0),
                weak,
            );
            cursor_x += 15.0;
        }
        if folder_icon {
            paint_category_folder(ui, egui::pos2(cursor_x, rect.center().y));
            cursor_x += FOLDER_ICON_WIDTH;
        }
        let count_galley = (count > 0).then(|| {
            ui.painter().layout_no_wrap(
                count.to_string(),
                egui::FontId::proportional(10.5),
                weak,
            )
        });
        let count_space = count_galley
            .as_ref()
            .map_or(0.0, |galley| galley.size().x + 8.0);
        // Reserve the × column even while it is hidden, so labels never jump
        // when the pointer enters the row.
        let close_space = if removable { CLOSE_WIDTH } else { 0.0 };
        let text_right = rect.right() - 6.0 - count_space - close_space;
        let text_width = (text_right - cursor_x).max(0.0);
        let visible =
            crate::ui::preview_panel::utils::truncate_text_to_fit(label, text_width, &font, ui);
        ui.painter().text(
            egui::pos2(cursor_x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &visible,
            font,
            ui.visuals().text_color(),
        );
        if let Some(galley) = count_galley {
            let galley_width = galley.size().x;
            ui.painter().galley(
                egui::pos2(
                    rect.right() - 6.0 - galley_width,
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                weak,
            );
        }
        if removable && row_hovered {
            let close_rect = egui::Rect::from_min_size(
                egui::pos2(rect.right() - 4.0 - CLOSE_WIDTH, rect.top()),
                egui::vec2(CLOSE_WIDTH, ROW_HEIGHT),
            );
            let hovered_close =
                ui.interact(close_rect, id_salt.with("close"), egui::Sense::click());
            let tint = if hovered_close.hovered() {
                CLOSE_HOVER_TINT
            } else {
                weak
            };
            ui.painter().text(
                close_rect.center(),
                egui::Align2::CENTER_CENTER,
                "×",
                egui::FontId::proportional(13.0),
                tint,
            );
            close_response = Some(hovered_close);
        }
    }
    (response, close_response)
}

/// Yellow folder drawn in front of category nodes, like MadEdit's favorites
/// tree: dark tab, light body, darker outline.
fn paint_category_folder(ui: &egui::Ui, position: egui::Pos2) {
    let painter = ui.painter();
    let tab = egui::Rect::from_min_size(
        egui::pos2(position.x, position.y - 5.0),
        egui::vec2(7.0, 4.0),
    );
    let body = egui::Rect::from_min_size(
        egui::pos2(position.x, position.y - 3.0),
        egui::vec2(15.0, 10.0),
    );
    painter.rect_filled(tab, 1.5, egui::Color32::from_rgb(224, 164, 20));
    painter.rect_filled(body, 2.0, egui::Color32::from_rgb(255, 213, 79));
    painter.rect_stroke(
        body,
        2.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(197, 144, 12)),
        egui::StrokeKind::Outside,
    );
}

/// Inline editor replacing one row. Returns `(commit, cancel)`.
fn inline_editor(
    ui: &mut egui::Ui,
    text: &mut String,
    indent: f32,
    focus_key: egui::Id,
) -> (bool, bool) {
    let mut commit = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        let response = ui.add(
            egui::TextEdit::singleline(text)
                .desired_width((ui.available_width() - 8.0).max(60.0))
                .id(focus_key.with("edit")),
        );
        let first_frame = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(focus_key).is_none());
        if first_frame {
            response.request_focus();
            ui.ctx()
                .data_mut(|data| data.insert_temp(focus_key, true));
        }
        commit =
            response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        cancel = ui.input(|input| input.key_pressed(egui::Key::Escape));
    });
    if commit || cancel {
        ui.ctx()
            .data_mut(|data| data.remove::<bool>(focus_key));
    }
    (commit, cancel)
}
