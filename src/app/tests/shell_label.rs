//! Shell tabs show their foreground command, settled for a second (#47).

use super::super::types::CmdLabel;
use super::*;

const MS: fn(u64) -> Duration = Duration::from_millis;

fn sh() -> (i32, Option<String>) {
    (100, Some("zsh".into()))
}

/// The foreground group and its leader's name, as `observe` takes it.
type Fg = Option<(i32, Option<String>)>;

fn job(pg: i32, name: &str) -> Fg {
    Some((pg, Some(name.into())))
}

/// A label fed `steps` of (ms since start, foreground), with the tab's own
/// process always `sh()`.
fn fed(steps: &[(u64, Fg)]) -> (CmdLabel, Instant) {
    let t0 = Instant::now();
    let mut l = CmdLabel::default();
    for (ms, fg) in steps {
        l.observe(sh(), fg.clone(), t0 + MS(*ms));
    }
    (l, t0)
}

#[test]
fn a_command_shows_once_it_has_run_for_a_second() {
    let (mut l, t0) = fed(&[(0, None), (250, job(200, "vim")), (1000, job(200, "vim"))]);
    assert_eq!(l.label(), None, "750ms in");
    l.observe(sh(), job(200, "vim"), t0 + MS(1250));
    assert_eq!(l.label(), Some("vim"));
}

#[test]
fn back_at_the_prompt_shows_idle_after_a_second() {
    let (mut l, t0) = fed(&[(0, job(200, "vim")), (1000, job(200, "vim")), (2000, None)]);
    assert_eq!(l.label(), Some("vim"), "still shown just after it ends");
    l.observe(sh(), None, t0 + MS(2900));
    assert_eq!(l.label(), Some("vim"));
    l.observe(sh(), None, t0 + MS(3000));
    assert_eq!(l.label(), None);
}

#[test]
fn a_short_command_before_another_never_shows() {
    let (mut l, t0) = fed(&[(0, job(200, "ls")), (500, job(201, "vim"))]);
    let mut seen = vec![];
    for ms in (750..=2000).step_by(250) {
        l.observe(sh(), job(201, "vim"), t0 + MS(ms));
        seen.push(l.label().map(String::from));
    }
    assert!(!seen.contains(&Some("ls".into())));
    assert_eq!(l.label(), Some("vim"));
}

#[test]
fn a_new_name_in_the_same_group_waits_its_own_second() {
    let (mut l, t0) = fed(&[(0, job(200, "npm")), (1000, job(200, "npm")), (1500, job(200, "node"))]);
    l.observe(sh(), job(200, "node"), t0 + MS(2400));
    assert_eq!(l.label(), Some("npm"));
    l.observe(sh(), job(200, "node"), t0 + MS(2500));
    assert_eq!(l.label(), Some("node"));
}

#[test]
fn an_unreadable_name_counts_as_idle() {
    let (l, _) = fed(&[(0, Some((200, None))), (2000, Some((200, None)))]);
    assert_eq!(l.label(), None);
}

#[test]
fn a_script_of_short_commands_keeps_the_last_settled_label() {
    let mut steps = vec![(0, job(200, "make")), (1000, job(200, "make"))];
    // A new short command every 300ms: none of them lasts a second.
    for (i, ms) in (1300..4000).step_by(300).enumerate() {
        steps.push((ms, job(300 + i as i32, "cc")));
    }
    let (l, _) = fed(&steps);
    assert_eq!(l.label(), Some("make"));
}

#[test]
fn exec_shows_the_new_name_after_a_second() {
    let t0 = Instant::now();
    let mut l = CmdLabel::default();
    l.observe(sh(), None, t0);
    l.observe((100, Some("vim".into())), None, t0 + MS(250));
    l.observe((100, Some("vim".into())), None, t0 + MS(1000));
    assert_eq!(l.label(), None);
    l.observe((100, Some("vim".into())), None, t0 + MS(1250));
    assert_eq!(l.label(), Some("vim"));
    // A command run inside the exec'd program shows as usual.
    l.observe((100, Some("bash".into())), job(200, "make"), t0 + MS(2000));
    l.observe((100, Some("bash".into())), job(200, "make"), t0 + MS(3000));
    assert_eq!(l.label(), Some("make"));
    l.observe((100, Some("bash".into())), None, t0 + MS(3250));
    l.observe((100, Some("bash".into())), None, t0 + MS(4250));
    assert_eq!(l.label(), Some("bash"), "at the exec'd shell's prompt");
}

#[test]
fn exec_into_the_same_shell_stays_idle() {
    // `exec zsh` inside zsh: the own process keeps its name.
    let (l, _) = fed(&[(0, None), (1000, None), (2000, None)]);
    assert_eq!(l.label(), None);
}

#[test]
fn no_exec_detection_without_a_readable_baseline() {
    let t0 = Instant::now();
    let mut l = CmdLabel::default();
    l.observe((100, None), None, t0);
    l.observe((100, None), None, t0 + MS(250));
    // The baseline is the first readable name, so a name showing up later is it.
    l.observe((100, Some("vim".into())), None, t0 + MS(500));
    l.observe((100, Some("vim".into())), None, t0 + MS(3000));
    assert_eq!(l.label(), None);
}

#[test]
fn reset_waits_a_full_second_and_keeps_the_exec_baseline() {
    let (mut l, t0) = fed(&[(0, job(200, "vim")), (1000, job(200, "vim"))]);
    assert_eq!(l.label(), Some("vim"));
    l.reset(t0 + MS(1100));
    assert_eq!(l.label(), None);
    l.observe(sh(), job(200, "vim"), t0 + MS(1200));
    l.observe(sh(), job(200, "vim"), t0 + MS(2100));
    assert_eq!(l.label(), None, "900ms after the reset");
    l.observe(sh(), job(200, "vim"), t0 + MS(2200));
    assert_eq!(l.label(), Some("vim"));
    assert_eq!(l.own_name.as_deref(), Some("zsh"));
}

// ---------- the tab ----------

/// A shell tab in issue "a" whose process really runs, so it counts as
/// running; the label is fed by hand.
fn shell_app(kind: TabKind, name: &str) -> App {
    let mut app = app(&[("a", &["t1"])]);
    let t = &mut app.rt.get_mut("a").unwrap().tabs[0];
    t.st.kind = kind;
    t.st.name = name.into();
    let argv = ["sleep".to_string(), "30".to_string()];
    t.pane = Some(Pane::spawn(&argv, &std::env::temp_dir(), &[], 5, 40).unwrap());
    app
}

fn tab0(app: &mut App) -> &mut Tab {
    &mut app.rt.get_mut("a").unwrap().tabs[0]
}

fn show_vim(t: &mut Tab) {
    let t0 = Instant::now() - Duration::from_secs(5);
    t.cmd.observe(sh(), job(200, "vim"), t0);
    t.cmd.observe(sh(), job(200, "vim"), t0 + Duration::from_secs(1));
}

#[test]
fn the_command_is_shown_but_never_stored() {
    let mut app = shell_app(TabKind::Shell, "shell");
    show_vim(tab0(&mut app));
    assert_eq!(tab0(&mut app).shown_name(), "vim");
    assert_eq!(tab0(&mut app).st.name, "shell");
}

#[test]
fn a_custom_name_hides_the_command_and_tracks_nothing() {
    let mut app = shell_app(TabKind::Shell, "build");
    let t = tab0(&mut app);
    t.st.custom_name = true;
    show_vim(t);
    assert_eq!(t.shown_name(), "build");
    t.observe_command(Instant::now());
    assert_eq!(t.cmd.label(), None, "the tick resets it while the name is custom");
}

#[test]
fn other_tabs_never_show_commands() {
    for kind in [TabKind::Claude, TabKind::Notes] {
        let mut app = shell_app(kind, "x");
        show_vim(tab0(&mut app));
        assert_eq!(tab0(&mut app).shown_name(), "x");
    }
}

#[test]
fn an_exited_shell_shows_its_stored_name() {
    let mut app = shell_app(TabKind::Shell, "shell");
    let t = tab0(&mut app);
    show_vim(t);
    t.pane.as_mut().unwrap().kill();
    wait(|| t.pane.as_mut().unwrap().poll_exit());
    assert_eq!(t.shown_name(), "shell");
}

#[test]
fn clearing_a_shell_rename_goes_back_to_shell_and_waits_a_second() {
    let mut app = shell_app(TabKind::Shell, "build");
    tab0(&mut app).st.custom_name = true;
    app.rename_tab(String::new());
    let t = tab0(&mut app);
    assert_eq!((t.st.name.as_str(), t.st.custom_name), ("shell", false));
    let t0 = Instant::now();
    t.cmd.observe(sh(), job(200, "vim"), t0);
    assert_eq!(t.shown_name(), "shell", "vim is running, but not for a second yet");
    t.cmd.observe(sh(), job(200, "vim"), t0 + Duration::from_secs(1));
    assert_eq!(t.shown_name(), "vim");
}

#[test]
fn clearing_a_note_rename_goes_back_to_notes() {
    let mut app = shell_app(TabKind::Notes, "todo");
    tab0(&mut app).st.custom_name = true;
    app.rename_tab("  ".into());
    assert_eq!(tab0(&mut app).st.name, "notes");
}

#[test]
fn clearing_a_claude_rename_keeps_the_name() {
    let mut app = shell_app(TabKind::Claude, "fix login");
    tab0(&mut app).st.custom_name = true;
    app.rename_tab(String::new());
    let t = tab0(&mut app);
    assert_eq!((t.st.name.as_str(), t.st.custom_name), ("fix login", false));
}

#[test]
fn the_tab_bar_shows_the_command() {
    let mut app = shell_app(TabKind::Shell, "shell");
    app.view = Stage::Active;
    show_vim(tab0(&mut app));
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 10)).unwrap();
    term.draw(|f| crate::ui::draw(&mut app, f)).unwrap();
    let buf = term.backend().buffer().clone();
    let top: String = (0..100).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    assert!(top.contains("$ 1:vim "), "{top:?}");
}

fn wait(mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < end, "timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}
