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
fn clicking_an_issue_focuses_its_last_active_tab() {
    let (mut app, vault) = click_app(&["a"]);
    with_tabs(&mut app, "a", vec![unstarted_tab(), live(TabKind::Claude, &vault.0)], 1);

    click(&mut app, "a");
    assert_eq!(app.selected.as_deref(), Some("a"));
    assert_eq!(app.focus, Focus::Terminal);
    assert_eq!(app.rt["a"].active, 1);
    assert!(app.rt["a"].tabs[0].pane.is_none());
    assert_no_spawn(&app);

    // Clicking the issue that is already selected also jumps into its tab.
    app.focus = Focus::Sidebar;
    click(&mut app, "a");
    assert_eq!(app.selected.as_deref(), Some("a"));
    assert_eq!(app.focus, Focus::Terminal);
    assert_no_spawn(&app);
}

#[test]
fn clicking_an_issue_shows_a_suspended_tab_without_resuming_it() {
    let (mut app, _vault) = click_app(&["a"]);
    with_tabs(&mut app, "a", vec![suspended_tab()], 0);

    click(&mut app, "a");
    assert_eq!(app.focus, Focus::Terminal);
    let rt = &app.rt["a"];
    assert_eq!(rt.tabs.len(), 1);
    assert!(rt.tabs[0].pane.is_none() && rt.tabs[0].suspended());
    assert_no_spawn(&app);
}

#[test]
fn clicking_an_issue_shows_an_exited_tab_without_restarting_it() {
    let (mut app, vault) = click_app(&["a"]);
    let mut exited = live(TabKind::Claude, &vault.0);
    exited.pane.as_mut().unwrap().exited = Some(1);
    with_tabs(&mut app, "a", vec![exited], 0);

    click(&mut app, "a");
    assert_eq!(app.focus, Focus::Terminal);
    let rt = &app.rt["a"];
    assert_eq!(rt.tabs.len(), 1);
    assert_eq!(rt.tabs[0].pane.as_ref().and_then(|p| p.exited), Some(1));
    assert_no_spawn(&app);
}

#[test]
fn clicking_an_issue_without_a_live_tab_stays_in_the_sidebar() {
    let (mut app, _vault) = click_app(&["empty", "unstarted"]);
    with_tabs(&mut app, "unstarted", vec![unstarted_tab()], 0);

    click(&mut app, "empty");
    assert_eq!(app.selected.as_deref(), Some("empty"));
    assert_eq!(app.focus, Focus::Sidebar);
    assert!(app.rt["empty"].tabs.is_empty() && app.rt["empty"].worktree.is_none());

    click(&mut app, "unstarted");
    assert_eq!(app.selected.as_deref(), Some("unstarted"));
    assert_eq!(app.focus, Focus::Sidebar);
    assert!(app.rt["unstarted"].tabs[0].pane.is_none());
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
        click(&mut app, "a");
        assert_eq!(app.focus, Focus::Terminal);

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
