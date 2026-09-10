use super::*;

/// Composes a shell whose workspace shows a dock column.
fn state_with_dock() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_dock_surface(Some(dock_surface()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("pane frame");
    state
}

fn press(state: &mut ClientShellState, ch: char) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char(ch),
        KeyModifiers::empty(),
    ))])
}

fn click(state: &mut ClientShellState, column: u16, row: u16) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })])
}

#[test]
fn a_click_in_the_dock_moves_the_keyboard_there_and_a_click_outside_gives_it_back() {
    let mut state = state_with_dock();

    // Before any click the pane still owns the keyboard, even though the dock
    // is on screen. The dock is not modal.
    let typed = press(&mut state, 'a');
    assert!(
        matches!(
            &typed.requests[..],
            [ClientMessage::ClientShellPaneInput { pane_id, .. }] if pane_id == "pane_1"
        ),
        "an unfocused dock must not take the keyboard: {:?}",
        typed.requests
    );

    let dock = state.hits.dock.clone().expect("dock hit geometry");
    click(&mut state, dock.inner_rect.x + 2, dock.inner_rect.y + 1);
    let typed = press(&mut state, 'b');
    assert!(
        matches!(
            &typed.requests[..],
            [ClientMessage::ClientShellDockInput { terminal_id, .. }]
                if terminal_id == "terminal-dock"
        ),
        "a click in the dock must send the next key to the dock terminal: {:?}",
        typed.requests
    );

    let pane = state.hits.panes[0].clone();
    click(&mut state, pane.inner_rect.x, pane.inner_rect.y);
    let typed = press(&mut state, 'c');
    assert!(
        matches!(
            &typed.requests[..],
            [ClientMessage::ClientShellPaneInput { pane_id, .. }] if pane_id == "pane_1"
        ),
        "a click on a pane must hand the keyboard back: {:?}",
        typed.requests
    );
}

#[test]
fn a_dock_that_leaves_the_surface_hands_the_keyboard_back_without_a_click() {
    let mut state = state_with_dock();
    let dock = state.hits.dock.clone().expect("dock hit geometry");
    click(&mut state, dock.inner_rect.x + 2, dock.inner_rect.y + 1);

    // The dock collapses. No click reports that, so the keyboard has to follow
    // the geometry on its own.
    state.set_dock_surface(None);
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("pane frame");

    let typed = press(&mut state, 'd');
    assert!(
        matches!(
            &typed.requests[..],
            [ClientMessage::ClientShellPaneInput { pane_id, .. }] if pane_id == "pane_1"
        ),
        "a collapsed dock must not keep the keyboard: {:?}",
        typed.requests
    );
}

#[test]
fn the_text_cursor_follows_the_keyboard_into_the_dock_and_back() {
    let mut state = state_with_dock();
    let pane_cursor = state.compose(106, 20).expect("pane frame").cursor;
    assert!(
        pane_cursor.is_some(),
        "the harness must start with a cursor in the pane"
    );

    let dock = state.hits.dock.clone().expect("dock hit geometry");
    click(&mut state, dock.inner_rect.x + 2, dock.inner_rect.y + 1);
    let focused = state.compose(106, 20).expect("pane frame");
    let cursor = focused.cursor.expect("a focused dock must own the cursor");
    // The wire places the dock's inner rect at (75, 1) and its cursor at
    // (78, 3), so the cursor must land three columns and two rows into the
    // dock wherever the pane surface itself begins on screen.
    assert_eq!(
        (cursor.x, cursor.y),
        (dock.inner_rect.x + 3, dock.inner_rect.y + 2),
        "the cursor sits where the dock process put it"
    );

    let pane = state.hits.panes[0].clone();
    click(&mut state, pane.inner_rect.x, pane.inner_rect.y);
    assert_eq!(
        state.compose(106, 20).expect("pane frame").cursor,
        pane_cursor,
        "and it returns to the pane when the pane takes the keyboard back"
    );
}

#[test]
fn a_click_beside_the_dock_still_reaches_the_pane_under_it() {
    let mut state = state_with_dock();
    let pane = state.hits.panes[0].clone();

    // The dock branch must fall through on a miss. A branch that returned
    // unconditionally would swallow this click.
    click(&mut state, pane.inner_rect.x + 1, pane.inner_rect.y);
    let recorded = state
        .last_pane_click
        .as_ref()
        .expect("the pane layer must see the click");
    assert_eq!(recorded.pane_id, "pane_1");
    assert_eq!((recorded.viewport_row, recorded.col), (0, 1));
}

#[test]
fn the_toggle_dock_key_asks_the_server_to_collapse_the_column() {
    let mut state = state_with_dock();

    // The key is a client-side keybind, but collapsing the dock is server
    // state: every attached client has to see the same column. So the shell
    // must turn the keybind into an API call, not into a local flag.
    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::ToggleDock),
        &mut outcome,
    );

    let [ClientShellAction::Endpoint { request, .. }] = &outcome.actions[..] else {
        panic!(
            "the dock key must reach the endpoint API, got {:?}",
            outcome.actions
        );
    };
    assert!(
        matches!(&request.method, crate::api::schema::Method::DockToggle(_)),
        "expected dock.toggle, got {:?}",
        request.method
    );
}
