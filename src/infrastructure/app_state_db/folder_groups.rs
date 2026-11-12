use super::AppStateDb;
use crate::domain::folder_group::FolderGroup;
use rusqlite::params;

impl AppStateDb {
    /// Load every saved group with its ordered paths. [READER]
    pub fn get_all_folder_groups(&self) -> Vec<FolderGroup> {
        let Ok(db) = self.reader.lock() else {
            log::error!("[GROUPS] Failed to acquire reader lock");
            return Vec::new();
        };
        let Ok(mut stmt) =
            db.prepare("SELECT id, name, position FROM folder_groups ORDER BY position ASC, id ASC")
        else {
            log::error!("[GROUPS] Failed to prepare group SELECT");
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |row| {
            Ok(FolderGroup {
                id: row.get(0)?,
                name: row.get(1)?,
                position: row.get(2)?,
                paths: Vec::new(),
            })
        }) else {
            return Vec::new();
        };
        let mut groups: Vec<FolderGroup> = rows.flatten().collect();
        let Ok(mut stmt) = db
            .prepare("SELECT group_id, path FROM folder_group_paths ORDER BY group_id, position ASC")
        else {
            return groups;
        };
        let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
        else {
            return groups;
        };
        for (group_id, path) in rows.flatten() {
            if let Some(group) = groups.iter_mut().find(|group| group.id == group_id) {
                group.paths.push(path);
            }
        }
        groups
    }

    /// Create a group (name is unique, case-insensitive). Returns the new row
    /// id, or `None` when the name already exists. [WRITER]
    pub fn save_folder_group(&self, name: &str, paths: &[String]) -> Option<i64> {
        let Ok(db) = self.writer.lock() else {
            return None;
        };
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
        let id = db.last_insert_rowid();
        for (position, path) in paths.iter().enumerate() {
            let _ = db.execute(
                "INSERT OR IGNORE INTO folder_group_paths (group_id, path, position)
                 VALUES (?, ?, ?)",
                params![id, path, position as i64],
            );
        }
        log::info!("[GROUPS] Saved group {:?} ({} paths)", name, paths.len());
        Some(id)
    }

    /// Replace a group's saved paths (keeps name and position). [WRITER]
    pub fn update_folder_group_paths(&self, id: i64, paths: &[String]) {
        if let Ok(db) = self.writer.lock() {
            let _ = db.execute("DELETE FROM folder_group_paths WHERE group_id = ?", params![id]);
            for (position, path) in paths.iter().enumerate() {
                let _ = db.execute(
                    "INSERT OR IGNORE INTO folder_group_paths (group_id, path, position)
                     VALUES (?, ?, ?)",
                    params![id, path, position as i64],
                );
            }
        }
    }

    /// Remove a group and its paths. [WRITER]
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
        assert_eq!(groups[0].paths, paths);

        // Case-insensitive uniqueness refuses the second insert.
        assert!(db.save_folder_group("work", &paths).is_none());

        db.update_folder_group_paths(id, &["E:\\Three".to_string()]);
        let groups = db.get_all_folder_groups();
        assert_eq!(groups[0].paths, vec!["E:\\Three".to_string()]);

        db.delete_folder_group(id);
        assert!(db.get_all_folder_groups().is_empty());
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
