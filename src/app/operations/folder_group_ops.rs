//! Saved tab groups: capture the open tabs as a named Quick Access group and
//! restore the whole group with one command (like browser bookmark groups).
use crate::app::dual_panel::ActivePanel;
use crate::app::favorites_state::{
    FavoritesDrag, FavoritesRename, FavoritesRenameTarget, PendingFavoriteOpen,
};
use crate::app::state::sidebar_tree_state::reveal::same_path;
use crate::app::ImageViewerApp;
use eframe::egui;
use std::path::Path;
use std::time::{Duration, Instant};

impl ImageViewerApp {
    /// Paths of the workspace tabs in tab order; the active tab contributes
    /// its live navigation path, which may differ from the stored tab path.
    pub(crate) fn workspace_tab_paths(&self) -> Vec<String> {
        self.tab_manager
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                if index == self.tab_manager.active_tab {
                    self.navigation_state.current_path.clone()
                } else {
                    tab.path.clone()
                }
            })
            .collect()
    }

    /// Opens every saved path of the group as its own tab, replacing the
    /// previous batch: all old *unpinned* tabs are closed, the group folders
    /// become the new tabs, and the first of them takes focus. Pinned tabs are
    /// never touched.
    ///
    /// The app never quits as a side effect: closing stops at one remaining
    /// tab while making room, and the final old tab is only closed after the
    /// group's tabs already exist.
    pub(crate) fn open_folder_group(&mut self, group_id: i64) {
        let Some(group) = self.folder_groups.iter().find(|group| group.id == group_id) else {
            return;
        };
        let paths: Vec<String> = group.items.iter().map(|item| item.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        let max_tabs = crate::tabs::MAX_TABS;
        if paths.len() > max_tabs {
            self.notifications.warning(
                rust_i18n::t!(
                    "favorites.group_too_large",
                    count = paths.len(),
                    max = max_tabs
                )
                .to_string(),
            );
            return;
        }
        let previous_ids: Vec<usize> = self.tab_manager.tabs.iter().map(|tab| tab.id).collect();
        let pinned_after_replace = self
            .tab_manager
            .tabs
            .iter()
            .filter(|tab| tab.pinned)
            .count();
        // After the replace, pinned tabs plus the group must fit the limit
        // (the replace keeps exactly one old tab alive until the group is in).
        if pinned_after_replace + paths.len() >= max_tabs {
            self.notifications
                .warning(rust_i18n::t!("favorites.group_no_room").to_string());
            return;
        }

        // Phase 1: close the old unpinned batch, stopping at one surviving tab
        // so the tab list never empties (an empty list is not a valid state and
        // closing the literal last tab would quit the window).
        let mut closable: Vec<usize> = (0..self.tab_manager.tabs.len())
            .filter(|&index| {
                previous_ids.contains(&self.tab_manager.tabs[index].id)
                    && !self.tab_manager.tabs[index].pinned
            })
            .collect();
        closable.sort_unstable_by_key(|&index| std::cmp::Reverse(index));
        for index in closable {
            if self.tab_manager.tabs.len() <= 1 {
                break;
            }
            self.close_main_tab_keep_app_open(index);
        }

        // Phase 2: open the group.
        self.open_paths_in_main_tabs(&paths);
        let new_indices: Vec<usize> = (0..self.tab_manager.tabs.len())
            .filter(|&index| {
                !previous_ids.contains(&self.tab_manager.tabs[index].id)
            })
            .collect();
        if new_indices.is_empty() {
            // Opening was blocked (settings window, search overlay, …). The
            // surviving old tab(s) still hold the session, so just report.
            self.notifications
                .warning(rust_i18n::t!("favorites.group_open_failed").to_string());
            return;
        }
        if let Some(&first_new) = new_indices.first() {
            self.sync_to_tab();
            self.tab_manager.switch_to(first_new);
            self.sync_from_tab();
            self.update_video_visibility();
        }

        // Phase 3: close whatever old unpinned tabs remain (at most the one
        // survivor from phase 1). New tabs already exist, so this can never be
        // the last tab and can never trigger the window-close path.
        for index in (0..self.tab_manager.tabs.len()).rev() {
            let tab = &self.tab_manager.tabs[index];
            if !previous_ids.contains(&tab.id) {
                continue;
            }
            if tab.pinned {
                continue;
            }
            self.close_main_tab(index);
        }
    }

    /// `close_main_tab` without the "last tab closed → close the window" side
    /// effect. `TabManager::close_tab` returns `true` only when one tab would
    /// remain and it did NOT close it, so this helper simply keeps that tab.
    fn close_main_tab_keep_app_open(&mut self, index: usize) {
        let Some(tab) = self.tab_manager.tabs.get(index) else {
            return;
        };
        if tab.pinned {
            return;
        }
        if self.media_preview_owner_tab_id == Some(tab.id) {
            self.destroy_media_preview();
        }
        self.sync_to_tab();
        let closing_active = index == self.tab_manager.active_tab;
        let closed_last = self.tab_manager.close_tab(index);
        if closed_last {
            return;
        }
        if closing_active {
            self.sync_from_tab();
        }
        self.update_video_visibility();
        self.save_preferences();
    }
}

/// 【列表】 favorites: groups of folders, inline renames and recent folders.
impl ImageViewerApp {
    /// Create an empty group with a unique default name and return its id.
    pub(crate) fn favorites_create_group(&mut self) -> Option<i64> {
        let base = rust_i18n::t!("favorites.default_group").to_string();
        let mut name = base.clone();
        let mut counter = 2;
        while self
            .folder_groups
            .iter()
            .any(|group| group.name.eq_ignore_ascii_case(&name))
        {
            name = format!("{base} {counter}");
            counter += 1;
        }
        let id = self.app_state_db.create_folder_group(&name)?;
        self.folder_groups = self.app_state_db.get_all_folder_groups();
        self.favorite_root_expanded = true;
        self.save_preferences();
        Some(id)
    }

    /// Target group for "add" commands: an explicit group, else the default
    /// 「常用」 group (created on demand like MadEdit's favorite target).
    fn favorites_target_group_id(&mut self, group_id: Option<i64>) -> Option<i64> {
        if let Some(id) = group_id.filter(|id| self.folder_groups.iter().any(|g| g.id == *id)) {
            return Some(id);
        }
        let name = rust_i18n::t!("favorites.default_group").to_string();
        if let Some(group) = self
            .folder_groups
            .iter()
            .find(|group| group.name.eq_ignore_ascii_case(&name))
        {
            return Some(group.id);
        }
        let id = self.app_state_db.create_folder_group(&name)?;
        self.folder_groups = self.app_state_db.get_all_folder_groups();
        Some(id)
    }

    /// Add folders to a group, skipping duplicates. Returns how many were added.
    pub(crate) fn favorites_add_paths(&mut self, group_id: Option<i64>, paths: Vec<String>) -> usize {
        let Some(id) = self.favorites_target_group_id(group_id) else {
            return 0;
        };
        let added = self.app_state_db.add_folder_group_items(id, &paths);
        self.app_state_db.set_folder_group_expanded(id, true);
        self.folder_groups = self.app_state_db.get_all_folder_groups();
        self.favorite_root_expanded = true;
        self.save_preferences();
        added
    }

    pub(crate) fn favorites_toggle_group(&mut self, group_id: i64) {
        if let Some(group) = self.folder_groups.iter_mut().find(|group| group.id == group_id) {
            group.expanded = !group.expanded;
            let expanded = group.expanded;
            self.app_state_db
                .set_folder_group_expanded(group_id, expanded);
        }
    }

    pub(crate) fn favorites_remove_group(&mut self, group_id: i64) {
        self.app_state_db.delete_folder_group(group_id);
        self.folder_groups = self.app_state_db.get_all_folder_groups();
        self.save_preferences();
    }

    pub(crate) fn favorites_remove_item(&mut self, group_id: i64, path: &str) {
        self.app_state_db.remove_folder_group_item(group_id, path);
        self.folder_groups = self.app_state_db.get_all_folder_groups();
    }

    pub(crate) fn favorites_begin_rename_group(&mut self, group_id: i64) {
        if let Some(group) = self.folder_groups.iter().find(|group| group.id == group_id) {
            self.favorites_rename = Some(FavoritesRename::new(
                FavoritesRenameTarget::Group(group_id),
                group.name.clone(),
            ));
        }
    }

    pub(crate) fn favorites_begin_rename_item(&mut self, group_id: i64, path: &str) {
        let current = self
            .folder_groups
            .iter()
            .find(|group| group.id == group_id)
            .and_then(|group| group.items.iter().find(|item| item.path == path))
            .and_then(|item| item.name.clone())
            .unwrap_or_default();
        self.favorites_rename = Some(FavoritesRename::new(
            FavoritesRenameTarget::Item {
                group_id,
                path: path.to_owned(),
            },
            current,
        ));
    }

    /// Drop one folder row into a possibly different group. `None` insertion
    /// appends to the destination group's end; a same-group drop with an index
    /// falls back to the in-group reorder path.
    pub(crate) fn favorites_move_item(
        &mut self,
        from_group: i64,
        path: &str,
        to_group: i64,
        insertion: Option<usize>,
    ) {
        if from_group == to_group {
            if let Some(index) = insertion {
                self.favorites_drop_item(from_group, path, index);
            }
            return;
        }
        if self
            .app_state_db
            .move_folder_group_item(from_group, path, to_group, insertion)
        {
            self.folder_groups = self.app_state_db.get_all_folder_groups();
        }
    }

    /// Apply the inline rename. A blank group name keeps the old name; a blank
    /// item name restores the default folder label.
    pub(crate) fn favorites_commit_rename(&mut self) {
        let Some(rename) = self.favorites_rename.take() else {
            return;
        };
        let name = rename.text.trim().to_owned();
        match &rename.target {
            FavoritesRenameTarget::Group(group_id) => {
                if name.is_empty() {
                    return;
                }
                if !self.app_state_db.rename_folder_group(*group_id, &name) {
                    self.notifications
                        .warning(rust_i18n::t!("sidebar.group_name_exists").to_string());
                }
            }
            FavoritesRenameTarget::Item { group_id, path } => {
                let name = (!name.is_empty()).then_some(name.as_str());
                self.app_state_db
                    .set_folder_group_item_name(*group_id, path, name);
            }
        }
        self.folder_groups = self.app_state_db.get_all_folder_groups();
        self.save_preferences();
    }

    pub(crate) fn favorites_cancel_rename(&mut self) {
        self.favorites_rename = None;
    }

    /// Remember which 【列表】 row the pointer started dragging.
    pub(crate) fn favorites_set_drag(&mut self, drag: Option<FavoritesDrag>) {
        self.favorites_drag = drag;
    }

    /// Drop a group row at the given insertion index of the *current* display
    /// order (inserting before the row at that index). Persists the new order;
    /// one-click group loading follows it.
    pub(crate) fn favorites_drop_group(&mut self, from_id: i64, insertion: usize) {
        let Some(from_index) = self.folder_groups.iter().position(|group| group.id == from_id)
        else {
            self.favorites_drag = None;
            return;
        };
        let mut order: Vec<i64> = self.folder_groups.iter().map(|group| group.id).collect();
        let target = insertion.min(order.len());
        if target != from_index && target != from_index + 1 {
            let moved = order.remove(from_index);
            let to = if from_index < target { target - 1 } else { target };
            order.insert(to, moved);
            self.app_state_db.reorder_folder_groups(&order);
            self.folder_groups = self.app_state_db.get_all_folder_groups();
            // Keep the drag alive so the row keeps following the pointer for
            // further moves; only releasing the button ends the drag.
        }
        // A no-op drop (dropped next to the original spot) keeps the drag
        // alive so the row can continue past adjacent dead zones.
    }

    /// Drop one folder row at the given insertion index of its group's current
    /// display order. The saved order is what one-click group loading uses.
    pub(crate) fn favorites_drop_item(&mut self, group_id: i64, from_path: &str, insertion: usize) {
        let Some(group) = self.folder_groups.iter().find(|group| group.id == group_id) else {
            self.favorites_drag = None;
            return;
        };
        let Some(from_index) = group
            .items
            .iter()
            .position(|item| item.path.eq_ignore_ascii_case(from_path))
        else {
            self.favorites_drag = None;
            return;
        };
        let mut order: Vec<String> = group.items.iter().map(|item| item.path.clone()).collect();
        let target = insertion.min(order.len());
        if target != from_index && target != from_index + 1 {
            let moved = order.remove(from_index);
            let to = if from_index < target { target - 1 } else { target };
            order.insert(to, moved);
            self.app_state_db.reorder_folder_group_items(group_id, &order);
            self.folder_groups = self.app_state_db.get_all_folder_groups();
            // Keep the drag alive so the row keeps following the pointer for
            // further moves; only releasing the button ends the drag.
        }
        // A no-op drop (dropped next to the original spot) keeps the drag
        // alive so the row can continue past adjacent dead zones.
    }

    pub(crate) fn favorites_toggle_root(&mut self) {
        self.favorite_root_expanded = !self.favorite_root_expanded;
        self.save_preferences();
    }

    pub(crate) fn favorites_toggle_show_names(&mut self) {
        self.favorites_show_names_only = !self.favorites_show_names_only;
        self.save_preferences();
    }

    pub(crate) fn favorites_toggle_recent(&mut self) {
        self.favorites_recent_expanded = !self.favorites_recent_expanded;
        self.save_preferences();
    }

    /// Record the current folder in the persisted recent list.
    pub(crate) fn record_recent_folder(&mut self) {
        let path = self.navigation_state.current_path.clone();
        if path.is_empty() || crate::domain::special_paths::is_virtual_path(&path) {
            return;
        }
        if self
            .recent_folders
            .first()
            .is_some_and(|first| first.eq_ignore_ascii_case(&path))
        {
            return;
        }
        self.recent_folders
            .retain(|existing| !existing.eq_ignore_ascii_case(&path));
        self.recent_folders.insert(0, path);
        self.recent_folders.truncate(RECENT_FOLDERS_LIMIT);
        self.save_preferences();
    }

    pub(crate) fn remove_recent_folder(&mut self, path: &str) {
        let before = self.recent_folders.len();
        self.recent_folders
            .retain(|existing| !existing.eq_ignore_ascii_case(path));
        if self.recent_folders.len() != before {
            self.save_preferences();
        }
    }

    /// Drag a 最近访问 row into a 快速打开 group: the folder is added to the
    /// group (custom position, `None` appends) and removed from the recent
    /// list. Adding a folder that is already in the group keeps it there —
    /// the row still leaves the recent list, which is the user's intent.
    pub(crate) fn favorites_recent_to_group(
        &mut self,
        path: &str,
        to_group: i64,
        insertion: Option<usize>,
    ) {
        let added = self.favorites_add_paths(Some(to_group), vec![path.to_owned()]);
        if added == 0 {
            let already_in_group = self
                .folder_groups
                .iter()
                .find(|group| group.id == to_group)
                .is_some_and(|group| group.contains_path(path));
            if !already_in_group {
                self.folder_groups = self.app_state_db.get_all_folder_groups();
                self.remove_recent_folder(path);
                return;
            }
        }
        if let Some(index) = insertion {
            // Position the freshly appended row at the requested index.
            if let Some(group) = self.folder_groups.iter().find(|group| group.id == to_group) {
                let mut order: Vec<String> =
                    group.items.iter().map(|item| item.path.clone()).collect();
                if let Some(last) = order.pop() {
                    let index = index.min(order.len());
                    order.insert(index, last);
                    self.app_state_db.reorder_folder_group_items(to_group, &order);
                }
            }
        }
        self.folder_groups = self.app_state_db.get_all_folder_groups();
        self.remove_recent_folder(path);
    }
}

const RECENT_FOLDERS_LIMIT: usize = 30;
/// How long the existence probe may take before the click is dropped.
const FAVORITE_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Opening a 收藏/最近 folder from the 【列表】 page.
impl ImageViewerApp {
    /// A favorite click never navigates the current tab: it activates the tab
    /// already showing that folder, or opens a new one. Existence is checked on
    /// a worker thread first so a dead network/share path cannot freeze the UI.
    pub(crate) fn request_open_favorite_folder(&mut self, group_id: Option<i64>, path: String) {
        if self.focus_tab_for_path(&path) {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let probe = path.clone();
        let spawned = std::thread::Builder::new()
            .name("favorites-existence-probe".into())
            .spawn(move || {
                let exists = Path::new(&probe).is_dir();
                let _ = sender.send(exists);
            });
        if spawned.is_err() {
            // Without a probe thread, fall back to opening directly.
            self.open_favorite_folder_now(&path);
            return;
        }
        self.pending_favorite_open = Some(PendingFavoriteOpen {
            group_id,
            path,
            started: Instant::now(),
            receiver,
        });
        self.ui_ctx.request_repaint();
    }

    /// Applies the result of the existence probe.
    pub(crate) fn poll_pending_favorite_open(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_favorite_open.as_ref() else {
            return;
        };
        let result = pending.receiver.try_recv();
        let elapsed = pending.started.elapsed();
        match result {
            Ok(true) => {
                let Some(pending) = self.pending_favorite_open.take() else {
                    return;
                };
                self.open_favorite_folder_now(&pending.path);
            }
            Ok(false) => {
                let Some(pending) = self.pending_favorite_open.take() else {
                    return;
                };
                self.report_missing_favorite(pending);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if elapsed > FAVORITE_PROBE_TIMEOUT {
                    self.pending_favorite_open = None;
                } else {
                    ctx.request_repaint_after(Duration::from_millis(50));
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_favorite_open = None;
            }
        }
    }

    fn report_missing_favorite(&mut self, pending: PendingFavoriteOpen) {
        self.notifications
            .warning(rust_i18n::t!("favorites.missing_removed").to_string());
        match pending.group_id {
            Some(group_id) => self.favorites_remove_item(group_id, &pending.path),
            None => self.remove_recent_folder(&pending.path),
        }
    }

    /// Opens the folder in a new tab and always activates it, regardless of the
    /// "activate new tabs" preference.
    fn open_favorite_folder_now(&mut self, path: &str) {
        if self.focus_tab_for_path(path) {
            return;
        }
        if self.tab_navigation_blocked() {
            return;
        }
        let panel = self.dual_panel_active;
        if !self.open_locations_in_new_tabs(panel, &[path.to_owned()], false) {
            return;
        }
        self.sync_to_tab();
        let target = Path::new(path);
        if self.dual_panel_enabled {
            let index = self
                .tab_manager
                .active()
                .dual_panel_tabs
                .as_ref()
                .and_then(|sets| {
                    sets.panel(panel)
                        .tabs
                        .iter()
                        .position(|tab| same_path(Path::new(&tab.snapshot.path), target))
                });
            if let Some(index) = index {
                self.dual_panel_select_tab(panel, index);
            }
        } else {
            let index = self
                .tab_manager
                .tabs
                .iter()
                .position(|tab| same_path(Path::new(&tab.path), target));
            if let Some(index) = index {
                self.tab_manager.switch_to(index);
                self.sync_from_tab();
                self.update_video_visibility();
            }
        }
        self.save_preferences();
    }

    /// Activates the tab (workspace or pane tab of the active workspace) that
    /// already shows `path`. Returns false when none does.
    fn focus_tab_for_path(&mut self, path: &str) -> bool {
        let target = Path::new(path);
        let active = self.tab_manager.active_tab;
        let mut found = None;
        for (index, tab) in self.tab_manager.tabs.iter().enumerate() {
            let tab_path = if index == active {
                &self.navigation_state.current_path
            } else {
                &tab.path
            };
            if same_path(Path::new(tab_path), target) {
                found = Some(index);
                break;
            }
        }
        if let Some(index) = found {
            if index != active {
                self.sync_to_tab();
                self.tab_manager.switch_to(index);
                self.sync_from_tab();
                self.update_video_visibility();
                self.save_preferences();
            }
            return true;
        }
        if self.dual_panel_enabled {
            let mut pane_hit = None;
            if let Some(sets) = self.tab_manager.active().dual_panel_tabs.as_ref() {
                for panel in [ActivePanel::Left, ActivePanel::Right] {
                    let set = sets.panel(panel);
                    if let Some(index) = set
                        .tabs
                        .iter()
                        .position(|tab| same_path(Path::new(&tab.snapshot.path), target))
                    {
                        pane_hit = Some((panel, index));
                        break;
                    }
                }
            }
            if let Some((panel, index)) = pane_hit {
                self.sync_to_tab();
                self.dual_panel_select_tab(panel, index);
                self.save_preferences();
                return true;
            }
        }
        false
    }
}
