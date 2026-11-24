//! #28: Done and Archive stop their tabs.

use super::*;

fn answer(app: &mut App, c: char) {
    app.handle(Event::Key(KeyEvent::from(KeyCode::Char(c))));
}

#[test]
fn done_stops_idle_tabs_and_keeps_resume_commands_in_the_note() {
    let (mut app, vault) = click_app(&["a", "b"]);
    app.cfg.claude_command = vec!["claude".into()];
    let suspended = fresh(TabKind::Claude, Some(S1), Some("/work/my repo"));
    let no_session = fresh(TabKind::Claude, None, None);
    let shell = live(TabKind::Shell, &vault.0);
    let shell_pid = shell.pane.as_ref().unwrap().pid().unwrap();
    let ids: Vec<String> = [&suspended, &no_session, &shell].iter().map(|t| t.st.id.clone()).collect();
    with_tabs(&mut app, "a", vec![suspended, no_session, shell], 2);
    let closed_id = uuid::Uuid::new_v4().to_string();
    {
        let rt = app.rt.get_mut("a").unwrap();
        rt.worktree = Some(PathBuf::from("/wt/a"));
        let mut st = fresh(TabKind::Claude, Some(S2), None).st;
        st.id = closed_id.clone();
        st.name = "old".into();
        rt.closed.push(st);
        // The same session closed twice is written once.
        rt.closed.push(fresh(TabKind::Claude, Some(S1), None).st);
    }
    state::write_snapshot(&ids[0], b"screen");
    state::write_snapshot(&closed_id, b"screen");
    app.select_id("a".into());

    key(&mut app, 'd');
    assert!(app.mode == Mode::Normal, "idle tabs need no confirmation");
    assert_eq!(note_of(&app, "a").stage, Stage::Done);
    assert_eq!(flashed(&app), "a → Done · 3 tabs stopped");
    assert!(app.rt["a"].tabs.is_empty() && app.rt["a"].closed.is_empty());
    let saved = &app.state.issues["a"];
    assert!(saved.tabs.is_empty() && saved.closed.is_empty(), "persisted, so they don't come back");
    assert_eq!(saved.worktree.as_deref(), Some(Path::new("/wt/a")), "the worktree stays");
    assert!(!state::snapshot_path(&ids[0]).exists() && !state::snapshot_path(&closed_id).exists());
    // The shell is gone: kill(0) on a reaped pid fails.
    assert_ne!(unsafe { libc::kill(shell_pid as i32, 0) }, 0, "shell still running");

    let text = note_text(&app, "a");
    assert!(text.contains(&format!(" · stopped · claude · `cd '/work/my repo' && claude --resume {S1}`")), "{text}");
    assert!(text.contains(&format!(" · stopped · old · `cd /wt/a && claude --resume {S2}`")), "{text}");
    assert_eq!(text.matches("--resume").count(), 2, "one line per session, none without one: {text}");
}

#[test]
fn busy_tabs_are_named_and_no_leaves_everything_as_it_was() {
    let (mut app, vault) = click_app(&["a"]);
    let mut claude = live(TabKind::Claude, &vault.0);
    claude.status = ClaudeStatus::Working;
    let editor = live(TabKind::Notes, &vault.0);
    let idle = fresh(TabKind::Claude, None, None);
    with_tabs(&mut app, "a", vec![claude, editor, idle], 0);
    app.select_id("a".into());

    key(&mut app, 'd');
    let Mode::Confirm(ConfirmKind::Retire { issue, stage, busy }) = app.mode.clone() else { panic!("no confirm") };
    assert_eq!((issue.as_str(), stage), ("a", Stage::Done));
    assert_eq!(busy, ["Claude: working", "note editor"]);
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    term.draw(|f| crate::ui::draw(&mut app, f)).unwrap();
    let screen: String = term.backend().buffer().content().iter().map(|c| c.symbol()).collect();
    assert!(screen.contains("Stop 2 busy tabs (Claude: working, note"), "{screen}");

    answer(&mut app, 'n');
    assert!(app.mode == Mode::Normal);
    assert_eq!(note_of(&app, "a").stage, Stage::Active);
    assert_eq!(app.rt["a"].tabs.len(), 3);
    assert!(app.rt["a"].tabs[0].running());

    key(&mut app, 'a');
    assert!(matches!(app.mode, Mode::Confirm(ConfirmKind::Retire { stage: Stage::Archive, .. })));
    answer(&mut app, 'y');
    assert_eq!(note_of(&app, "a").stage, Stage::Archive);
    assert!(app.rt["a"].tabs.is_empty());
    assert_eq!(flashed(&app), "a → Archive · 3 tabs stopped");
}

#[test]
fn a_failed_move_stops_nothing() {
    let (mut app, vault) = click_app(&["a"]);
    with_tabs(&mut app, "a", vec![fresh(TabKind::Claude, Some(S1), None)], 0);
    app.select_id("a".into());
    let path = note_of(&app, "a").path;
    let done = path.parent().unwrap().parent().unwrap().join(Stage::Done.dir());
    assert!(done.starts_with(&vault.0));
    std::fs::create_dir_all(&done).unwrap();
    std::fs::write(done.join(path.file_name().unwrap()), "taken").unwrap();

    key(&mut app, 'd');
    assert!(flashed(&app).starts_with("Move failed"), "{}", flashed(&app));
    assert_eq!(app.rt["a"].tabs.len(), 1);
    assert!(!note_text(&app, "a").contains("stopped"));
}

#[test]
fn moving_back_brings_no_tabs_and_done_to_archive_has_none_to_stop() {
    let (mut app, _vault) = click_app(&["a"]);
    with_tabs(&mut app, "a", vec![fresh(TabKind::Claude, Some(S1), None)], 0);
    app.select_id("a".into());
    key(&mut app, 'd');
    app.set_view(Stage::Done);
    app.select_id("a".into());
    key(&mut app, 'a');
    assert_eq!(flashed(&app), "a → Archive", "no tabs left to stop");
    app.set_view(Stage::Archive);
    app.select_id("a".into());
    key(&mut app, 'u');
    assert_eq!(note_of(&app, "a").stage, Stage::Active);
    assert!(app.rt["a"].tabs.is_empty() && app.rt["a"].closed.is_empty());
}

#[test]
fn a_move_made_outside_salash_stops_idle_tabs_and_leaves_busy_ones() {
    let (mut app, vault) = click_app(&["a", "b"]);
    let tabs = vec![fresh(TabKind::Claude, Some(S1), None), live(TabKind::Shell, &vault.0), live(TabKind::Notes, &vault.0)];
    with_tabs(&mut app, "a", tabs, 0);
    // "b" was never loaded: its tabs are only saved, as after a restart.
    app.state.issues.insert("b".into(), IssueState { tabs: vec![fresh(TabKind::Shell, None, None).st], ..Default::default() });
    for id in ["a", "b"] {
        app.vault.move_to(&note_of(&app, id), Stage::Done).unwrap();
    }

    app.rescan();
    let rt = &app.rt["a"];
    assert_eq!(rt.tabs.len(), 1, "only the editor is left");
    assert_eq!(rt.tabs[0].st.kind, TabKind::Notes);
    // The editor would save over a line written under it, so the session waits in closed.
    assert!(!note_text(&app, "a").contains("--resume"));
    assert_eq!(rt.closed.iter().map(|st| st.claude_session.as_deref()).collect::<Vec<_>>(), [Some(S1)]);
    assert!(app.state.issues["b"].tabs.is_empty(), "saved leftovers go too");
    assert_eq!(app.busy_left["a"], ["note editor"]);
    let m = flashed(&app);
    assert!(m.contains("a is in Done: left 1 busy tab running (note editor), stopped once idle") && m.contains("b is in Done: 1 tab stopped"), "{m}");

    // Busy tabs are retried every rescan, but said only once.
    app.msg = None;
    app.rescan();
    assert!(app.msg.is_none(), "flashed again: {}", flashed(&app));
    assert_eq!(app.rt["a"].tabs.len(), 1);

    // Once the editor quits, the next rescan stops it and writes the line.
    let p = app.rt.get_mut("a").unwrap().tabs[0].pane.as_mut().unwrap();
    p.kill();
    let start = Instant::now();
    while !p.poll_exit() {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    app.rescan();
    assert!(app.rt["a"].tabs.is_empty() && app.rt["a"].closed.is_empty());
    assert_eq!(flashed(&app), "a is in Done: 1 tab stopped");
    assert_eq!(note_text(&app, "a").matches(&format!("--resume {S1}")).count(), 1);
    assert!(app.busy_left.is_empty());
}

#[test]
fn a_note_that_cant_be_written_keeps_the_sessions() {
    use std::os::unix::fs::PermissionsExt;
    let (mut app, vault) = click_app(&["a"]);
    let mut claude = live(TabKind::Claude, &vault.0);
    claude.st.claude_session = Some(S1.into());
    let tab_id = claude.st.id.clone();
    with_tabs(&mut app, "a", vec![claude], 0);
    app.select_id("a".into());
    let path = note_of(&app, "a").path;
    // The move renames the file and rewrites it first, so lock it after.
    app.vault.move_to(&note_of(&app, "a"), Stage::Done).unwrap();
    app.notes = app.vault.scan();
    let path = path.parent().unwrap().parent().unwrap().join(Stage::Done.dir()).join(path.file_name().unwrap());
    let mode = |m| std::fs::set_permissions(&path, std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o444);

    app.rescan();
    let rt = &app.rt["a"];
    assert!(rt.tabs.is_empty(), "stopped all the same");
    assert_eq!(rt.closed.len(), 1, "kept for R");
    assert_eq!(rt.closed[0].id, tab_id);
    assert_eq!(app.state.issues["a"].closed.len(), 1, "persisted");
    assert!(state::snapshot_path(&tab_id).exists(), "its screen is kept");
    assert!(app.msg_error && flashed(&app).contains("not written"), "{}", flashed(&app));

    // Retried every rescan, said once.
    app.msg = None;
    app.rescan();
    assert!(app.msg.is_none(), "flashed again: {}", flashed(&app));

    mode(0o644);
    app.rescan();
    assert!(app.rt["a"].closed.is_empty());
    assert!(note_text(&app, "a").contains(&format!("--resume {S1}")));
    assert!(!state::snapshot_path(&tab_id).exists());
}

#[test]
fn a_rescan_leaves_a_tab_waiting_for_its_worktree_focused() {
    let (mut app, _vault) = click_app(&["a"]);
    let t = fresh(TabKind::Claude, None, None);
    let (_tx, rx) = mpsc::channel();
    let waiting = vec![t.st.id.clone()];
    app.rt.insert("a".into(), IssueRt { tabs: vec![t], creating: Some(WorktreeJob { rx, waiting, fallback: None }), ..Default::default() });
    app.select_id("a".into());
    app.focus = Focus::Terminal;
    app.rescan();
    assert_eq!(app.focus, Focus::Terminal);
}

#[test]
fn nothing_new_opens_on_a_finished_issue() {
    let (mut app, _vault) = click_app(&["a"]);
    app.vault.move_to(&note_of(&app, "a"), Stage::Done).unwrap();
    app.rescan();
    app.set_view(Stage::Done);
    app.select_id("a".into());
    for c in ['c', 'C', 't', 'e', 'I', 'R'] {
        app.msg = None;
        key(&mut app, c);
        assert_eq!(flashed(&app), "Move to Active first (u)", "key {c}");
        assert!(app.mode == Mode::Normal, "key {c}");
    }
    app.open_issue();
    assert!(app.rt["a"].tabs.is_empty());
    let req = Request { issue: "a".into(), kind: TabKind::Shell, cwd: None, name: None, mode: None, prompt: None };
    let reply = app.open_requested(req);
    assert!(!reply.ok && reply.error.unwrap().contains("Move to Active first"));
    // The note and the issue's page still open.
    key(&mut app, 'o');
    assert!(matches!(app.outbox.last(), Some(ServerMsg::Open(u)) if u.starts_with("obsidian://")));
}

#[test]
fn a_session_id_is_read_from_what_was_pasted() {
    let id = S1;
    for ok in [
        id.to_string(),
        format!("  {id}\t"),
        id.to_uppercase(),
        format!("claude --resume {id}"),
        format!("`cd /x && claude --resume {id}`"),
        format!("- 2026-11-19 10:00 · stopped · claude · `cd /x && claude --resume {id}`"),
    ] {
        assert_eq!(parse_session_id(&ok).map(|(s, _)| s).as_deref(), Some(id), "{ok}");
    }
    for bad in ["", "   ", "hello", "11111111111141118111111111111111", "claude --resume", &format!("{id} {id}"), "{11111111-1111-4111-8111-111111111111}"] {
        assert_eq!(parse_session_id(bad), None, "{bad}");
    }
    let dir = |input: &str| parse_session_id(input).and_then(|(_, d)| d);
    assert_eq!(dir(S1), None);
    assert_eq!(dir(&format!("claude --resume {S1}")), None);
    assert_eq!(dir(&format!("- x · stopped · claude · `cd /wt/a && claude --resume {S1}`")), Some(PathBuf::from("/wt/a")));
    assert_eq!(dir(&format!("`cd '/my dir/it'\\''s' && claude --resume {S1}`")), Some(PathBuf::from("/my dir/it's")));
}
