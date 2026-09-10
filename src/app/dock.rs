//! Opening and closing the process a workspace's dock holds.
//!
//! Modelled on `src/app/popup.rs`. The two differ in three ways: a dock belongs
//! to one workspace rather than to the application, a dock does not take over
//! input mode, and a dock's size comes from the reserved column rather than
//! from a popup geometry.

use std::path::PathBuf;

use crate::app::App;
use crate::dock::DockPaneState;
use crate::layout::PaneId;
use crate::pane::PaneLaunchEnv;
use crate::terminal::{TerminalId, TerminalRuntime, TerminalState};

impl App {
    /// Close the dock process of `ws_idx`. Returns false when it had none.
    pub(crate) fn close_dock_pane(&mut self, ws_idx: usize) -> bool {
        let Some(dock) = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .and_then(|workspace| workspace.dock_pane.take())
        else {
            return false;
        };
        self.state
            .direct_attach_resize_locks
            .remove(&dock.terminal_id);
        self.state.terminals.remove(&dock.terminal_id);
        self.shutdown_terminal_runtime(dock.terminal_id);
        self.render_dirty.request_generic();
        self.render_notify.notify_one();
        true
    }

    pub(crate) fn spawn_dock_argv_command(
        &mut self,
        ws_idx: usize,
        argv: &[String],
        cwd: Option<PathBuf>,
        extra_env: Vec<(String, String)>,
    ) -> std::io::Result<PaneId> {
        self.spawn_dock_command(
            ws_idx,
            cwd,
            extra_env,
            |pane_id, rows, cols, cwd, launch_env, app| {
                TerminalRuntime::spawn_argv_command(
                    pane_id,
                    rows,
                    cols,
                    cwd,
                    argv,
                    launch_env,
                    crate::pane::AgentDetection::Disabled,
                    app.state.pane_scrollback_limit_bytes,
                    app.state.host_terminal_theme,
                    app.state.host_terminal_appearance,
                    app.event_tx.clone(),
                    app.render_notify.clone(),
                    app.render_dirty.clone(),
                )
                .map(|runtime| (runtime, Some(argv.to_vec())))
            },
        )
    }

    fn spawn_dock_command<F>(
        &mut self,
        ws_idx: usize,
        cwd: Option<PathBuf>,
        extra_env: Vec<(String, String)>,
        spawn: F,
    ) -> std::io::Result<PaneId>
    where
        F: FnOnce(
            PaneId,
            u16,
            u16,
            PathBuf,
            &PaneLaunchEnv,
            &mut App,
        ) -> std::io::Result<(TerminalRuntime, Option<Vec<String>>)>,
    {
        if self.state.dock.is_none() {
            return Err(std::io::Error::other("dock is disabled"));
        }
        let workspace = self
            .state
            .workspaces
            .get(ws_idx)
            .ok_or_else(|| std::io::Error::other("no such workspace"))?;
        if workspace.dock_pane.is_some() {
            return Err(std::io::Error::other("dock already open"));
        }
        let cwd = cwd.or_else(|| {
            let focused = workspace.focused_pane_id()?;
            workspace.active_tab()?.cwd_for_pane(
                focused,
                &self.state.terminals,
                &self.terminal_runtimes,
            )
        });
        let cwd = cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()));

        let (rows, cols) = self.dock_inner_size();
        let pane_id = PaneId::alloc();
        let terminal_id = TerminalId::alloc();
        let launch_env = PaneLaunchEnv::from_extra(extra_env).without_pane_identity();
        let (runtime, launch_argv) = spawn(pane_id, rows, cols, cwd.clone(), &launch_env, self)?;
        let terminal = match launch_argv {
            Some(argv) => TerminalState::new(terminal_id.clone(), cwd).with_launch_argv(argv),
            None => TerminalState::new(terminal_id.clone(), cwd),
        };
        self.terminal_runtimes.insert(terminal_id.clone(), runtime);
        self.state.terminals.insert(terminal_id.clone(), terminal);
        self.state
            .workspaces
            .get_mut(ws_idx)
            .ok_or_else(|| std::io::Error::other("workspace disappeared while spawning"))?
            .dock_pane = Some(DockPaneState {
            pane_id,
            terminal_id,
        });
        self.render_dirty.request_generic();
        self.render_notify.notify_one();
        Ok(pane_id)
    }

    /// Rows and columns the dock's process gets.
    ///
    /// Falls back to an estimate when no frame has been laid out yet, the same
    /// way the popup does: a process must be spawned with some size, and a zero
    /// one makes the child think its terminal is broken.
    fn dock_inner_size(&self) -> (u16, u16) {
        let area = self.state.view.terminal_area;
        let area = if area.width >= 4 && area.height >= 4 {
            area
        } else {
            let (rows, cols) = self.state.estimate_pane_size();
            ratatui::layout::Rect::new(0, 0, cols, rows)
        };
        crate::ui::dock_pane_rects(&self.state, area).map_or((1, 1), |(_outer, inner)| {
            (inner.height.max(1), inner.width.max(1))
        })
    }
}
