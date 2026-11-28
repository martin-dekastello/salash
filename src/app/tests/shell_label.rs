//! Shell tabs show their foreground command, settled for a second (#47).

use super::super::types::{is_shell, reading, CmdLabel, CmdState};
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
    assert_eq!(l.label(), None, "an exec'd shell at its prompt is idle");
}

#[test]
fn a_wrapper_that_execs_a_shell_stays_idle() {
    // `sh -c 'cd x && exec zsh'`, or a .bashrc ending in `exec fish`: the own
    // name changes before any command runs, and it's still a shell.
    let t0 = Instant::now();
    let mut l = CmdLabel::default();
    l.observe((100, Some("sh".into())), None, t0);
    for ms in (250..=3000).step_by(250) {
        l.observe((100, Some("zsh".into())), None, t0 + MS(ms));
        assert_eq!(l.label(), None, "{ms}ms");
    }
}

#[test]
fn a_login_shell_and_the_configured_shell_count_as_shells() {
    assert!(is_shell("-zsh", None));
    assert!(!is_shell("vim", None));
    assert!(is_shell("vim", Some("vim")), "shell_command = [\"vim\"] is the tab's shell");
    let t0 = Instant::now();
    let mut l = CmdLabel::for_shell("/usr/bin/vim");
    l.observe((100, Some("vim".into())), None, t0);
    l.observe((100, Some("vim".into())), None, t0 + MS(2000));
    assert_eq!(l.label(), None);
}

#[test]
fn an_unreadable_own_name_counts_as_idle() {
    assert_eq!(reading((100, None), None, None), CmdState::Idle);
}

#[test]
fn an_unreadable_job_still_shows_an_exec_d_program() {
    // `sudo …` under an exec'd vim, where the job's name can't be read.
    assert_eq!(reading((100, Some("vim".into())), Some((200, None)), None), CmdState::Job(100, "vim".into()));
    assert_eq!(reading(sh(), Some((200, None)), None), CmdState::Idle);
}

#[test]
fn reset_waits_a_full_second() {
    let (mut l, t0) = fed(&[(0, job(200, "vim")), (1000, job(200, "vim"))]);
    assert_eq!(l.label(), Some("vim"));
    l.reset(t0 + MS(1100));
    assert_eq!(l.label(), None);
    l.observe(sh(), job(200, "vim"), t0 + MS(1200));
    l.observe(sh(), job(200, "vim"), t0 + MS(2100));
    assert_eq!(l.label(), None, "900ms after the reset");
    l.observe(sh(), job(200, "vim"), t0 + MS(2200));
    assert_eq!(l.label(), Some("vim"));
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
fn renaming_a_shell_to_shell_clears_the_name() {
    let mut app = shell_app(TabKind::Shell, "shell");
    app.rename_tab("shell".into());
    assert!(!tab0(&mut app).st.custom_name, "`,` Enter on the prefilled default");
}

#[test]
fn an_exec_d_program_counts_as_busy() {
    // The tab's own process is `sleep`: not a shell, so it's an exec'd program.
    let mut app = shell_app(TabKind::Shell, "shell");
    let own = tab0(&mut app).pane.as_ref().unwrap().child_pid().unwrap();
    wait(|| term::process_name(own).as_deref() == Some("sleep"));
    assert_eq!(tab0(&mut app).busy_label().as_deref(), Some("shell: sleep"));
    tab0(&mut app).cmd = CmdLabel::for_shell("sleep");
    assert_eq!(tab0(&mut app).busy_label(), None, "the configured shell at its prompt");
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
