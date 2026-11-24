//! Sorting the issue list.

use super::*;

/// An App over notes with these ids, keeping its state away from the real instance.
fn sort_app(ids: &[&str]) -> App {
    crate::config::set_data_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("target/salash-unit-data"));
    let tabs: Vec<(&str, &[&str])> = ids.iter().map(|id| (*id, &[][..])).collect();
    app(&tabs)
}

fn order(app: &App) -> Vec<String> {
    app.visible().iter().map(|n| n.id.clone()).collect()
}

#[test]
fn id_sort_groups_sources_and_counts_numbers() {
    let mut app = sort_app(&["office-10", "LOCAL-2", "office-2", "PROJ-1", "local-10"]);
    app.state.sort = SortMode::Id;
    assert_eq!(order(&app), ["LOCAL-2", "local-10", "office-2", "office-10", "PROJ-1"]);
    assert_eq!(app.sort_label(), "↑ id");
    app.state.sort_reversed = true;
    assert_eq!(order(&app), ["PROJ-1", "office-10", "office-2", "local-10", "LOCAL-2"]);
    assert_eq!(app.sort_label(), "↓ id");
}

#[test]
fn i_reverses_the_sort_and_s_starts_the_next_one_unreversed() {
    let mut app = sort_app(&["b", "a"]);
    assert_eq!(app.sort_label(), "↓ created");
    app.on_sidebar_key(KeyEvent::from(KeyCode::Char('i')));
    assert_eq!(order(&app), ["a", "b"]);
    assert_eq!(app.sort_label(), "↑ created");
    app.on_sidebar_key(KeyEvent::from(KeyCode::Char('s')));
    assert_eq!(app.state.sort, SortMode::Id);
    assert!(!app.state.sort_reversed);
    assert_eq!(order(&app), ["a", "b"]);
}

#[test]
fn clicking_the_sort_label_reverses_it() {
    let mut app = sort_app(&["a", "b"]);
    app.state.sort = SortMode::Name;
    draw(&mut app);
    let r = app.sort_hit;
    assert!(r.width > 0, "sort label drawn");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), Position::new(r.x, r.y));
    assert!(app.state.sort_reversed);
    assert_eq!(order(&app), ["b", "a"]);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), Position::new(r.right() - 1, r.y));
    assert!(!app.state.sort_reversed);
}

#[test]
fn the_sort_label_is_clickable_when_it_fits_exactly() {
    // " salash" + "  ⠋ sync" + " ↓ activity " + " quit " fill the header.
    let mut app = sort_app(&["a"]);
    app.state.sort = SortMode::Activity;
    app.syncing = true;
    draw(&mut app);
    assert_eq!(app.sort_hit.right(), app.quit_hit.x);
    assert_eq!(app.sort_hit.x, 15, "the label starts right after the header text");
    assert!(app.sort_hit.width > 0, "sort label drawn");
}

#[test]
fn reversing_keeps_ties_newest_created_first() {
    // `notes` is newest-created first: "new" was created after "old".
    let mut app = sort_app(&["new", "old", "busy"]);
    app.rt.get_mut("busy").unwrap().tabs.push(tab(ClaudeStatus::Attention, 1, None));
    app.state.sort = SortMode::Activity;
    assert_eq!(order(&app), ["busy", "new", "old"]);
    app.state.sort_reversed = true;
    assert_eq!(order(&app), ["new", "old", "busy"]);

    for n in &mut app.notes {
        n.title = if n.id == "busy" { "a".into() } else { "same".into() };
    }
    app.state.sort = SortMode::Name;
    assert_eq!(order(&app), ["new", "old", "busy"]);
    app.state.sort_reversed = false;
    assert_eq!(order(&app), ["busy", "new", "old"]);
}

#[test]
fn undated_issues_stay_last_in_the_moved_sort() {
    let mut app = sort_app(&["undated", "early", "late"]);
    for n in &mut app.notes {
        n.moved = match n.id.as_str() {
            "early" => Some("2026-01-01T00:00:00".into()),
            "late" => Some("2026-02-01T00:00:00".into()),
            _ => None,
        };
    }
    app.state.sort = SortMode::Moved;
    assert_eq!(order(&app), ["late", "early", "undated"]);
    app.state.sort_reversed = true;
    assert_eq!(order(&app), ["early", "late", "undated"]);
}

#[test]
fn sort_label_shows_each_modes_direction() {
    let mut app = sort_app(&["a"]);
    for (mode, label, reversed) in [
        (SortMode::Created, "↓ created", "↑ created"),
        (SortMode::Id, "↑ id", "↓ id"),
        (SortMode::Name, "↑ name", "↓ name"),
        (SortMode::Moved, "↓ moved", "↑ moved"),
        (SortMode::Activity, "↓ activity", "↑ activity"),
    ] {
        app.state.sort = mode;
        app.state.sort_reversed = false;
        assert_eq!(app.sort_label(), label);
        app.state.sort_reversed = true;
        assert_eq!(app.sort_label(), reversed);
    }
}
