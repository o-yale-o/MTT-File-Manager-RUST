/// A named group of folder paths saved from the open tabs and shown under
/// Quick Access. Opening the group restores every path as its own tab.
#[derive(Debug, Clone)]
pub struct FolderGroup {
    /// Persistent row id from `folder_groups`.
    pub id: i64,
    /// Unique, case-insensitive group display name.
    pub name: String,
    /// Position index for ordering (0-based, ascending).
    pub position: i64,
    /// Saved paths in tab order.
    pub paths: Vec<String>,
}
