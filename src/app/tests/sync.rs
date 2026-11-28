//! Syncing in a folder that picks no sources ([#43](https://github.com/martin-dekastello/salash/issues/43)).

use super::*;
use crate::config::GithubSource;

fn source(repo: &str) -> GithubSource {
    GithubSource { repo: repo.into(), path: None, id_prefix: None, search: None, token_env: None, worktrees: None }
}

#[test]
fn sync_in_a_folder_without_sources_says_how_to_add_one() {
    let mut app = app(&[("a", &[])]);
    app.cfg.catalog_github = vec![source("a/one"), source("a/two")];
    key(&mut app, 'S');
    assert!(!app.syncing, "nothing to sync");
    assert_eq!(flashed(&app), "No sources for this folder: `salash sources add <repo|url>` (your config lists 2)");
}

#[test]
fn sync_with_an_empty_catalog_points_at_your_config() {
    let mut app = app(&[("a", &[])]);
    key(&mut app, 'S');
    assert!(!app.syncing);
    assert!(flashed(&app).starts_with("No [[github]] or [[jira]] sources in "), "{}", flashed(&app));
}
