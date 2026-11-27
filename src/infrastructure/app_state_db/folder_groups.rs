use super::AppStateDb;
use crate::domain::folder_group::{FolderGroup, FolderGroupItem};
use rusqlite::params;

impl AppStateDb {
    /// Load every saved group with its ordered folders. [READER]
    pub fn get_all_folder_groups(&self) -> Vec<FolderGroup> {
        let Ok(db) = self.reader.lock() else {
            log::error!("[GROUPS] Failed to acquire reader lock");
            return Vec::new();
        };
        let Ok(mut stmt) = db.prepare(
            "SELECT id, name, position, expanded FROM folder_groups ORDER BY position ASC, id ASC",
        ) else {
            log::error!("[GROUPS] Failed to prepare group SELECT");
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |row| {
            Ok(FolderGroup {
                id: row.get(0)?,
                name: row.get(1)?,
                position: row.get(2)?,
                expanded: row.get::<_, i64>(3)? != 0,
                items: Vec::new(),
            })
        }) else {
            return Vec::new();
        };
        let mut groups: Vec<FolderGroup> = rows.flatten().collect();
        let Ok(mut stmt) = db.prepare(
            "SELECT group_id, path, name FROM folder_group_paths ORDER BY group_id, position ASC",
        ) else {
            return groups;
        };
        let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                FolderGroupItem {
                    path: row.get(1)?,
                    name: row.get(2)?,
                },
            ))
        }) else {
            return groups;
        };
        for (group_id, item) in rows.flatten() {
            if let Some(group) = groups.iter_mut().find(|group| group.id == group_id) {
                group.items.push(item);
            }
        }
        groups
    }

    /// Create an empty group at the end of the list. Returns the new row id, or
    /// `None` when the name already exists (case-insensitive). [WRITER]
    pub fn create_folder_group(&self, name: &str) -> Option<i64> {
        let db = self.writer.lock().ok()?;
        let inserted = db
            .execute(
                "INSERT INTO folder_groups (name, position)
                 SELECT ?, COALESCE(MAX(position), -1) + 1 FROM folder_groups",
                params![name],
            )
            .ok()?;
        if inserted == 0 {
            return None;
        }
        Some(db.last_insert_rowid())
    }

    /// Rename a group. Returns false when the new name is already taken. [WRITER]
    pub fn rename_folder_group(&self, id: i64, name: &str) -> bool {
        let Ok(db) = self.writer.lock() else {
            return false;
        };
        db.execute(
            "UPDATE folder_groups SET name = ? WHERE id = ?",
            params![name, id],
        )
        .map(|changed| changed > 0)
        .unwrap_or(false)
    }

    /// Persist the expand/collapse state of a group. [WRITER]
    pub fn set_folder_group_expanded(&self, id: i64, expanded: bool) {
        if let Ok(db) = self.writer.lock() {
            let _ = db.execute(
                "UPDATE folder_groups SET expanded = ? WHERE id = ?",
                params![i64::from(expanded), id],
            );
        }
    }

    /// Append folders to a group, skipping duplicates. Returns how many were
    /// inserted. [WRITER]
    pub fn add_folder_group_items(&self, id: i64, paths: &[String]) -> usize {
        let Ok(db) = self.writer.lock() else {
            return 0;
        };
        let mut added = 0usize;
        for path in paths {
            let inserted = db
                .execute(
                    "INSERT OR IGNORE INTO folder_group_paths (group_id, path, position)
                     VALUES (
                         ?,
                         ?,
                         COALESCE((SELECT MAX(position) FROM folder_group_paths WHERE group_id = ?), -1) + 1
                     )",
                    params![id, path, id],
                )
                .unwrap_or(0);
            added += inserted;
        }
        added
    }

    /// Remove one folder from a group. [WRITER]
    pub fn remove_folder_group_item(&self, id: i64, path: &str) {
        if let Ok(db) = self.writer.lock() {
            let _ = db.execute(
                "DELETE FROM folder_group_paths WHERE group_id = ? AND path = ?",
                params![id, path],
            );
        }
    }

    /// Set (or clear, with `None`) the custom display name of one folder. [WRITER]
    pub fn set_folder_group_item_name(&self, id: i64, path: &str, name: Option<&str>) {
        if let Ok(db) = self.writer.lock() {
            let _ = db.execute(
                "UPDATE folder_group_paths SET name = ? WHERE group_id = ? AND path = ?",
                params![name, id, path],
            );
        }
    }

    /// Create a group from paths (legacy "save open tabs as group"). [WRITER]
    pub fn save_folder_group(&self, name: &str, paths: &[String]) -> Option<i64> {
        let id = self.create_folder_group(name)?;
        let inserted = self.add_folder_group_items(id, paths);
        log::info!(
            "[GROUPS] Saved group {:?} ({} paths, {} inserted)",
            name,
            paths.len(),
            inserted
        );
        Some(id)
    }

    /// Replace a group's folders (keeps name and position). Display names of
    /// paths that stay in the group are preserved. [WRITER]
    pub fn update_folder_group_paths(&self, id: i64, paths: &[String]) {
        let existing: Vec<(String, Option<String>)> = {
            let Ok(db) = self.reader.lock() else {
                return;
            };
            db.prepare("SELECT path, name FROM folder_group_paths WHERE group_id = ?")
                .and_then(|mut stmt| {
                    let rows = stmt.query_map(params![id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                    })?;
                    Ok(rows.flatten().collect())
                })
                .unwrap_or_default()
        };
        let Ok(db) = self.writer.lock() else {
            return;
        };
        let _ = db.execute("DELETE FROM folder_group_paths WHERE group_id = ?", params![id]);
        for (position, path) in paths.iter().enumerate() {
            let name = existing
                .iter()
                .find(|(existing_path, _)| existing_path.eq_ignore_ascii_case(path))
                .and_then(|(_, name)| name.clone());
            let _ = db.execute(
                "INSERT OR IGNORE INTO folder_group_paths (group_id, path, position, name)
                 VALUES (?, ?, ?, ?)",
                params![id, path, position as i64, name],
            );
        }
    }

    /// Persist a new display order of groups (the list must contain every
    /// existing id exactly once). [WRITER]
    pub fn reorder_folder_groups(&self, ordered_ids: &[i64]) {
        let Ok(db) = self.writer.lock() else {
            return;
        };
        for (position, id) in ordered_ids.iter().enumerate() {
            let _ = db.execute(
                "UPDATE folder_groups SET position = ? WHERE id = ?",
                params![position as i64, id],
            );
        }
    }

    /// Persist a new display order of one group's folders (`ordered_paths`
    /// must contain every path of the group exactly once). [WRITER]
    pub fn reorder_folder_group_items(&self, id: i64, ordered_paths: &[String]) {
        let Ok(db) = self.writer.lock() else {
            return;
        };
        for (position, path) in ordered_paths.iter().enumerate() {
            let _ = db.execute(
                "UPDATE folder_group_paths SET position = ?
                 WHERE group_id = ? AND path = ?",
                params![position as i64, id, path],
            );
        }
    }

    /// Move one folder from group `from_id` into group `to_id`, keeping its
    /// custom display name. `insertion` (`None` = append) is the target
    /// position index in the destination group. Returns false when the path
    /// already exists in the destination. Both groups are renumbered without
    /// gaps. [WRITER]
    pub fn move_folder_group_item(
        &self,
        from_id: i64,
        path: &str,
        to_id: i64,
        insertion: Option<usize>,
    ) -> bool {
        let Ok(db) = self.writer.lock() else {
            return false;
        };
        // Duplicate in destination refuses the move.
        let exists: bool = db
            .query_row(
                "SELECT COUNT(*) FROM folder_group_paths WHERE group_id = ? AND path = ?",
                params![to_id, path],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(true);
        if exists || from_id == to_id {
            return false;
        }
        // Carry the custom display name over.
        let name: Option<String> = db
            .query_row(
                "SELECT name FROM folder_group_paths WHERE group_id = ? AND path = ?",
                params![from_id, path],
                |row| row.get(0),
            )
            .ok();
        if db
            .execute(
                "DELETE FROM folder_group_paths WHERE group_id = ? AND path = ?",
                params![from_id, path],
            )
            .is_err()
        {
            return false;
        }
        let position = match insertion {
            Some(index) => {
                let index = index as i64;
                let _ = db.execute(
                    "UPDATE folder_group_paths SET position = position + 1
                     WHERE group_id = ? AND position >= ?",
                    params![to_id, index],
                );
                index
            }
            None => {
                db.query_row(
                    "SELECT COALESCE(MAX(position), -1) + 1 FROM folder_group_paths
                     WHERE group_id = ?",
                    params![to_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0)
            }
        };
        let inserted = db
            .execute(
                "INSERT INTO folder_group_paths (group_id, path, position, name)
                 VALUES (?, ?, ?, ?)",
                params![to_id, path, position, name],
            )
            .is_ok();
        // Compact the source group's positions after the removal.
        if let Ok(mut stmt) =
            db.prepare("SELECT path FROM folder_group_paths WHERE group_id = ? ORDER BY position")
        {
            if let Ok(rows) = stmt.query_map(params![from_id], |row| row.get::<_, String>(0)) {
                let remaining: Vec<String> = rows.flatten().collect();
                for (position, path) in remaining.iter().enumerate() {
                    let _ = db.execute(
                        "UPDATE folder_group_paths SET position = ? WHERE group_id = ? AND path = ?",
                        params![position as i64, from_id, path],
                    );
                }
            }
        }
        inserted
    }

    /// Remove a group and its folders. [WRITER]
    pub fn delete_folder_group(&self, id: i64) {
        if let Ok(db) = self.writer.lock() {
            let _ = db.execute("DELETE FROM folder_group_paths WHERE group_id = ?", params![id]);
            let _ = db.execute("DELETE FROM folder_groups WHERE id = ?", params![id]);
            log::info!("[GROUPS] Deleted group {}", id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mtt_groups_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn groups_round_trip_with_paths_and_unique_names() {
        let dir = temp_state_dir();
        let db = AppStateDb::new(dir.clone()).expect("test db");
        let paths = vec!["C:\\One".to_string(), "D:\\Two".to_string()];
        let id = db.save_folder_group("Work", &paths).expect("insert");
        let groups = db.get_all_folder_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, id);
        assert_eq!(groups[0].name, "Work");
        assert!(groups[0].expanded);
        assert_eq!(groups[0].paths().collect::<Vec<_>>(), vec!["C:\\One", "D:\\Two"]);

        // Case-insensitive uniqueness refuses the second insert.
        assert!(db.save_folder_group("work", &paths).is_none());

        // Display names and expansion persist.
        db.set_folder_group_item_name(id, "C:\\One", Some("First"));
        db.set_folder_group_expanded(id, false);
        let groups = db.get_all_folder_groups();
        assert_eq!(groups[0].items[0].name.as_deref(), Some("First"));
        assert!(!groups[0].expanded);

        // Replacing the paths keeps the name of a surviving path.
        db.update_folder_group_paths(id, &["C:\\One".to_string(), "E:\\Three".to_string()]);
        let groups = db.get_all_folder_groups();
        assert_eq!(groups[0].items.len(), 2);
        assert_eq!(groups[0].items[0].name.as_deref(), Some("First"));
        assert!(groups[0].items[1].name.is_none());

        // Removing one folder leaves the rest.
        db.remove_folder_group_item(id, "C:\\One");
        assert_eq!(db.get_all_folder_groups()[0].items.len(), 1);

        db.delete_folder_group(id);
        assert!(db.get_all_folder_groups().is_empty());
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reordering_groups_and_items_persists_display_order() {
        let dir = temp_state_dir();
        let db = AppStateDb::new(dir.clone()).expect("test db");
        let a = db.create_folder_group("A").expect("create A");
        let b = db.create_folder_group("B").expect("create B");
        db.add_folder_group_items(a, &["A1".to_string(), "A2".to_string(), "A3".to_string()]);

        // Drag B above A.
        db.reorder_folder_groups(&[b, a]);
        let groups = db.get_all_folder_groups();
        assert_eq!(groups[0].id, b);
        assert_eq!(groups[1].id, a);

        // Drag A2 to the front of its group.
        db.reorder_folder_group_items(a, &["A2".to_string(), "A3".to_string(), "A1".to_string()]);
        let groups = db.get_all_folder_groups();
        let items: Vec<_> = groups.iter().find(|g| g.id == a).expect("group A")
            .paths().map(str::to_owned).collect();
        assert_eq!(items, vec!["A2", "A3", "A1"]);

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adding_folders_skips_duplicates_and_keeps_order() {
        let dir = temp_state_dir();
        let db = AppStateDb::new(dir.clone()).expect("test db");
        let id = db.create_folder_group("G").expect("create");
        assert_eq!(
            db.add_folder_group_items(
                id,
                &["A".to_string(), "B".to_string(), "A".to_string()]
            ),
            2
        );
        assert_eq!(
            db.get_all_folder_groups()[0]
                .paths()
                .collect::<Vec<_>>(),
            vec!["A", "B"]
        );
        // Appending keeps the existing order.
        db.add_folder_group_items(id, &["C".to_string()]);
        assert_eq!(
            db.get_all_folder_groups()[0]
                .paths()
                .collect::<Vec<_>>(),
            vec!["A", "B", "C"]
        );
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
