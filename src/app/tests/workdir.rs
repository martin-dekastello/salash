//! Claude's argv, start directories and worktrees.

use super::super::spawn::{claude_argv, Workdir};
use super::*;

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn claude_gets_hooks_plugin_and_mcp_before_its_own_args() {
    let (s, p, m) = (Path::new("/d/claude-hooks.json"), Path::new("/d/plugin"), Path::new("/d/claude-mcp.json"));
    let first = claude_argv(&strs(&["claude"]), Some(s), Some(p), Some(m), None, strs(&["--permission-mode", "plan", "brief"]));
    assert_eq!(
        first,
        strs(&[
            "claude",
            "--settings",
            "/d/claude-hooks.json",
            "--plugin-dir",
            "/d/plugin",
            "--mcp-config=/d/claude-mcp.json",
            "--permission-mode",
            "plan",
            "brief"
        ])
    );
    let resumed = claude_argv(&strs(&["claude"]), Some(s), Some(p), Some(m), Some("sid"), strs(&["ignored"]));
    assert_eq!(
        resumed,
        strs(&["claude", "--settings", "/d/claude-hooks.json", "--plugin-dir", "/d/plugin", "--mcp-config=/d/claude-mcp.json", "--resume", "sid"])
    );
}

#[test]
fn without_injection_claude_gets_neither() {
    let v = claude_argv(&strs(&["npx", "claude"]), None, None, None, Some("sid"), vec![]);
    assert_eq!(v, strs(&["npx", "claude", "--resume", "sid"]));
}

#[test]
fn resolve_dir_accepts_only_directories() {
    let dir = temp_dir();
    let file = dir.join("file");
    std::fs::write(&file, "").unwrap();

    let got = resolve_dir(dir.to_str().unwrap()).unwrap();
    assert!(got.is_absolute() && got.is_dir());
    assert_eq!(resolve_dir(file.to_str().unwrap()), None);
    assert_eq!(resolve_dir(dir.join("missing").to_str().unwrap()), None);
    assert_eq!(resolve_dir("~"), dirs::home_dir().and_then(|h| std::fs::canonicalize(h).ok()));

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_deleted_default_falls_back_to_the_folder() {
    let dir = temp_dir();
    let folder = Path::new("/srv/team");

    assert_eq!(usable_default(Some(&dir), folder), dir);
    assert_eq!(usable_default(None, folder), folder);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(usable_default(Some(&dir), folder), folder);
}

#[test]
fn only_a_claude_tab_with_a_session_is_suspended() {
    let mut t = tab(ClaudeStatus::Exited, 0, None);
    assert!(!t.suspended());
    t.st.claude_session = Some("s".into());
    assert!(t.suspended());
    t.st.kind = TabKind::Shell;
    assert!(!t.suspended());
}

/// One issue "a" with a Claude tab "t" that has no directory yet. State is saved to a
/// temp dir, never to the `$SALASH_DATA` of the salash these tests may run in.
fn worktree_app(cfg: Config, note: Note) -> App {
    crate::config::set_data_dir(std::env::temp_dir().join(format!("salash-test-data-{}", std::process::id())));
    let vault = Vault { root: PathBuf::from("/nonexistent") };
    let mut app = App::with_parts(cfg, PathBuf::from("/"), vault, vec![note], State::default(), None, None, None);
    let mut t = tab(ClaudeStatus::Idle, 0, None);
    t.st.id = "t".into();
    app.rt.insert("a".into(), IssueRt { tabs: vec![t], ..Default::default() });
    app
}

fn github_note(repo: &str) -> Note {
    Note { source: "github".into(), repo: Some(repo.into()), ..note("a") }
}

fn workdir_of(app: &mut App) -> Workdir {
    let n = app.notes[0].clone();
    app.workdir(&n, "t")
}

#[test]
fn with_worktrees_off_a_tab_starts_in_the_repo() {
    let repo = temp_dir();
    let cfg = Config { worktrees: false, local_repo: Some(repo.clone()), ..Config::default() };
    let mut app = worktree_app(cfg, note("a"));
    assert!(matches!(workdir_of(&mut app), Workdir::Repo(p) if p == repo));
    assert!(app.rt["a"].creating.is_none());
    std::fs::remove_dir_all(repo).ok();
}

#[test]
fn with_worktrees_off_the_notes_repo_path_wins() {
    let (local, own) = (temp_dir(), temp_dir());
    let cfg = Config { worktrees: false, local_repo: Some(local.clone()), ..Config::default() };
    let mut app = worktree_app(cfg, Note { repo_path: Some(own.clone()), ..note("a") });
    assert!(matches!(workdir_of(&mut app), Workdir::Repo(p) if p == own));
    std::fs::remove_dir_all(local).ok();
    std::fs::remove_dir_all(own).ok();
}

#[test]
fn with_worktrees_off_an_issue_with_no_repo_still_has_none() {
    let cfg = Config { worktrees: false, ..Config::default() };
    let mut app = worktree_app(cfg, note("a"));
    assert!(matches!(workdir_of(&mut app), Workdir::NoRepo));
}

#[test]
fn a_source_can_turn_worktrees_off_for_its_issues_only() {
    let repo = temp_dir();
    let cfg: Config = toml::from_str(&format!(
        r#"
        [[github]]
        repo = "a/off"
        path = "{0}"
        worktrees = false
        [[github]]
        repo = "a/on"
        path = "{0}"
        "#,
        repo.display()
    ))
    .unwrap();
    let mut off = worktree_app(cfg.clone(), github_note("a/off"));
    assert!(matches!(workdir_of(&mut off), Workdir::Repo(p) if p == repo));
    // Not a git repo, so the worktree the other source asks for fails in the background.
    let mut on = worktree_app(cfg, github_note("a/on"));
    assert!(matches!(workdir_of(&mut on), Workdir::Pending));
    assert!(on.rt["a"].creating.is_some());
}

#[test]
fn a_worktree_made_before_keeps_being_used_with_worktrees_off() {
    let (repo, wt) = (temp_dir(), temp_dir());
    let cfg = Config { worktrees: false, local_repo: Some(repo.clone()), ..Config::default() };
    let mut app = worktree_app(cfg, note("a"));
    app.rt.get_mut("a").unwrap().worktree = Some(wt.clone());
    assert!(matches!(workdir_of(&mut app), Workdir::Ready(p) if p == wt));
    std::fs::remove_dir_all(repo).ok();
    std::fs::remove_dir_all(wt).ok();
}

#[test]
fn a_tab_started_in_the_repo_keeps_it_for_resume() {
    let repo = temp_dir();
    let cfg = Config {
        worktrees: false,
        local_repo: Some(repo.clone()),
        claude_command: vec!["/nonexistent/claude".into()],
        ..Config::default()
    };
    let mut app = worktree_app(cfg, note("a"));
    app.spawn_pane("a", 0);
    assert_eq!(app.rt["a"].tabs[0].st.cwd.as_deref(), Some(repo.as_path()));
    // Saved before the process starts, so a resume after turning worktrees on finds it.
    assert_eq!(app.state.issues["a"].tabs[0].cwd.as_deref(), Some(repo.as_path()));
    app.cfg.worktrees = true;
    assert!(matches!(workdir_of(&mut app), Workdir::Pending));
    let mut again = worktree_app(app.cfg.clone(), note("a"));
    again.rt.get_mut("a").unwrap().tabs[0].st.cwd = app.state.issues["a"].tabs[0].cwd.clone();
    again.spawn_pane("a", 0);
    assert!(again.rt["a"].creating.is_none(), "a tab with a directory never asks for a worktree");
    std::fs::remove_dir_all(repo).ok();
}
