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

    click(&mut app, "a");
    assert_eq!(app.selected.as_deref(), Some("a"));
    assert_eq!(app.focus, Focus::Sidebar);
    assert_eq!(app.rt["a"].active, 1, "the terminal still shows the last-used tab");
    assert!(app.rt["a"].tabs[0].pane.is_none());
    assert_no_spawn(&app);
}

#[test]
fn clicking_the_selected_issue_from_the_terminal_focuses_the_sidebar() {
    let (mut app, vault) = click_app(&["a"]);
    with_tabs(&mut app, "a", vec![live(TabKind::Claude, &vault.0)], 0);
    app.select_id("a".into());
    app.focus = Focus::Terminal;

    click(&mut app, "a");
    assert_eq!(app.selected.as_deref(), Some("a"));
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

    // ⏎ resumes it: the test's Claude command can't start, so the attempt flashes.
    app.handle(Event::Key(KeyEvent::from(KeyCode::Enter)));
    assert_eq!(app.focus, Focus::Terminal);
    assert!(app.msg.is_some(), "⏎ tried to resume the tab");
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
    let argv = ["sh".to_string(), "-i".to_string()];
    let env = [("PS1".to_string(), "prompt> ".to_string()), ("ENV".to_string(), "/dev/null".to_string())];
    let mut t = fresh(TabKind::Shell, None, None);
    t.pane = Some(Pane::spawn(&argv, &vault.0, &env, 10, 60).unwrap());
    let wait = |t: &Tab, f: &dyn Fn(&Tab) -> bool| {
        let start = Instant::now();
        while !f(t) {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    wait(&t, &|t| t.pane.as_ref().unwrap().snapshot(&[]).windows(7).any(|w| w == b"prompt>"));
    t.pane.as_mut().unwrap().write(b"sleep 30\r");
    wait(&t, &|t| t.busy_label().is_some_and(|b| b != "shell: running command"));
    with_tabs(&mut app, "a", vec![t], 0);

    click(&mut app, "a");
    // With the terminal focused, `d` would go to the shell and no dialog would open.
    key(&mut app, 'd');
    let Mode::Confirm(ConfirmKind::Retire { issue, stage, busy }) = app.mode.clone() else { panic!("`d` didn't reach the sidebar") };
    assert_eq!((issue.as_str(), stage), ("a", Stage::Done));
    assert_eq!(busy, ["shell: sleep"]);
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
