use super::*;

mod click;
mod render;
mod resume;
mod retire;
mod seen;
mod sort;
mod workdir;

pub(super) fn tab(status: ClaudeStatus, status_ts: i64, esc_ts: Option<i64>) -> Tab {
    let st = TabState {
        id: "t".into(),
        name: "claude".into(),
        kind: TabKind::Claude,
        custom_name: false,
        claude_session: None,
        cwd: None,
    };
    Tab { status, status_ts, esc_ts, ..Tab::new(st) }
}

pub(super) fn note(id: &str) -> Note {
    Note {
        id: id.into(),
        title: id.into(),
        stage: Stage::Active,
        path: PathBuf::new(),
        source: "local".into(),
        url: None,
        repo: None,
        repo_path: None,
        created: None,
        moved: None,
    }
}

/// Issues "a" and "b", each with Done tabs named by `tabs`, "a" selected.
/// Built in memory: no vault, state file or sync.
pub(super) fn app(tabs: &[(&str, &[&str])]) -> App {
    let notes = tabs.iter().map(|(id, _)| note(id)).collect();
    let vault = Vault { root: PathBuf::from("/nonexistent") };
    let mut app = App::with_parts(Config::default(), PathBuf::from("/"), vault, notes, State::default(), None, None, None);
    for (id, ids) in tabs {
        let tabs = ids
            .iter()
            .map(|t| {
                let mut tab = tab(ClaudeStatus::Done, 1, None);
                tab.st.id = (*t).into();
                tab
            })
            .collect();
        app.rt.insert((*id).into(), IssueRt { tabs, ..Default::default() });
    }
    app.selected = Some(tabs[0].0.into());
    app
}

pub(super) fn shown(app: &App, issue: &str, idx: usize) -> ClaudeStatus {
    app.rt[issue].tabs[idx].shown_status()
}

/// Never run on a click: if one ever spawned, it would not be Claude.
pub(super) const MUST_NOT_SPAWN: &str = "/nonexistent/salash-test-must-not-spawn";

/// A vault dir removed on drop, even when an assertion fails first.
pub(super) struct TempVault(PathBuf);

impl Drop for TempVault {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// A real App over a fresh vault holding `issues` in Active. Keep the
/// returned guard alive for as long as the App.
pub(super) fn click_app(issues: &[&str]) -> (App, TempVault) {
    // The data dir (state.json, snapshots, status files) is set once per
    // test binary. A fixed dir under target/ keeps it away from the real
    // instance without leaking a new dir into /tmp on every run. Tests
    // share it, so they assert on in-memory state only.
    crate::config::set_data_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("target/salash-unit-data"));
    let vault = TempVault(std::env::temp_dir().join(format!("salash-test-{}", uuid::Uuid::new_v4())));
    let cfg = Config {
        vault: vault.0.clone(),
        inject_hooks: false,
        claude_command: vec![MUST_NOT_SPAWN.into()],
        shell_command: vec![MUST_NOT_SPAWN.into()],
        ..Default::default()
    };
    let mut app = App::new(cfg, false, vault.0.clone()).unwrap();
    // Another test may have saved state in the shared dir; start clean.
    app.state = State::default();
    app.rt.clear();
    for id in issues {
        let note = NewNote { id, title: id, source: "local", url: None, repo: None, stage: Stage::Active };
        app.vault.create(note).unwrap();
    }
    app.rescan();
    app.view = Stage::Active;
    app.selected = None;
    app.msg = None;
    (app, vault)
}

pub(super) fn with_tabs(app: &mut App, id: &str, tabs: Vec<Tab>, active: usize) {
    app.rt.insert(id.to_string(), IssueRt { tabs, active, ..Default::default() });
}

/// A Claude tab with no pane and no session: never started.
pub(super) fn unstarted_tab() -> Tab {
    tab(ClaudeStatus::Exited, 0, None)
}

/// A tab whose pane runs `sleep`; dropping the pane kills it.
pub(super) fn running_tab(dir: &Path) -> Tab {
    let argv = ["sh".to_string(), "-c".to_string(), "sleep 30".to_string()];
    let mut t = unstarted_tab();
    t.pane = Some(Pane::spawn(&argv, dir, &[], 5, 40).unwrap());
    t
}

/// Lay the screen out so the mouse hit rects are where the real UI draws them.
pub(super) fn draw(app: &mut App) {
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    term.draw(|f| crate::ui::draw(app, f)).unwrap();
}

pub(super) fn mouse(app: &mut App, kind: MouseEventKind, pos: Position) {
    app.handle(Event::Mouse(MouseEvent { kind, column: pos.x, row: pos.y, modifiers: KeyModifiers::NONE }));
}

/// Left-click the issue's row.
pub(super) fn click(app: &mut App, id: &str) {
    draw(app);
    let r = app.item_hits.iter().find(|(_, i)| i == id).map(|(r, _)| *r).expect("issue row drawn");
    mouse(app, MouseEventKind::Down(MouseButton::Left), Position::new(r.x, r.y));
}

pub(super) fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("salash-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(super) fn row(buf: &ratatui::buffer::Buffer, y: u16, from: u16) -> String {
    (from..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

pub(super) const S1: &str = "11111111-1111-4111-8111-111111111111";
pub(super) const S2: &str = "22222222-2222-4222-8222-222222222222";

/// A tab of `kind` with a fresh id, so its status file and snapshot are its own.
pub(super) fn fresh(kind: TabKind, session: Option<&str>, cwd: Option<&str>) -> Tab {
    let mut t = unstarted_tab();
    t.st.id = uuid::Uuid::new_v4().to_string();
    t.st.kind = kind;
    t.st.claude_session = session.map(Into::into);
    t.st.cwd = cwd.map(PathBuf::from);
    t
}

/// A tab of `kind` whose pane runs `sleep` with no command in front of it.
pub(super) fn live(kind: TabKind, dir: &Path) -> Tab {
    let mut t = running_tab(dir);
    t.st.id = uuid::Uuid::new_v4().to_string();
    t.st.kind = kind;
    t
}

pub(super) fn key(app: &mut App, c: char) {
    app.on_sidebar_key(KeyEvent::from(KeyCode::Char(c)));
}

pub(super) fn note_of(app: &App, id: &str) -> Note {
    app.notes.iter().find(|n| n.id == id).cloned().expect("note")
}

pub(super) fn note_text(app: &App, id: &str) -> String {
    std::fs::read_to_string(&note_of(app, id).path).unwrap()
}

pub(super) fn flashed(app: &App) -> String {
    app.msg.as_ref().map(|m| m.0.clone()).unwrap_or_default()
}

pub(super) fn resume(app: &mut App, input: &str) {
    app.mode = Mode::Prompt(PromptKind::ResumeId, String::new());
    app.on_paste(input.into());
    app.handle(Event::Key(KeyEvent::from(KeyCode::Enter)));
}
