/// One saved folder inside a favorite group.
///
/// Renaming only changes the label shown in the panel; the folder on disk is
/// never renamed or moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderGroupItem {
    /// Absolute folder path.
    pub path: String,
    /// Optional custom display name; `None` falls back to the folder name or
    /// the full path depending on the "names only" preference.
    pub name: Option<String>,
}

impl FolderGroupItem {
    pub fn new(path: String) -> Self {
        Self { path, name: None }
    }

    /// Folder name, falling back to the full path at a drive root.
    pub fn folder_name(&self) -> String {
        let trimmed = self.path.trim_end_matches(['\\', '/']);
        trimmed
            .rsplit(['\\', '/'])
            .find(|segment| !segment.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| self.path.clone())
    }

    /// Label shown in the panel: a custom name wins; otherwise the folder name
    /// or the full path depending on `names_only`.
    pub fn label(&self, names_only: bool) -> String {
        if let Some(name) = self.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            return name.to_owned();
        }
        if names_only {
            self.folder_name()
        } else {
            self.path.clone()
        }
    }
}

/// A named group of folders shown in the left dock's 【列表】 page.
///
/// Clicking a folder navigates to it; opening the whole group restores every
/// folder as its own tab (like a browser bookmark group).
#[derive(Debug, Clone)]
pub struct FolderGroup {
    /// Persistent row id from `folder_groups`.
    pub id: i64,
    /// Unique, case-insensitive group display name.
    pub name: String,
    /// Position index for ordering (0-based, ascending).
    pub position: i64,
    /// Whether the group is expanded in the panel.
    pub expanded: bool,
    /// Saved folders in display order.
    pub items: Vec<FolderGroupItem>,
}

impl FolderGroup {
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|item| item.path.as_str())
    }

    pub fn contains_path(&self, path: &str) -> bool {
        self.items
            .iter()
            .any(|item| item.path.eq_ignore_ascii_case(path))
    }
}
