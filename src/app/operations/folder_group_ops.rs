//! Saved tab groups: capture the open tabs as a named Quick Access group and
//! restore the whole group with one command (like browser bookmark groups).
use crate::app::ImageViewerApp;

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

    pub(crate) fn begin_save_tabs_as_group(&mut self) {
        if self.show_save_group_dialog {
            return;
        }
        let default_name = self
            .navigation_state
            .current_path
            .rsplit(['\\', '/'])
            .find(|segment| !segment.is_empty())
            .unwrap_or("")
            .to_string();
        let name = if default_name.is_empty() {
            rust_i18n::t!("tabs.group_default_name").to_string()
        } else {
            default_name
        };
        self.save_group_name = name;
        self.show_save_group_dialog = true;
    }

    pub(crate) fn cancel_save_tabs_as_group(&mut self) {
        self.show_save_group_dialog = false;
        self.save_group_name.clear();
    }

    pub(crate) fn confirm_save_tabs_as_group(&mut self) {
        let name = self.save_group_name.trim().to_string();
        if name.is_empty() {
            self.notifications
                .warning(rust_i18n::t!("tabs.group_name_required").to_string());
            return;
        }
        let exists = self
            .folder_groups
            .iter()
            .any(|group| group.name.eq_ignore_ascii_case(&name));
        if exists {
            self.notifications
                .warning(rust_i18n::t!("sidebar.group_name_exists").to_string());
            return;
        }
        let paths = self.workspace_tab_paths();
        match self
            .app_state_db
            .save_folder_group(&name, &paths)
        {
            Some(_) => {
                self.folder_groups = self.app_state_db.get_all_folder_groups();
                self.cancel_save_tabs_as_group();
            }
            None => {
                self.notifications
                    .warning(rust_i18n::t!("sidebar.group_name_exists").to_string());
            }
        }
    }

    /// Opens every saved path of the group as its own tab, then closes the
    /// previous batch so switching groups is a clean replacement. Pinned tabs
    /// are never closed; the first tab of the new group takes focus.
    pub(crate) fn open_folder_group(&mut self, group_id: i64) {
        let Some(group) = self.folder_groups.iter().find(|group| group.id == group_id) else {
            return;
        };
        let paths = group.paths.clone();
        if paths.is_empty() {
            return;
        }
        let previous_ids: Vec<usize> = self.tab_manager.tabs.iter().map(|tab| tab.id).collect();
        self.open_paths_in_main_tabs(&paths);

        // Focus the first tab of the new group before closing anything, so
        // the removals below never run against the active tab.
        let new_indices: Vec<usize> = (0..self.tab_manager.tabs.len())
            .filter(|&index| {
                !previous_ids.contains(&self.tab_manager.tabs[index].id)
            })
            .collect();
        if let Some(&first_new) = new_indices.first() {
            self.sync_to_tab();
            self.tab_manager.switch_to(first_new);
            self.sync_from_tab();
            self.update_video_visibility();
        }

        // Close the previous batch back-to-front so indices stay stable.
        // close_tab refuses pinned tabs as an extra guard.
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

    pub(crate) fn delete_folder_group(&mut self, group_id: i64) {
        self.app_state_db.delete_folder_group(group_id);
        self.folder_groups = self.app_state_db.get_all_folder_groups();
    }
}
