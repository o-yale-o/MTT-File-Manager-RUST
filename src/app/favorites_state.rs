//! Inline-rename state for the left dock's 【列表】 page.
//!
//! The page renders a `TextEdit` in place of the edited row while this state is
//! set, so no separate dialog window is involved. Committing writes through to
//! `app_state.db`; cancelling only clears the state.

/// Which row is being renamed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FavoritesRenameTarget {
    /// A favorite group, identified by its persistent row id.
    Group(i64),
    /// One folder inside a group.
    Item { group_id: i64, path: String },
}

/// One in-progress rename.
#[derive(Clone, Debug)]
pub struct FavoritesRename {
    pub target: FavoritesRenameTarget,
    /// The text currently in the inline editor.
    pub text: String,
}

/// A favorite/recent folder whose existence is being checked on a worker thread
/// before the tab is created. The check runs off the UI thread because a folder
/// on a sleeping disk, a dead network share or a stalled cloud placeholder can
/// block `is_dir()` for a long time.
pub struct PendingFavoriteOpen {
    /// `Some(group id)` for a 收藏 item, `None` for a 最近访问 row.
    pub group_id: Option<i64>,
    pub path: String,
    pub started: std::time::Instant,
    pub receiver: std::sync::mpsc::Receiver<bool>,
}

impl FavoritesRename {
    pub fn new(target: FavoritesRenameTarget, text: String) -> Self {
        Self { target, text }
    }

    pub fn is_group(&self, id: i64) -> bool {
        matches!(&self.target, FavoritesRenameTarget::Group(group_id) if *group_id == id)
    }

    pub fn is_item(&self, group_id: i64, path: &str) -> bool {
        matches!(
            &self.target,
            FavoritesRenameTarget::Item { group_id: target_group, path: target_path }
                if *target_group == group_id && target_path == path
        )
    }
}
