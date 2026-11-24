//! Resuming sessions by id and the note's resume lines.

use super::*;

#[test]
fn resuming_an_open_session_shows_its_tab() {
    let (mut app, _vault) = click_app(&["a"]);
    with_tabs(&mut app, "a", vec![fresh(TabKind::Shell, None, None), fresh(TabKind::Claude, Some(S1), None)], 0);
    app.select_id("a".into());
    resume(&mut app, &format!("claude --resume {S1}"));
    assert_eq!(app.rt["a"].tabs.len(), 2, "no second tab");
    assert_eq!(app.rt["a"].active, 1);
    assert_eq!(app.focus, Focus::Terminal);
    assert!(app.rt["a"].tabs[1].pane.is_none(), "shown, not started");
}

#[test]
fn resuming_a_closed_session_brings_its_tab_back() {
    let (mut app, _vault) = click_app(&["a"]);
    with_tabs(&mut app, "a", vec![], 0);
    let st = fresh(TabKind::Claude, Some(S1), Some("/nonexistent/dir")).st;
    app.rt.get_mut("a").unwrap().closed.push(st.clone());
    app.select_id("a".into());
    resume(&mut app, S1);
    let rt = &app.rt["a"];
    assert!(rt.closed.is_empty());
    assert_eq!(rt.tabs.len(), 1);
    assert_eq!(rt.tabs[0].st.id, st.id);
}

#[test]
fn resuming_by_id_opens_a_claude_tab_and_starts_work() {
    let (mut app, vault) = click_app(&["a"]);
    // "claude" records its arguments.
    let argv = vault.0.join("argv");
    app.cfg.claude_command = vec!["sh".into(), "-c".into(), format!("echo \"$@\" > '{}'", argv.display()), "sh".into()];
    app.vault.move_to(&note_of(&app, "a"), Stage::Todo).unwrap();
    app.rescan();
    app.set_view(Stage::Todo);
    app.select_id("a".into());

    resume(&mut app, "not an id");
    assert!(flashed(&app).starts_with("Not a session ID"));
    assert!(app.mode == Mode::Prompt(PromptKind::ResumeId, "not an id".into()), "the prompt stays to fix it");
    assert!(app.rt["a"].tabs.is_empty());
    app.mode = Mode::Normal;

    resume(&mut app, S2);
    assert_eq!(note_of(&app, "a").stage, Stage::Active, "like c, it starts work");
    let t = &app.rt["a"].tabs[0];
    assert_eq!(t.st.kind, TabKind::Claude);
    assert_eq!(t.st.claude_session, None, "set by its first hook, which writes the Sessions line");
    assert_eq!(t.st.cwd.as_deref(), Some(app.default_dir().as_path()), "where c starts, not home");
    assert_eq!(app.focus, Focus::Terminal);
    let start = Instant::now();
    while !std::fs::read_to_string(&argv).is_ok_and(|a| a.trim_end().ends_with(&format!("--resume {S2}"))) {
        assert!(start.elapsed() < Duration::from_secs(5), "{:?}", std::fs::read_to_string(&argv));
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn scratch_can_resume_a_session() {
    let (mut app, _vault) = click_app(&["a"]);
    app.select_id(SCRATCH.into());
    key(&mut app, 'I');
    assert!(app.mode == Mode::Prompt(PromptKind::ResumeId, String::new()));
    app.mode = Mode::Normal;
    resume(&mut app, S1);
    assert_eq!(app.rt[SCRATCH].tabs.len(), 1);
    assert_eq!(app.rt[SCRATCH].tabs[0].st.kind, TabKind::Claude);
}

#[test]
fn a_shell_word_is_quoted_only_when_it_must_be() {
    assert_eq!(shell_word("/a/b-c_d.e"), "/a/b-c_d.e");
    assert_eq!(shell_word("/a b"), "'/a b'");
    assert_eq!(shell_word("/it's"), r"'/it'\''s'");
    // A newline would end the note's line; the shell reads the escape back.
    assert_eq!(shell_word("/a\nb\t'c\\"), r"$'/a\x0ab\x09\'c\\'");
    assert!(!shell_word("/a\nb").contains('\n'));
}

#[test]
fn the_note_line_uses_the_configured_claude_command() {
    let (mut app, _vault) = click_app(&["a"]);
    app.cfg.claude_command = vec!["npx".into(), "claude code".into()];
    with_tabs(&mut app, "a", vec![fresh(TabKind::Claude, Some(S1), Some("/w"))], 0);
    app.select_id("a".into());
    key(&mut app, 'd');
    assert!(note_text(&app, "a").contains(&format!("`cd /w && npx 'claude code' --resume {S1}`")), "{}", note_text(&app, "a"));
}

#[test]
fn a_busy_shell_is_named_by_its_command() {
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
    app.select_id("a".into());
    key(&mut app, 'd');
    let Mode::Confirm(ConfirmKind::Retire { busy, .. }) = app.mode.clone() else { panic!("no confirm") };
    assert_eq!(busy, ["shell: sleep"]);
}

#[test]
fn a_closed_session_whose_transcript_is_gone_leaves_no_line() {
    let (mut app, vault) = click_app(&["a"]);
    let t = fresh(TabKind::Claude, Some(S1), None);
    let sf = status::StatusFile {
        status: ClaudeStatus::Idle,
        session_id: Some(S1.into()),
        transcript: Some(vault.0.join("gone.jsonl").to_string_lossy().into_owned()),
        message: None,
        event: "Stop".into(),
        ts: 0,
    };
    std::fs::create_dir_all(status::status_dir()).unwrap();
    std::fs::write(status::status_path(&t.st.id), serde_json::to_vec(&sf).unwrap()).unwrap();
    with_tabs(&mut app, "a", vec![t], 0);
    app.select_id("a".into());
    assert!(!app.close_tab_at("a", 0), "nothing to reopen");
    assert!(app.rt["a"].closed.is_empty());
    key(&mut app, 'd');
    assert!(!note_text(&app, "a").contains("--resume"));
}

#[test]
fn a_session_open_in_another_issue_is_shown_there() {
    let (mut app, _vault) = click_app(&["a", "b"]);
    with_tabs(&mut app, "a", vec![], 0);
    app.state.issues.insert("b".into(), IssueState { tabs: vec![fresh(TabKind::Shell, None, None).st, fresh(TabKind::Claude, Some(S1), None).st], ..Default::default() });
    app.select_id("a".into());
    resume(&mut app, S1);
    assert_eq!(app.selected.as_deref(), Some("b"));
    assert_eq!(app.rt["b"].active, 1);
    assert!(app.rt["a"].tabs.is_empty(), "no second process on the session");
    assert_eq!(flashed(&app), "That session is open in b");
}

#[test]
fn resuming_twice_before_the_first_hook_opens_one_tab() {
    let (mut app, vault) = click_app(&["a"]);
    app.cfg.claude_command = vec!["sh".into(), "-c".into(), "sleep 30".into(), "sh".into()];
    app.select_id("a".into());
    let line = format!("`cd {} && claude --resume {S1}`", shell_word(&vault.0.to_string_lossy()));
    resume(&mut app, &line);
    assert_eq!(app.rt["a"].tabs[0].st.cwd.as_deref(), Some(vault.0.as_path()), "where the pasted cd goes");
    resume(&mut app, S1);
    assert_eq!(app.rt["a"].tabs.len(), 1);
    assert_eq!(flashed(&app), "That session is open in this tab");
}
