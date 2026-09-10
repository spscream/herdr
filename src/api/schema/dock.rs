//! The workspace dock column, as the API describes it.
//!
//! The dock is not a pane: it never appears in `pane.list` and no pane verb
//! addresses it. These types are the whole of its public surface.

use serde::{Deserialize, Serialize};

/// Which workspace's dock a request is about.
///
/// `None` means the active workspace, matching `plugin.pane.open`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct DockTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

/// The state of one workspace's dock column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DockInfo {
    pub workspace_id: String,
    /// The identifier of the column itself. Stable while the workspace lives,
    /// and set in a dock process's `HERDR_DOCK_ID`.
    pub dock_id: String,
    /// False when `[ui.dock]` is off. Every other field then describes nothing.
    pub enabled: bool,
    /// True when the column is hidden by `dock.toggle`. The process, if any,
    /// keeps running.
    pub collapsed: bool,
    /// Absent when the dock is off: there is then no edge to report, and a
    /// default one would read as a fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<crate::dock::DockEdge>,
    /// Absent when the dock is off, for the same reason as `edge`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<crate::popup_size::PopupSize>,
    /// The title of the running process, or `None` when the column is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// True when a process occupies the column right now. A collapsed dock can
    /// still be occupied.
    pub occupied: bool,
}
