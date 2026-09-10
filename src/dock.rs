//! Workspace dock: a column reserved along one edge of the tab area.
//!
//! The dock is not a pane. It never enters `Tab::panes` and never appears in a
//! `TileLayout`, so none of the tree invariants apply to it. It only shrinks the
//! rectangle that the tab's split tree is laid out into, the same way the
//! client-side sidebar shrinks the columns the server is told about.

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use crate::popup_size::PopupSize;

/// Which side of the tab area the dock occupies.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DockEdge {
    Left,
    #[default]
    Right,
}

/// The dock column, as the whole application sees it.
///
/// One per application, shared by every workspace and every tab. Each workspace
/// runs its own process inside the column -- see [`DockPaneState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockState {
    pub edge: DockEdge,
    pub width: PopupSize,
    pub collapsed: bool,
}

impl DockState {
    /// Width in cells for a tab area `available` columns wide.
    ///
    /// Returns 0 when the dock cannot be shown. The tab always keeps at least
    /// one column, so a dock configured wider than the window shrinks instead of
    /// leaving the tab with nothing to draw into.
    pub fn resolved_width(self, available: u16) -> u16 {
        if self.collapsed {
            return 0;
        }
        self.width
            .resolve(available)
            .min(available.saturating_sub(1))
    }
}

/// Split `area` into the rectangle the tab's panes get and the dock's rectangle.
///
/// The dock rectangle is `None` when there is no dock, when it is collapsed, or
/// when the area is too small to hold both.
pub(crate) fn dock_split(area: Rect, dock: Option<DockState>) -> (Rect, Option<Rect>) {
    let Some(dock) = dock else {
        return (area, None);
    };
    let width = dock.resolved_width(area.width);
    if width == 0 {
        return (area, None);
    }
    let tab_width = area.width - width;
    match dock.edge {
        DockEdge::Left => (
            Rect::new(area.x + width, area.y, tab_width, area.height),
            Some(Rect::new(area.x, area.y, width, area.height)),
        ),
        DockEdge::Right => (
            Rect::new(area.x, area.y, tab_width, area.height),
            Some(Rect::new(area.x + tab_width, area.y, width, area.height)),
        ),
    }
}

/// The process a workspace's dock holds.
///
/// Separate from [`DockState`], which is geometry the whole application shares.
/// Each workspace runs its own dock process, so this lives on the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockPaneState {
    pub pane_id: crate::layout::PaneId,
    pub terminal_id: crate::terminal::TerminalId,
}

/// The `[ui.dock]` section of the configuration file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct DockConfig {
    /// Reserve a dock column along the edge of the tab area. Default: false.
    pub enabled: bool,
    /// Which edge the dock sits on. Default: right.
    pub edge: DockEdge,
    /// Dock width, in cells or as a percentage of the tab area. Default: 32.
    pub width: PopupSize,
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            edge: DockEdge::default(),
            width: PopupSize::Cells(DEFAULT_DOCK_WIDTH),
        }
    }
}

/// Dock width used when the configuration does not give one.
pub const DEFAULT_DOCK_WIDTH: u16 = 32;

impl DockConfig {
    /// The dock this configuration asks for, or `None` when it is off.
    pub fn state(self) -> Option<DockState> {
        self.enabled.then_some(DockState {
            edge: self.edge,
            width: self.width,
            collapsed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dock(edge: DockEdge, width: PopupSize) -> Option<DockState> {
        Some(DockState {
            edge,
            width,
            collapsed: false,
        })
    }

    #[test]
    fn no_dock_leaves_the_area_whole() {
        let area = Rect::new(0, 0, 100, 20);
        assert_eq!(dock_split(area, None), (area, None));
    }

    #[test]
    fn right_dock_takes_the_last_columns() {
        let (tab, dock_rect) = dock_split(
            Rect::new(0, 0, 100, 20),
            dock(DockEdge::Right, PopupSize::Cells(32)),
        );
        assert_eq!(tab, Rect::new(0, 0, 68, 20));
        assert_eq!(dock_rect, Some(Rect::new(68, 0, 32, 20)));
    }

    #[test]
    fn left_dock_takes_the_first_columns() {
        let (tab, dock_rect) = dock_split(
            Rect::new(0, 0, 100, 20),
            dock(DockEdge::Left, PopupSize::Cells(32)),
        );
        assert_eq!(tab, Rect::new(32, 0, 68, 20));
        assert_eq!(dock_rect, Some(Rect::new(0, 0, 32, 20)));
    }

    #[test]
    fn dock_honours_the_area_offset() {
        let (tab, dock_rect) = dock_split(
            Rect::new(10, 3, 100, 20),
            dock(DockEdge::Right, PopupSize::Cells(32)),
        );
        assert_eq!(tab, Rect::new(10, 3, 68, 20));
        assert_eq!(dock_rect, Some(Rect::new(78, 3, 32, 20)));
    }

    #[test]
    fn percent_width_resolves_against_the_area() {
        let (tab, dock_rect) = dock_split(
            Rect::new(0, 0, 100, 20),
            dock(DockEdge::Right, PopupSize::Percent(25)),
        );
        assert_eq!(tab.width, 75);
        assert_eq!(dock_rect.map(|rect| rect.width), Some(25));
    }

    #[test]
    fn collapsed_dock_takes_nothing() {
        let area = Rect::new(0, 0, 100, 20);
        let collapsed = Some(DockState {
            edge: DockEdge::Right,
            width: PopupSize::Cells(32),
            collapsed: true,
        });
        assert_eq!(dock_split(area, collapsed), (area, None));
    }

    #[test]
    fn the_tab_always_keeps_a_column() {
        let (tab, dock_rect) = dock_split(
            Rect::new(0, 0, 20, 5),
            dock(DockEdge::Right, PopupSize::Cells(80)),
        );
        assert_eq!(tab.width, 1);
        assert_eq!(dock_rect.map(|rect| rect.width), Some(19));
    }

    #[test]
    fn an_area_too_small_to_share_gets_no_dock() {
        let area = Rect::new(0, 0, 1, 5);
        assert_eq!(
            dock_split(area, dock(DockEdge::Right, PopupSize::Cells(32))),
            (area, None)
        );
    }
}
