//! Clicking issues and what gets focus.

use super::*;

fn suspended_tab() -> Tab {
    let mut t = unstarted_tab();
    t.st.claude_session = Some("s".into());
    t
}

/// Nothing tried to spawn: a failed spawn would leave `pane == None` but flash.
fn assert_no_spawn(app: &App) {
    assert!(app.msg.is_none(), "unexpected flash: {:?}", app.msg.as_ref().map(|m| &m.0));
}

/// Scroll the wheel down once over the sidebar's issue list.
fn wheel_down_sidebar(app: &mut App) {
    draw(app);
    let pos = app.item_hits.first().expect("issue row drawn").0;
    let pos = Position::new(pos.x, pos.y);
    mouse(app, MouseEventKind::ScrollDown, pos);
}

#[test]
fn clicking_an_issue_keeps_focus_in_the_sidebar() {
    let (mut app, vault) = click_app(&["a"]);
    with_tabs(&mut app, "a", vec![unstarted_tab(), live(TabKind::Claude, &vault.0)], 1);
    app.focus = Focus::Terminal;

    click(&mut app, "a");
    assert_eq!(app.selected.as_deref(), Some("a"));
    assert_eq!(app.focus, Focus::Sidebar);
    assert_eq!(app.rt["a"].active, 1, "the terminal still shows the last-used tab");
    assert!(app.rt["a"].tabs[0].pane.is_none());
    assert_no_spawn(&app);

    // Clicking the issue that is already selected also returns to the sidebar.
    app.focus = Focus::Terminal;
    click(&mut app, "a");
    assert_eq!(app.selected.as_deref(), Some("a"));
    assert_eq!(app.focus, Focus::Sidebar);
    assert_no_spawn(&app);
}

#[test]
fn clicking_another_issue_from_a_focused_tab_focuses_the_sidebar() {
    let (mut app, vault) = click_app(&["a", "b"]);
    with_tabs(&mut app, "a", vec![live(TabKind::Claude, &vault.0)], 0);
    with_tabs(&mut app, "b", vec![live(TabKind::Claude, &vault.0)], 0);
    app.select_id("a".into());
    app.focus = Focus::Terminal;

    click(&mut app, "b");
    assert_eq!(app.selected.as_deref(), Some("b"));
    assert_eq!(app.focus, Focus::Sidebar);
    assert_no_spawn(&app);
}

#[test]
fn clicking_an_issue_with_a_suspended_tab_does_not_resume_it() {
    let (mut app, _vault) = click_app(&["a"]);
    with_tabs(&mut app, "a", vec![suspended_tab()], 0);

    click(&mut app, "a");
    assert_eq!(app.focus, Focus::Sidebar);
    let rt = &app.rt["a"];
    assert_eq!(rt.tabs.len(), 1);
    assert!(rt.tabs[0].pane.is_none() && rt.tabs[0].suspended());
    assert_no_spawn(&app);

    // ⏎ resumes it: the test's Claude command can't start, so the spawn error
    // shows the command it tried.
    app.handle(Event::Key(KeyEvent::from(KeyCode::Enter)));
    assert_eq!(app.focus, Focus::Terminal);
    let msg = flashed(&app);
    assert!(msg.starts_with("Spawn failed") && msg.contains("--resume s"), "⏎ didn't resume: {msg}");
}

#[test]
fn clicking_an_issue_with_an_exited_tab_does_not_restart_it() {
    let (mut app, vault) = click_app(&["a"]);
    let mut exited = live(TabKind::Claude, &vault.0);
    exited.pane.as_mut().unwrap().exited = Some(1);
    with_tabs(&mut app, "a", vec![exited], 0);

    click(&mut app, "a");
    assert_eq!(app.focus, Focus::Sidebar);
    let rt = &app.rt["a"];
    assert_eq!(rt.tabs.len(), 1);
    assert_eq!(rt.tabs[0].pane.as_ref().and_then(|p| p.exited), Some(1));
    assert_no_spawn(&app);
}

#[test]
fn clicking_a_busy_issue_then_d_asks_to_move_it_to_done() {
    let (mut app, vault) = click_app(&["a"]);
    let mut t = live(TabKind::Claude, &vault.0);
    t.status = ClaudeStatus::Working;
    with_tabs(&mut app, "a", vec![t], 0);
    app.select_id("a".into());
    app.focus = Focus::Terminal;

    click(&mut app, "a");
    // `answer` goes through focus routing: had the click left the terminal
    // focused, `d` would reach Claude and no dialog would open.
    answer(&mut app, 'd');
    let Mode::Confirm(ConfirmKind::Retire { issue, stage, busy }) = app.mode.clone() else { panic!("`d` didn't reach the sidebar") };
    assert_eq!((issue.as_str(), stage), ("a", Stage::Done));
    assert_eq!(busy, ["Claude: working"]);
}

#[test]
fn clicking_a_tab_header_focuses_the_terminal() {
    let (mut app, vault) = click_app(&["a"]);
    with_tabs(&mut app, "a", vec![live(TabKind::Claude, &vault.0), suspended_tab()], 0);
    click(&mut app, "a");
    assert_eq!(app.focus, Focus::Sidebar);

    for i in [0, 1] {
        app.focus = Focus::Sidebar;
        draw(&mut app);
        let r = app.tab_hits.iter().find(|(_, t)| *t == i).expect("tab header drawn").0;
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), Position::new(r.x + 1, r.y));
        assert_eq!(app.focus, Focus::Terminal);
        assert_eq!(app.rt["a"].active, i);
    }
    assert!(app.rt["a"].tabs[1].suspended(), "showing a suspended tab doesn't resume it");
    assert_no_spawn(&app);
}

#[test]
fn moving_from_a_tab_to_an_issue_without_a_live_tab_focuses_the_sidebar() {
    // The sidebar wheel and prefix j both leave a focused tab for the next issue.
    let moves: [fn(&mut App); 2] = [wheel_down_sidebar, |app| {
        let (code, mods) = app.prefix;
        app.handle(Event::Key(KeyEvent::new(code, mods)));
        app.handle(Event::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)));
    }];
    for move_down in moves {
        let (mut app, _vault) = click_app(&["a", "b", "c"]);
        with_tabs(&mut app, "a", vec![suspended_tab()], 0);
        with_tabs(&mut app, "b", vec![suspended_tab()], 0);
        with_tabs(&mut app, "c", vec![unstarted_tab()], 0);
        app.select_id("a".into());
        app.focus = Focus::Terminal;

        // A suspended tab is live: stay in the terminal.
        move_down(&mut app);
        assert_eq!(app.selected.as_deref(), Some("b"));
        assert_eq!(app.focus, Focus::Terminal);

        // An unstarted tab is not: back to the sidebar.
        move_down(&mut app);
        assert_eq!(app.selected.as_deref(), Some("c"));
        assert_eq!(app.focus, Focus::Sidebar);
        assert!(app.rt.values().flat_map(|rt| &rt.tabs).all(|t| t.pane.is_none()));
        assert_no_spawn(&app);
    }
}
