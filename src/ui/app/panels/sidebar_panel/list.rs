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
    /// Pointer grabbed a row for drag-reordering (`None` = release/cancel).
    SetDrag(Option<crate::app::favorites_state::FavoritesDrag>),
    /// Drop a group at the given insertion index of the current order.
    DropGroup { id: i64, insertion: usize },
    /// Drop a folder row at the given insertion index of its group's order.
    DropItem { group_id: i64, path: String, insertion: usize },
    /// Drop a folder row into another group (`None` insertion appends).
    MoveItemBetweenGroups {
        from_group_id: i64,
        path: String,
        to_group_id: i64,
        insertion: Option<usize>,
    },
    /// Drag a 最近访问 row into a 快速打开 group (`None` insertion appends);
    /// the entry is removed from the recent list once it lands.
    RecentToGroup {
        path: String,
        to_group_id: i64,
        insertion: Option<usize>,
    },
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
    // Drag-reorder in progress (spans frames), snapshot before the UI closure.
    let drag = app.favorites_drag.clone();
    let mut drop_action: Option<ListAction> = None;
    // The hovered target group already contains the dragged folder: the line
    // turns red with a × and releasing does nothing.
    let mut drop_invalid = false;
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
                false,
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
                for (group_index, group) in groups.iter().enumerate() {
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
                            true,
                        );
                        // Drag-reorder: grab on drag start, show the insertion
                        // line while hovering other rows, drop on release.
                        // Only the four-way cursor while a drag is actually
                        // active; plain hover keeps the normal arrow.
                        if drag.is_some() && ui.rect_contains_pointer(group_row.rect) {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        }
                        if let Some(drag_payload) = &drag {
                            match drag_payload {
                                // Reordering group rows among themselves.
                                crate::app::favorites_state::FavoritesDrag::Group(
                                    drag_id,
                                ) => {
                                    // Keep the grabbed row tinted so the drag is visible.
                                    if *drag_id == group_id
                                        && ui.is_rect_visible(group_row.rect)
                                    {
                                        ui.painter().rect_filled(
                                            group_row.rect,
                                            4.0,
                                            egui::Color32::from_rgba_unmultiplied(
                                                0, 120, 215, 60,
                                            ),
                                        );
                                    } else if *drag_id != group_id
                                        && row_hit(ui, group_row.rect)
                                    {
                                        let after = drop_insertion(
                                            ui,
                                            group_row.rect,
                                            GROUP_INDENT,
                                            false,
                                        );
                                        drop_action = Some(ListAction::DropGroup {
                                            id: *drag_id,
                                            insertion: group_index + usize::from(after),
                                        });
                                    }
                                }
                                // Dropping a folder row onto a group header
                                // appends it to that group's end; refused when
                                // the group already contains the folder.
                                crate::app::favorites_state::FavoritesDrag::Item {
                                    group_id: from_group,
                                    path,
                                } => {
                                    if ui.is_rect_visible(group_row.rect)
                                        && row_hit(ui, group_row.rect)
                                    {
                                        let target_has_it = groups
                                            .iter()
                                            .find(|group| group.id == group_id)
                                            .is_some_and(|group| group.contains_path(path));
                                        drop_invalid |= target_has_it;
                                        if target_has_it {
                                            dashed_header_refused(ui, group_row.rect);
                                        } else {
                                            ui.painter().rect_stroke(
                                                group_row.rect,
                                                4.0,
                                                egui::Stroke::new(
                                                    1.5,
                                                    egui::Color32::from_rgb(0, 120, 215),
                                                ),
                                                egui::StrokeKind::Outside,
                                            );
                                        }
                                        drop_action = Some(ListAction::MoveItemBetweenGroups {
                                            from_group_id: *from_group,
                                            path: path.clone(),
                                            to_group_id: group_id,
                                            insertion: None,
                                        });
                                    }
                                }
                                // Dropping a 最近访问 row onto a group header
                                // appends it to that group and clears the entry;
                                // refused when the group already has the folder.
                                crate::app::favorites_state::FavoritesDrag::Recent(path) => {
                                    if ui.is_rect_visible(group_row.rect)
                                        && row_hit(ui, group_row.rect)
                                    {
                                        let target_has_it = groups
                                            .iter()
                                            .find(|group| group.id == group_id)
                                            .is_some_and(|group| group.contains_path(path));
                                        drop_invalid |= target_has_it;
                                        if target_has_it {
                                            dashed_header_refused(ui, group_row.rect);
                                        } else {
                                            ui.painter().rect_stroke(
                                                group_row.rect,
                                                4.0,
                                                egui::Stroke::new(
                                                    1.5,
                                                    egui::Color32::from_rgb(0, 120, 215),
                                                ),
                                                egui::StrokeKind::Outside,
                                            );
                                        }
                                        drop_action = Some(ListAction::RecentToGroup {
                                            path: path.clone(),
                                            to_group_id: group_id,
                                            insertion: None,
                                        });
                                    }
                                }
                            }
                        }
                        if group_row.drag_started() {
                            action = Some(ListAction::SetDrag(Some(
                                crate::app::favorites_state::FavoritesDrag::Group(group_id),
                            )));
                        }
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
                        for (item_index, item) in group.items.iter().enumerate() {
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
                                true,
                            );
                            // Same drag-reorder protocol as group rows, but
                            // scoped to rows of the dragged item's own group.
                            // Four-way cursor only while actually dragging.
                            if drag.is_some() && ui.rect_contains_pointer(item_row.rect) {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                            }
                            if let Some(drag_payload) = &drag {
                            match drag_payload {
                                // The dragged row itself: tint it so the drag
                                // is visible.
                                crate::app::favorites_state::FavoritesDrag::Item {
                                    group_id: drag_group,
                                    path: drag_path,
                                } if *drag_group == group.id
                                    && drag_path.eq_ignore_ascii_case(&item.path)
                                    && ui.is_rect_visible(item_row.rect) =>
                                {
                                    ui.painter().rect_filled(
                                        item_row.rect,
                                        4.0,
                                        egui::Color32::from_rgba_unmultiplied(0, 120, 215, 60),
                                    );
                                }
                                // In-group reorder: insert at the hovered position.
                                crate::app::favorites_state::FavoritesDrag::Item {
                                    group_id: drag_group,
                                    path: drag_path,
                                } if *drag_group == group.id => {
                                    if row_hit(ui, item_row.rect) {
                                        let after =
                                            drop_insertion(ui, item_row.rect, ITEM_INDENT, false);
                                        drop_action = Some(ListAction::DropItem {
                                            group_id: group.id,
                                            path: drag_path.clone(),
                                            insertion: item_index + usize::from(after),
                                        });
                                    }
                                }
                                // Cross-group drop: insert at the hovered
                                // position of this group; refused when the
                                // group already contains the folder.
                                crate::app::favorites_state::FavoritesDrag::Item {
                                    group_id: drag_group,
                                    path: drag_path,
                                } => {
                                    if row_hit(ui, item_row.rect) {
                                        let refused = group.contains_path(drag_path);
                                        drop_invalid |= refused;
                                        let after = drop_insertion(
                                            ui,
                                            item_row.rect,
                                            ITEM_INDENT,
                                            refused,
                                        );
                                        drop_action = Some(ListAction::MoveItemBetweenGroups {
                                            from_group_id: *drag_group,
                                            path: drag_path.clone(),
                                            to_group_id: group.id,
                                            insertion: Some(item_index + usize::from(after)),
                                        });
                                    }
                                }
                                // A 最近访问 row dropped onto this group's
                                // folder rows inserts at the hovered position;
                                // refused when the group already has the folder.
                                crate::app::favorites_state::FavoritesDrag::Recent(
                                    drag_path,
                                ) => {
                                    if row_hit(ui, item_row.rect) {
                                        let refused = group.contains_path(drag_path);
                                        drop_invalid |= refused;
                                        let after = drop_insertion(
                                            ui,
                                            item_row.rect,
                                            ITEM_INDENT,
                                            refused,
                                        );
                                        drop_action = Some(ListAction::RecentToGroup {
                                            path: drag_path.clone(),
                                            to_group_id: group.id,
                                            insertion: Some(item_index + usize::from(after)),
                                        });
                                    }
                                }
                                _ => {}
                            }
                            }
                            if item_row.drag_started() {
                                action = Some(ListAction::SetDrag(Some(
                                    crate::app::favorites_state::FavoritesDrag::Item {
                                        group_id: group.id,
                                        path: item.path.clone(),
                                    },
                                )));
                            }
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
                false,
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
                        true,
                    );
                    // 最近访问 rows can be dragged into a 快速打开 group.
                    if drag.is_some() && ui.rect_contains_pointer(recent_item.rect) {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    }
                    if let Some(crate::app::favorites_state::FavoritesDrag::Recent(
                        drag_path,
                    )) = &drag
                    {
                        if drag_path.eq_ignore_ascii_case(path)
                            && ui.is_rect_visible(recent_item.rect)
                        {
                            ui.painter().rect_filled(
                                recent_item.rect,
                                4.0,
                                egui::Color32::from_rgba_unmultiplied(0, 120, 215, 60),
                            );
                        }
                    }
                    if recent_item.drag_started() {
                        action = Some(ListAction::SetDrag(Some(
                            crate::app::favorites_state::FavoritesDrag::Recent(path.clone()),
                        )));
                    }
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

    // Drop-at-release model: while the button is held the insertion line only
    // previews the landing spot; the reorder is applied at the moment the
    // button is released over a target row. Releasing anywhere else cancels.
    let released = ui.input(|input| input.pointer.primary_released());
    let primary_down = ui.input(|input| input.pointer.primary_down());
    match drop_action {
        Some(ListAction::DropGroup { id, insertion }) => {
            if released {
                app.favorites_drop_group(id, insertion);
            }
        }
        Some(ListAction::DropItem { group_id, path, insertion }) => {
            if released {
                app.favorites_drop_item(group_id, &path, insertion);
            }
        }
        Some(ListAction::MoveItemBetweenGroups {
            from_group_id,
            path,
            to_group_id,
            insertion,
        }) => {
            if released && !drop_invalid {
                app.favorites_move_item(from_group_id, &path, to_group_id, insertion);
            }
        }
        Some(ListAction::RecentToGroup {
            path,
            to_group_id,
            insertion,
        }) => {
            if released && !drop_invalid {
                app.favorites_recent_to_group(&path, to_group_id, insertion);
            }
        }
        _ => {}
    }
    if drag.is_some() && (released || !primary_down) {
        app.favorites_set_drag(None);
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
        Some(ListAction::SetDrag(drag)) => app.favorites_set_drag(drag),
        Some(ListAction::DropGroup { id, insertion }) => {
            // Handled above before the action match; kept for exhaustiveness.
            app.favorites_drop_group(id, insertion)
        }
        Some(ListAction::DropItem { group_id, path, insertion }) => {
            app.favorites_drop_item(group_id, &path, insertion)
        }
        Some(ListAction::MoveItemBetweenGroups {
            from_group_id,
            path,
            to_group_id,
            insertion,
        }) => app.favorites_move_item(from_group_id, &path, to_group_id, insertion),
        Some(ListAction::RecentToGroup {
            path,
            to_group_id,
            insertion,
        }) => app.favorites_recent_to_group(&path, to_group_id, insertion),
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

/// Paint the insertion line at the top or bottom edge of the row under the
/// dragged pointer and return whether the drop inserts *after* that row.
/// Row hover including the 2 px gap below, so a moving pointer never
/// slips between rows during a drag.
fn row_hit(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.input(|input| {
        input.pointer.latest_pos().is_some_and(|pointer| {
            pointer.x >= rect.left()
                && pointer.x <= rect.right()
                && pointer.y >= rect.top() - 1.0
                && pointer.y <= rect.bottom() + 2.0
        })
    })
}

/// Paint the insertion line at the top or bottom edge of the row under the
/// dragged pointer and return whether the drop inserts *after* that row.
/// `refused` renders the line red with a red × (duplicate target).
fn drop_insertion(ui: &egui::Ui, rect: egui::Rect, indent: f32, refused: bool) -> bool {
    let after = ui
        .input(|input| input.pointer.latest_pos())
        .is_some_and(|pointer| pointer.y > rect.center().y);
    let y = if after { rect.bottom() } else { rect.top() };
    let color = if refused {
        CLOSE_HOVER_TINT
    } else {
        // Same accent as the old sidebar's pinned-folder reorder line.
        egui::Color32::from_rgb(0, 120, 215)
    };
    let from = egui::pos2(rect.left() + indent, y);
    let to = egui::pos2(rect.right(), y);
    if refused {
        // Red dashed line: this group already has the folder.
        for shape in egui::epaint::Shape::dashed_line(
            &[from, to],
            egui::Stroke::new(2.0, color),
            4.0,
            3.0,
        ) {
            ui.painter().add(shape);
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
    } else {
        ui.painter()
            .line_segment([from, to], egui::Stroke::new(2.0, color));
    }
    after
}

/// Red dashed rectangle outline for a refused drop on a group header.
fn dashed_header_refused(ui: &egui::Ui, rect: egui::Rect) {
    let stroke = egui::Stroke::new(1.5, CLOSE_HOVER_TINT);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    for edge in corners.windows(2) {
        for shape in egui::epaint::Shape::dashed_line(edge, stroke, 4.0, 3.0) {
            ui.painter().add(shape);
        }
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
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
/// Draggable rows (favorite groups and their folders) also sense drags so they
/// can be reordered; egui keeps plain clicks working through the drag threshold.
fn row(
    ui: &mut egui::Ui,
    id_salt: egui::Id,
    label: &str,
    indent: f32,
    expandable: Option<bool>,
    count: usize,
    removable: bool,
    folder_icon: bool,
    draggable: bool,
) -> (egui::Response, Option<egui::Response>) {
    let width = (ui.available_width() - SCROLLBAR_GAP).max(0.0);
    let sense = if draggable {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::click()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), sense);
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

#[cfg(test)]
mod drag_probe {
    //! Synthetic pointer probe: a `Sense::click_and_drag()` row inside a
    //! `ScrollArea::both()` must report `drag_started()` frame by frame, the
    //! same way the favorites rows sense drags.
    use eframe::egui;

    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> (bool, bool) {
        let mut observed = (false, false);
        ctx.run_ui(
            egui::RawInput { events, ..Default::default() },
            |ui| {
                egui::ScrollArea::both()
                    .id_salt("probe_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let (_, response) = ui.allocate_exact_size(
                            egui::vec2(150.0, 24.0),
                            egui::Sense::click_and_drag(),
                        );
                        observed = (response.drag_started(), response.dragged());
                    });
            },
        );
        observed
    }

    #[test]
    fn click_and_drag_row_inside_scroll_area_reports_drag() {
        let ctx = egui::Context::default();
        let mut drag_started = false;
        let mut dragged = false;

        let _ = frame(&ctx, Vec::new()); // layout
        let (started, dragged_frame) = frame(
            &ctx,
            vec![egui::Event::PointerButton {
                pos: egui::pos2(60.0, 12.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        drag_started |= started;
        dragged |= dragged_frame;
        for step in 1..=4 {
            let (started, dragged_frame) = frame(
                &ctx,
                vec![egui::Event::PointerMoved(egui::pos2(60.0, 12.0 + 6.0 * step as f32))],
            );
            drag_started |= started;
            dragged |= dragged_frame;
        }

        assert!(drag_started || dragged, "row must report the drag");
    }
}
