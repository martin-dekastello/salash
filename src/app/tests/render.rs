//! Drawing: focus, dimming, dialogs, tiny screens.

use super::*;

/// Draw `app` into a `w`×`h` buffer.
fn render(app: &mut App, w: u16, h: u16) -> ratatui::buffer::Buffer {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::draw(app, f)).unwrap();
    t.backend().buffer().clone()
}

/// Issue `a` has two suspended tabs, so the terminal can take focus; `b` has none.
fn focus_app(focus: Focus) -> App {
    let mut app = app(&[("a", &["t1", "t2"]), ("b", &[])]);
    for t in app.rt.get_mut("a").unwrap().tabs.iter_mut() {
        t.st.name = t.st.id.clone();
        t.st.claude_session = Some("s".into());
    }
    app.view = Stage::Active;
    app.focus = focus;
    app
}

#[test]
fn moving_focus_never_resizes_the_terminal() {
    let mut app = focus_app(Focus::Sidebar);
    render(&mut app, 100, 30);
    let side = app.term_rect;
    app.focus = Focus::Terminal;
    render(&mut app, 100, 30);
    assert_eq!(app.term_rect, side);
    // Tab bar, frame line and status bar.
    assert_eq!(app.term_rect.height, 30 - 3);
    assert_eq!(app.term_rect.y, 2);
}

#[test]
fn the_frame_is_heavy_only_with_the_terminal_focused() {
    // By shape, not colour.
    // With the first tab active the terminal's frame climbs up beside it.
    for (focus, active, top, joint, column, line) in [
        (Focus::Terminal, 0, "┃", "┣", "┃", "━"),
        (Focus::Terminal, 1, "│", "┢", "┃", "━"),
        (Focus::Sidebar, 0, "│", "├", "│", "─"),
    ] {
        let mut app = focus_app(focus);
        app.rt.get_mut("a").unwrap().active = active;
        let buf = render(&mut app, 100, 30);
        let x = app.sidebar_rect.right() - 1;
        assert_eq!(buf[(x, 0)].symbol(), top, "beside the tab bar");
        assert_eq!(buf[(x, 1)].symbol(), joint);
        assert_eq!(buf[(x, 15)].symbol(), column);
        assert_eq!(buf[(x, 29)].symbol(), "│", "beside the status bar stays thin");
        assert_eq!(row(&buf, 1, x + 1), line.repeat((99 - x) as usize));
    }
}

#[test]
fn the_bar_and_the_tab_marker_follow_focus() {
    let mut app = focus_app(Focus::Sidebar);
    let buf = render(&mut app, 100, 30);
    let x = app.term_rect.x;
    let item = app.item_hits.iter().find(|(_, id)| id == "a").unwrap().0;
    assert_eq!(buf[(item.x, item.y)].symbol(), "▌");
    assert_eq!(buf[(x, 0)].symbol(), " ", "no marker while the sidebar has focus");
    let (t1, t2) = (app.tab_hits[0].0.x + 2, app.tab_hits[1].0.x + 2);
    assert_ne!(buf[(t1, 0)].bg, buf[(t2, 0)].bg, "the active tab still stands out");

    app.focus = Focus::Terminal;
    let buf = render(&mut app, 100, 30);
    let item = app.item_hits.iter().find(|(_, id)| id == "a").unwrap().0;
    assert_eq!(buf[(item.x, item.y)].symbol(), " ");
    assert_eq!(buf[(x - 1, 0)].symbol(), "┃", "the frame marks the first tab, in its column");
    assert_eq!(buf[(x, 0)].symbol(), " ");
    assert_eq!(buf[(app.tab_hits[1].0.x, 0)].symbol(), " ", "only the active tab is marked");

    app.rt.get_mut("a").unwrap().active = 1;
    let buf = render(&mut app, 100, 30);
    assert_eq!(buf[(x - 1, 0)].symbol(), "│");
    assert_eq!(buf[(x, 0)].symbol(), " ");
    assert_eq!(buf[(app.tab_hits[1].0.x, 0)].symbol(), "▌");
}

fn luma(c: ratatui::style::Color) -> f32 {
    let ratatui::style::Color::Rgb(r, g, b) = c else { panic!("{c:?} isn't RGB") };
    0.3 * r as f32 + 0.59 * g as f32 + 0.11 * b as f32
}

#[test]
fn the_side_without_focus_is_dimmed() {
    // In the dark theme dimmed text is darker. The issue list dims; the
    // " salash" header and the key footer stay lit.
    let mut app = focus_app(Focus::Sidebar);
    let lit = render(&mut app, 100, 30);
    let item = app.item_hits.iter().find(|(_, id)| id == "a").unwrap().0;
    // The issue's title, on its second line.
    let (ix, iy) = (item.x + 3, item.y + 1);
    let (tx, sy) = (app.tab_hits[1].0.x + 3, 29);
    app.focus = Focus::Terminal;
    let dim = render(&mut app, 100, 30);
    assert!(luma(dim[(ix, iy)].fg) < luma(lit[(ix, iy)].fg), "issue list");
    assert_eq!(dim[(1, 0)].fg, lit[(1, 0)].fg, "header stays lit");
    assert_eq!(dim[(1, 25)].fg, lit[(1, 25)].fg, "footer stays lit");
    // And the other way: tab bar and status bar dim with the sidebar focused.
    assert!(luma(lit[(tx, 0)].fg) < luma(dim[(tx, 0)].fg), "tab bar");
    assert!(luma(lit[(99, sy)].bg) < luma(dim[(99, sy)].bg), "status bar");
}

#[test]
fn clicking_the_frame_line_focuses_the_terminal() {
    let mut app = focus_app(Focus::Sidebar);
    render(&mut app, 100, 30);
    let (x, y) = (app.frame_rect.x + 5, app.frame_rect.y);
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.focus == Focus::Terminal);
}

#[test]
fn closing_a_tab_names_it() {
    let mut app = focus_app(Focus::Sidebar);
    app.mode = Mode::Confirm(ConfirmKind::CloseTab);
    let buf = render(&mut app, 120, 30);
    assert!((0..30).any(|y| row(&buf, y, 0).contains("Close tab \"t1\"")));
}

#[test]
fn dialogs_are_drawn_over_the_dimming() {
    let mut app = focus_app(Focus::Terminal);
    app.mode = Mode::Help;
    let buf = render(&mut app, 100, 40);
    // The help box's title sits over the sidebar; its border keeps the accent.
    let y = (0..40).find(|&y| row(&buf, y, 0).contains("Keys — any key")).unwrap();
    let x = (0..100).find(|&x| buf[(x, y)].symbol() == "╭").unwrap();
    assert_eq!(buf[(x, y)].fg, crate::theme::get().accent);
}

#[test]
fn tiny_and_empty_screens_still_draw() {
    let mut app = focus_app(Focus::Terminal);
    for (w, h) in [(1, 1), (10, 3), (40, 5), (60, 6)] {
        render(&mut app, w, h);
    }
    app.selected = None;
    app.rt.clear();
    app.focus = Focus::Sidebar;
    render(&mut app, 80, 20);
}

#[test]
fn a_key_the_sidebar_ignores_says_where_focus_is() {
    let press = |app: &mut App, code, mods| app.handle(Event::Key(KeyEvent::new(code, mods)));
    let mut app = focus_app(Focus::Sidebar);
    press(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
    let msg = app.msg.take().map(|m| m.0).unwrap_or_default();
    assert!(msg.starts_with("Sidebar has focus"), "{msg}");
    assert!(msg.contains(&format!("{} s", app.prefix_label())));
    assert!(app.focus == Focus::Sidebar && app.mode == Mode::Normal);

    press(&mut app, KeyCode::Char('Y'), KeyModifiers::SHIFT);
    assert!(app.msg.take().is_some(), "shift still counts as typing");

    press(&mut app, KeyCode::Char('y'), KeyModifiers::ALT);
    press(&mut app, KeyCode::Char('y'), KeyModifiers::META);
    press(&mut app, KeyCode::F(5), KeyModifiers::NONE);
    assert!(app.msg.is_none(), "{:?}", app.msg);

    // Issue b has no tab to type in, so there's nothing to suggest.
    press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(app.selected.as_deref(), Some("b"));
    press(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(app.msg.is_none(), "{:?}", app.msg);

    app.focus = Focus::Terminal;
    press(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(app.msg.as_ref().is_none_or(|m| !m.0.starts_with("Sidebar")));
}

#[test]
fn change_notifications_follow_status_not_hook_noise() {
    use crate::query::{diff, fingerprints};
    let (mut app, _vault) = click_app(&["A-1"]);
    let base = fingerprints(&app);
    assert!(base.listed.is_empty(), "an issue without tabs isn't listed");
    with_tabs(&mut app, "A-1", vec![unstarted_tab()], 0);
    let opened = fingerprints(&app);
    let d = diff(&base, &opened);
    assert!(d.list, "{d:?}");
    assert_eq!(d.uris, ["salash://issue/A-1", "salash://issues", "salash://tab/t"]);
    assert!(diff(&opened, &fingerprints(&app)).uris.is_empty(), "nothing changed");

    // Looking at a tab or a hook's message doesn't notify; its status does.
    app.rt.get_mut("A-1").unwrap().tabs[0].seen_ts = 99;
    assert_eq!(fingerprints(&app), opened);
    app.rt.get_mut("A-1").unwrap().tabs[0].status = ClaudeStatus::Working;
    let d = diff(&opened, &fingerprints(&app));
    assert_eq!((d.uris.as_slice(), d.list), (["salash://tab/t".to_string()].as_slice(), false));

    // A title change is the issue's and the list's, not its tabs'.
    let working = fingerprints(&app);
    app.notes[0].title = "renamed".into();
    assert_eq!(diff(&working, &fingerprints(&app)).uris, ["salash://issue/A-1", "salash://issues"]);

    // The last tab closing: its URI goes and the issue leaves the list.
    let renamed = fingerprints(&app);
    app.rt.get_mut("A-1").unwrap().tabs.clear();
    let d = diff(&renamed, &fingerprints(&app));
    assert!(d.list && d.uris.contains(&"salash://tab/t".to_string()), "{d:?}");
}
