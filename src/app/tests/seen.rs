//! Tab status: seen, done, interrupts, ranking, change notifications.

use super::super::types::{activity, dwell, scratch_shown};
use super::*;

#[test]
fn a_done_tab_reads_as_idle_only_once_seen() {
    let mut t = tab(ClaudeStatus::Done, 1000, None);
    assert_eq!(t.shown_status(), ClaudeStatus::Done);
    t.seen_ts = 999;
    assert_eq!(t.shown_status(), ClaudeStatus::Done);
    t.seen_ts = 1000;
    assert_eq!(t.shown_status(), ClaudeStatus::Idle);
    let mut w = tab(ClaudeStatus::Working, 1000, None);
    w.seen_ts = 5000;
    assert_eq!(w.shown_status(), ClaudeStatus::Working);
}

#[test]
fn a_tab_counts_as_seen_after_staying_on_view() {
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let after = Duration::from_secs(5);
    let mut v = None;
    assert!(!dwell(&mut v, Some("a"), at(0), after));
    assert!(!dwell(&mut v, Some("a"), at(4999), after));
    assert!(dwell(&mut v, Some("a"), at(5000), after));
    // Another tab, then back: the timer starts again.
    assert!(!dwell(&mut v, Some("b"), at(6000), after));
    assert!(!dwell(&mut v, Some("a"), at(7000), after));
    assert!(!dwell(&mut v, Some("a"), at(11999), after));
    assert!(dwell(&mut v, Some("a"), at(12000), after));
    // Nothing on view resets it too.
    assert!(!dwell(&mut v, None, at(12500), after));
    assert!(!dwell(&mut v, Some("a"), at(13000), after));
    // A zero delay counts at once.
    assert!(dwell(&mut None, Some("a"), at(0), Duration::ZERO));
}

#[test]
fn flicking_past_an_issue_keeps_its_done_unread() {
    let mut app = app(&[("a", &["a1"]), ("b", &["b1"])]);
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    app.mark_seen(at(0));
    app.mark_seen(at(3));
    app.selected = Some("b".into());
    app.mark_seen(at(4));
    assert_eq!(shown(&app, "a", 0), ClaudeStatus::Done);
    // Back on "a": the timer starts again from there.
    app.selected = Some("a".into());
    app.mark_seen(at(5));
    app.mark_seen(at(9));
    assert_eq!(shown(&app, "a", 0), ClaudeStatus::Done);
    app.mark_seen(at(10));
    assert_eq!(shown(&app, "a", 0), ClaudeStatus::Idle);
    assert_eq!(shown(&app, "b", 0), ClaudeStatus::Done);
}

#[test]
fn switching_tab_restarts_the_timer_and_marks_the_tab_on_view() {
    let mut app = app(&[("a", &["a1", "a2"])]);
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    app.mark_seen(at(0));
    app.rt.get_mut("a").unwrap().active = 1;
    app.mark_seen(at(4));
    app.mark_seen(at(8));
    assert_eq!(shown(&app, "a", 1), ClaudeStatus::Done);
    app.mark_seen(at(9));
    assert_eq!(shown(&app, "a", 0), ClaudeStatus::Done);
    assert_eq!(shown(&app, "a", 1), ClaudeStatus::Idle);
}

#[test]
fn no_tab_on_view_clears_the_timer() {
    let mut app = app(&[("a", &["a1"])]);
    app.mark_seen(Instant::now());
    assert!(app.on_view.is_some());
    app.rt.get_mut("a").unwrap().tabs.clear();
    app.mark_seen(Instant::now());
    assert!(app.on_view.is_none());
}

#[test]
fn a_zero_delay_marks_the_tab_seen_at_once() {
    let mut app = app(&[("a", &["a1"])]);
    app.cfg.seen_after_secs = 0;
    app.mark_seen(Instant::now());
    assert_eq!(shown(&app, "a", 0), ClaudeStatus::Idle);
}

#[test]
fn an_overlay_hides_the_tab() {
    let mut app = app(&[("a", &["a1"])]);
    let t0 = Instant::now();
    app.mode = Mode::Help;
    app.mark_seen(t0);
    app.mark_seen(t0 + Duration::from_secs(10));
    assert_eq!(shown(&app, "a", 0), ClaudeStatus::Done);
    // Closing it starts the timer.
    app.mode = Mode::Normal;
    app.mark_seen(t0 + Duration::from_secs(11));
    app.mark_seen(t0 + Duration::from_secs(16));
    assert_eq!(shown(&app, "a", 0), ClaudeStatus::Idle);
}

#[test]
fn a_done_seen_while_selected_keeps_its_rank_until_you_move() {
    let mut app = app(&[("a", &["a1"]), ("b", &["b1"])]);
    let t0 = Instant::now();
    app.mark_seen(t0);
    app.mark_seen(t0 + Duration::from_secs(5));
    let a1 = |app: &App| app.ranked_status("a", &app.rt["a"].tabs[0]);
    assert_eq!(shown(&app, "a", 0), ClaudeStatus::Idle);
    assert_eq!(a1(&app), ClaudeStatus::Done);
    app.selected = Some("b".into());
    app.mark_seen(t0 + Duration::from_secs(6));
    assert_eq!(a1(&app), ClaudeStatus::Idle);
    // "b" was only flicked past: it ranks as Done.
    assert_eq!(app.ranked_status("b", &app.rt["b"].tabs[0]), ClaudeStatus::Done);
}

#[test]
fn scratch_hides_while_empty_and_unselected() {
    assert!(!scratch_shown(0, false));
    assert!(scratch_shown(0, true));
    assert!(scratch_shown(2, false));
}

#[test]
fn closed_tabs_are_capped_oldest_first() {
    let mut rt = IssueRt::default();
    for i in 0..MAX_CLOSED + 2 {
        let mut st = tab(ClaudeStatus::Exited, 0, None).st;
        st.id = format!("salash-test-no-snapshot-{i}");
        rt.push_closed(st);
    }
    assert_eq!(rt.closed.len(), MAX_CLOSED);
    assert_eq!(rt.closed[0].id, "salash-test-no-snapshot-2");
}

#[test]
fn removing_a_tab_keeps_the_active_one() {
    let rt_with = |active| {
        let mut rt = IssueRt::default();
        for i in 0..3 {
            let mut t = tab(ClaudeStatus::Exited, 0, None);
            t.st.id = i.to_string();
            rt.tabs.push(t);
        }
        rt.active = active;
        rt
    };
    let active = |rt: &IssueRt| rt.tabs[rt.active].st.id.clone();

    let mut rt = rt_with(2);
    rt.remove_tab(0);
    assert_eq!(active(&rt), "2");
    let mut rt = rt_with(0);
    rt.remove_tab(2);
    assert_eq!(active(&rt), "0");
    // The active one itself: the next tab, or the one before the last.
    let mut rt = rt_with(1);
    rt.remove_tab(1);
    assert_eq!(active(&rt), "2");
    rt.remove_tab(1);
    assert_eq!(active(&rt), "0");
    rt.remove_tab(0);
    assert_eq!(rt.active, 0);
}

#[test]
fn interrupt_settles_only_without_hooks() {
    let mut t = tab(ClaudeStatus::Attention, 100, Some(1000));
    assert!(!t.take_interrupt(1000 + INTERRUPT_SETTLE_MS - 1));
    assert!(t.take_interrupt(1000 + INTERRUPT_SETTLE_MS));
    assert_eq!(t.esc_ts, None);

    // A hook after the Esc: Claude carried on.
    let mut t = tab(ClaudeStatus::Working, 1200, Some(1000));
    assert!(!t.take_interrupt(9000));
    assert_eq!(t.esc_ts, None);

    // Already resolved some other way.
    let mut t = tab(ClaudeStatus::Done, 100, Some(1000));
    assert!(!t.take_interrupt(9000));
}

#[test]
fn the_most_urgent_tab_ranks_the_issue() {
    use ClaudeStatus::*;
    let level = |tabs: &[(TabKind, Option<ClaudeStatus>)]| tabs.iter().map(|(k, s)| activity(*k, *s)).max().unwrap_or(0);
    let busy = level(&[(TabKind::Shell, None), (TabKind::Claude, Some(Idle)), (TabKind::Claude, Some(Working))]);
    let done = level(&[(TabKind::Claude, Some(Done))]);
    let idle = level(&[(TabKind::Claude, Some(Idle)), (TabKind::Shell, None)]);
    let shell = level(&[(TabKind::Shell, None), (TabKind::Notes, None)]);
    assert!(level(&[(TabKind::Claude, Some(Attention))]) > busy);
    assert!(busy > done && done > idle && idle > shell && shell > level(&[]));
    // A suspended session counts as an inactive Claude, as does one that exited.
    assert_eq!(activity(TabKind::Claude, None), idle);
    assert_eq!(activity(TabKind::Claude, Some(Exited)), idle);
    assert_eq!(level(&[(TabKind::Notes, None)]), 0);
}
