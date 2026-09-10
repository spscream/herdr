//! Opening and closing the process a workspace's dock holds.
//!
//! Modelled on `src/app/popup.rs`. The two differ in three ways: a dock belongs
//! to one workspace rather than to the application, a dock does not take over
//! input mode, and a dock's size comes from the reserved column rather than
//! from a popup geometry.

use std::path::PathBuf;

use crate::app::api::responses;
use crate::app::App;
use crate::dock::DockPaneState;
use crate::layout::PaneId;
use crate::pane::PaneLaunchEnv;
use crate::terminal::{TerminalId, TerminalRuntime, TerminalState};

impl App {
    /// Answer `dock.get`: what the workspace's dock column is right now.
    pub(crate) fn handle_dock_get(
        &mut self,
        request_id: String,
        params: crate::api::schema::DockTarget,
    ) -> String {
        let Some(ws_idx) = self.dock_target_workspace(params.workspace_id.as_deref()) else {
            return responses::encode_error(
                request_id,
                "workspace_not_found",
                "workspace not found",
            );
        };
        responses::encode_success(
            request_id,
            crate::api::schema::ResponseResult::Dock {
                dock: self.dock_info(ws_idx),
            },
        )
    }

    /// Answer `dock.close`: stop the process, leave the column configured.
    pub(crate) fn handle_dock_close(
        &mut self,
        request_id: String,
        params: crate::api::schema::DockTarget,
    ) -> String {
        let Some(ws_idx) = self.dock_target_workspace(params.workspace_id.as_deref()) else {
            return responses::encode_error(
                request_id,
                "workspace_not_found",
                "workspace not found",
            );
        };
        if !self.close_dock_pane(ws_idx) {
            return responses::encode_error(
                request_id,
                "dock_not_open",
                "the workspace dock holds no process",
            );
        }
        responses::encode_success(request_id, crate::api::schema::ResponseResult::Ok {})
    }

    /// The workspace a dock request is about: the named one, or the active one.
    fn dock_target_workspace(&self, workspace_id: Option<&str>) -> Option<usize> {
        match workspace_id {
            Some(workspace_id) => self.parse_workspace_id(workspace_id),
            None => self.state.active,
        }
        .filter(|ws_idx| *ws_idx < self.state.workspaces.len())
    }

    fn dock_info(&self, ws_idx: usize) -> crate::api::schema::DockInfo {
        let dock_pane = self
            .state
            .workspaces
            .get(ws_idx)
            .and_then(|workspace| workspace.dock_pane.as_ref());
        crate::api::schema::DockInfo {
            workspace_id: self.public_workspace_id(ws_idx),
            dock_id: self.public_dock_id(ws_idx).unwrap_or_default(),
            enabled: self.state.dock.is_some(),
            collapsed: self.state.dock_collapsed(),
            edge: self.state.dock.map(|dock| dock.edge),
            width: self.state.dock.map(|dock| dock.width),
            title: dock_pane.and_then(|dock| {
                self.state
                    .terminals
                    .get(&dock.terminal_id)
                    .and_then(|terminal| terminal.manual_label.clone())
            }),
            occupied: dock_pane.is_some(),
        }
    }

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
        // The column is not a pane, so it takes the dock identity rather than
        // a pane's: the workspace it serves, and the dock's own identifier.
        let launch_env = self
            .dock_launch_env(ws_idx, extra_env)
            .ok_or_else(|| std::io::Error::other("no such workspace"))?;
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

#[cfg(test)]
mod tests {
    use crate::api::schema::{EmptyParams, Method, Request, ResponseResult};
    use crate::app::App;

    fn app_with_dock(enabled: bool) -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = crate::config::Config::default();
        config.ui.dock = crate::dock::DockConfig {
            enabled,
            edge: crate::dock::DockEdge::Right,
            width: crate::popup_size::PopupSize::Cells(32),
        };
        let mut app = App::new(
            &config,
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("dock")];
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.view.terminal_area = ratatui::layout::Rect::new(0, 0, 100, 30);
        app
    }

    fn toggle(app: &mut App) -> String {
        app.handle_api_request(Request {
            id: "dock-toggle".into(),
            method: Method::DockToggle(EmptyParams::default()),
        })
    }

    fn dock_width(app: &App) -> Option<u16> {
        crate::ui::dock_pane_rects(&app.state, app.state.view.terminal_area)
            .map(|(outer, _inner)| outer.width)
    }

    #[test]
    fn toggling_the_dock_gives_the_column_back_and_takes_it_again() {
        let mut app = app_with_dock(true);
        assert_eq!(dock_width(&app), Some(32));

        let response = toggle(&mut app);
        let response: crate::api::schema::SuccessResponse =
            serde_json::from_str(&response).unwrap();
        assert_eq!(response.result, ResponseResult::Ok {});
        assert_eq!(
            dock_width(&app),
            None,
            "a collapsed dock reserves no column at all"
        );

        toggle(&mut app);
        assert_eq!(dock_width(&app), Some(32), "and toggling again restores it");
    }

    /// Put a process in the dock without a PTY: `dock.get` reads state only.
    fn occupy_dock(app: &mut App, title: &str) -> crate::terminal::TerminalId {
        let terminal_id = crate::terminal::TerminalId::alloc();
        let mut terminal = crate::terminal::TerminalState::new(terminal_id.clone(), "/tmp".into());
        terminal.set_manual_label(title.to_string());
        app.state.terminals.insert(terminal_id.clone(), terminal);
        app.state.workspaces[0].dock_pane = Some(crate::dock::DockPaneState {
            pane_id: crate::layout::PaneId::alloc(),
            terminal_id: terminal_id.clone(),
        });
        terminal_id
    }

    fn get(app: &mut App, workspace_id: Option<&str>) -> String {
        app.handle_api_request(Request {
            id: "dock-get".into(),
            method: Method::DockGet(crate::api::schema::DockTarget {
                workspace_id: workspace_id.map(str::to_string),
            }),
        })
    }

    fn dock_of(response: &str) -> crate::api::schema::DockInfo {
        let response: crate::api::schema::SuccessResponse = serde_json::from_str(response).unwrap();
        match response.result {
            ResponseResult::Dock { dock } => dock,
            other => panic!("expected a dock result, got {other:?}"),
        }
    }

    // The test runtime spawns a compression task, which needs a reactor.
    #[tokio::test]
    async fn the_spawn_path_asks_for_the_dock_identity() {
        let mut app = app_with_dock(true);
        let seen = std::cell::RefCell::new(None);
        let keep_alive = std::cell::RefCell::new(None);

        app.spawn_dock_command(
            0,
            None,
            Vec::new(),
            |_pane_id, rows, cols, _cwd, env, _app| {
                *seen.borrow_mut() = env
                    .dock_identity()
                    .map(|(workspace, dock)| (workspace.to_string(), dock.to_string()));
                let (runtime, rx) = crate::terminal::TerminalRuntime::test_with_channel(cols, rows);
                *keep_alive.borrow_mut() = Some(rx);
                Ok((runtime, None))
            },
        )
        .expect("the dock spawns");

        // Asking `apply_pane_launch_env` directly proves the mapping only. This
        // proves the spawn path chose the dock identity over a pane's.
        assert_eq!(
            seen.into_inner(),
            Some((
                app.public_workspace_id(0),
                format!("{}:dock", app.public_workspace_id(0))
            ))
        );
    }

    #[test]
    fn an_empty_dock_reports_the_column_without_a_process() {
        let mut app = app_with_dock(true);
        let dock = dock_of(&get(&mut app, None));

        assert!(dock.enabled);
        assert!(!dock.occupied, "nothing runs in the column yet");
        assert_eq!(dock.title, None);
        assert_eq!(dock.edge, Some(crate::dock::DockEdge::Right));
        assert_eq!(dock.width, Some(crate::popup_size::PopupSize::Cells(32)));
        assert_eq!(dock.dock_id, format!("{}:dock", dock.workspace_id));
    }

    #[test]
    fn an_occupied_dock_reports_the_title_of_its_process() {
        let mut app = app_with_dock(true);
        occupy_dock(&mut app, "Explorer");

        let dock = dock_of(&get(&mut app, None));
        assert!(dock.occupied);
        assert_eq!(dock.title.as_deref(), Some("Explorer"));
    }

    #[test]
    fn a_dock_that_is_off_reports_no_edge_and_no_width() {
        let mut app = app_with_dock(false);
        let dock = dock_of(&get(&mut app, None));

        assert!(!dock.enabled);
        // A default edge here would read as a fact about a column that does
        // not exist.
        assert_eq!(dock.edge, None);
        assert_eq!(dock.width, None);
    }

    #[test]
    fn a_collapsed_dock_is_still_reported_as_occupied() {
        let mut app = app_with_dock(true);
        occupy_dock(&mut app, "Explorer");
        toggle(&mut app);

        let dock = dock_of(&get(&mut app, None));
        assert!(dock.collapsed);
        assert!(
            dock.occupied,
            "collapsing hides the column; the process keeps running"
        );
    }

    #[test]
    fn a_named_workspace_is_reported_instead_of_the_active_one() {
        let mut app = app_with_dock(true);
        app.state
            .workspaces
            .push(crate::workspace::Workspace::test_new("second"));
        let second = app.public_workspace_id(1);

        let dock = dock_of(&get(&mut app, Some(&second)));
        assert_eq!(dock.workspace_id, second);
        assert_ne!(dock.workspace_id, app.public_workspace_id(0));
    }

    #[test]
    fn an_unknown_workspace_is_refused() {
        let mut app = app_with_dock(true);
        let response = get(&mut app, Some("w_9999"));
        let response: crate::api::schema::ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(response.error.code, "workspace_not_found");
    }

    fn close(app: &mut App) -> String {
        app.handle_api_request(Request {
            id: "dock-close".into(),
            method: Method::DockClose(crate::api::schema::DockTarget::default()),
        })
    }

    #[test]
    fn closing_the_dock_stops_the_process_and_keeps_the_column() {
        let mut app = app_with_dock(true);
        let terminal_id = occupy_dock(&mut app, "Explorer");

        let response: crate::api::schema::SuccessResponse =
            serde_json::from_str(&close(&mut app)).unwrap();
        assert_eq!(response.result, ResponseResult::Ok {});

        assert!(app.state.workspaces[0].dock_pane.is_none());
        assert!(
            !app.state.terminals.contains_key(&terminal_id),
            "the terminal goes with the process"
        );
        assert_eq!(
            dock_width(&app),
            Some(32),
            "closing the process does not give up the column"
        );
        assert!(dock_of(&get(&mut app, None)).enabled);
    }

    #[test]
    fn closing_an_empty_dock_is_refused() {
        let mut app = app_with_dock(true);
        let response = close(&mut app);
        let response: crate::api::schema::ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(response.error.code, "dock_not_open");
    }

    #[test]
    fn toggling_is_refused_when_the_dock_is_off() {
        let mut app = app_with_dock(false);
        let response = toggle(&mut app);
        let response: crate::api::schema::ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(response.error.code, "dock_disabled");
    }
}
