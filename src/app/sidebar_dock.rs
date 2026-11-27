//! Which page the left dock shows: 【列表】 or 【资源管理器】.
//!
//! The state lives on the app (not on a tab) because, like MadEdit's file view
//! panel, the choice is global and survives tab switches and restarts.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum SidebarDockTab {
    /// 收藏（快速访问）与最近访问的文件夹。
    List,
    /// 此电脑、磁盘/云盘/网络位置与文件夹树。
    #[default]
    Explorer,
}

impl SidebarDockTab {
    pub const ALL: [SidebarDockTab; 2] = [SidebarDockTab::List, SidebarDockTab::Explorer];

    pub fn preference_value(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Explorer => "explorer",
        }
    }

    pub fn from_preference(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("list") => Self::List,
            _ => Self::Explorer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SidebarDockTab;

    #[test]
    fn preference_round_trips_and_falls_back_to_explorer() {
        assert_eq!(SidebarDockTab::from_preference(Some("list")), SidebarDockTab::List);
        assert_eq!(
            SidebarDockTab::from_preference(Some(" list ")),
            SidebarDockTab::List
        );
        assert_eq!(
            SidebarDockTab::from_preference(Some("explorer")),
            SidebarDockTab::Explorer
        );
        assert_eq!(SidebarDockTab::from_preference(None), SidebarDockTab::Explorer);
        assert_eq!(
            SidebarDockTab::from_preference(Some("garbage")),
            SidebarDockTab::Explorer
        );
        assert_eq!(SidebarDockTab::List.preference_value(), "list");
        assert_eq!(SidebarDockTab::default(), SidebarDockTab::Explorer);
    }
}
