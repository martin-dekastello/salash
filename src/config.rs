use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A GitHub repository to pull issues from (via the `gh` CLI).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GithubSource {
    /// "owner/name"
    pub repo: String,
    /// Local clone used to create per-issue worktrees.
    pub path: Option<PathBuf>,
    /// ID prefix for notes, defaults to the repo name ("transport-sim" -> "transport-sim-42").
    pub id_prefix: Option<String>,
    /// `gh issue list --search` query. Default: "assignee:@me".
    pub search: Option<String>,
    /// Environment variable holding a GitHub token, checked before the
    /// keychain and $GH_TOKEN. Without any token, `gh` uses its own login.
    pub token_env: Option<String>,
    /// Overrides the global `worktrees` for this source's issues.
    pub worktrees: Option<bool>,
}

/// A Jira Cloud site to pull issues from (REST API v3).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraSource {
    /// e.g. "https://yourcompany.atlassian.net"
    pub url: String,
    pub email: String,
    /// Environment variable holding the API token, checked before the
    /// keychain and $JIRA_API_TOKEN.
    pub token_env: Option<String>,
    /// Default: "assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC"
    pub jql: Option<String>,
    /// Local clone used to create per-issue worktrees.
    pub path: Option<PathBuf>,
    /// Overrides the global `worktrees` for this source's issues.
    pub worktrees: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// Obsidian vault root.
    pub vault: PathBuf,
    /// Folder inside the vault holding active/ done/ archive/.
    pub notes_dir: String,
    /// Where per-issue git worktrees are created: <worktrees_dir>/<repo>/<id>-<slug>
    pub worktrees_dir: PathBuf,
    /// Create a worktree per issue. Off, an issue's tabs start in its repo.
    pub worktrees: bool,
    /// Creates an issue's worktree instead of plain `git worktree add`. Run as
    /// `<command…> <repo> <id>-<slug>` in the repo; prints the worktree's path last.
    pub worktree_command: Vec<String>,
    /// Command used for Claude tabs.
    pub claude_command: Vec<String>,
    /// Command used for shell tabs. Empty = $SHELL / %COMSPEC%.
    pub shell_command: Vec<String>,
    /// Command used for the notes tab. Empty = $VISUAL / $EDITOR / vi / notepad.
    pub editor_command: Vec<String>,
    /// Pass a generated `--settings` file (status hooks) to claude, so they
    /// don't have to go into ~/.claude.
    pub inject_hooks: bool,
    /// Give Claude tabs `salash mcp` (`claude --mcp-config`), so a session can
    /// see salash's issues, tabs, their status and screens, and open tabs.
    pub mcp: bool,
    /// Prefix key, tmux style: "ctrl-a", "ctrl-b", "ctrl-g", ...
    pub prefix: String,
    /// Prefix for locally created issues ("LOCAL-1").
    pub local_prefix: String,
    /// Repo used for worktrees of local issues (optional).
    pub local_repo: Option<PathBuf>,
    /// Ring the host terminal bell when a background tab needs you or finishes.
    pub bell: bool,
    /// Seconds a tab must stay on view before its "done" counts as seen. 0 = at once.
    pub seen_after_secs: u64,
    /// Capture the mouse (click sidebar/tabs, wheel and clicks in tabs, drag to copy in shell tabs).
    /// Elsewhere, hold Shift/Option to select text.
    pub mouse: bool,
    /// "auto" (match the terminal's background), "dark" or "light".
    pub theme: String,
    /// The sources this folder syncs: the folder config's selection from
    /// `catalog_*`. Empty without a folder config, or outside any folder.
    pub github: Vec<GithubSource>,
    pub jira: Vec<JiraSource>,
    /// The sources your own config lists: the only ones a folder may select.
    #[serde(skip)]
    pub catalog_github: Vec<GithubSource>,
    #[serde(skip)]
    pub catalog_jira: Vec<JiraSource>,
    /// What the folder's `.salash/config.toml` tried to set but may not.
    #[serde(skip)]
    pub ignored: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let claude = vec!["claude".into()];
        Config {
            vault: dirs::document_dir()
                .unwrap_or_else(|| home.clone())
                .join("Obsidian"),
            notes_dir: "Issues".into(),
            worktrees_dir: home.join("worktrees"),
            worktrees: true,
            worktree_command: vec![],
            claude_command: claude,
            shell_command: vec![],
            editor_command: vec![],
            inject_hooks: true,
            mcp: true,
            prefix: "ctrl-a".into(),
            local_prefix: "LOCAL".into(),
            local_repo: None,
            bell: true,
            seen_after_secs: 5,
            mouse: true,
            theme: "auto".into(),
            github: vec![],
            jira: vec![],
            catalog_github: vec![],
            catalog_jira: vec![],
            ignored: vec![],
        }
    }
}

const TEMPLATE: &str = r#"# salash configuration
# The [[github]] and [[jira]] sources here are a catalog: none of them syncs until a folder
# picks it. `salash <folder>` asks which, or run `salash sources add <repo|url>` there; the
# picks go to <folder>/.salash/config.toml. That file can also override vault, notes_dir,
# prefix, local_prefix, bell, mouse and worktrees. Commands, repo paths, emails and
# token_env only come from here: a repo can ship its own .salash/config.toml.

# Obsidian vault root and the folder inside it that holds active/ done/ archive/
vault = "{VAULT}"
notes_dir = "Issues"

# true = one git worktree + branch per issue; false = an issue's tabs start in its repo.
# A [[github]] or [[jira]] source can set its own.
worktrees = false

# With worktrees on, per-issue git worktrees go to <worktrees_dir>/<repo>/<id>-<slug>
worktrees_dir = "{WORKTREES}"

# With worktrees on, your own script for creating them, run as `<command…> <repo> <id>-<slug>` in the
# repo when an issue's first Claude or shell tab opens. It must print the worktree's
# path as its last line of output. Default: `git worktree add` into worktrees_dir.
# worktree_command = ["~/bin/make-worktree.sh"]

# tmux-style prefix key. Press it twice to send it to the terminal.
prefix = "ctrl-a"

# claude_command = ["claude"]
# shell_command  = []          # default: $SHELL / %COMSPEC%
# editor_command = ["nvim"]    # default: $VISUAL / $EDITOR
# inject_hooks = true          # status hooks via `claude --settings`
# mcp = true                   # give Claude tabs `salash mcp`: issues, tabs, status, screens
# local_prefix = "LOCAL"
# local_repo = "/path/to/repo" # worktrees for local issues
# bell = true
# seen_after_secs = 5        # a finished tab stays unread until shown this long; 0 = at once
# mouse = true
# theme = "auto"              # auto (follow the terminal) | dark | light

# Tokens never go in this file. For each source salash looks in token_env (if set),
# then the keychain (`salash token set github:owner/repo` or `salash token set
# jira:https://yourcompany.atlassian.net`), then, without token_env, $GH_TOKEN /
# $JIRA_API_TOKEN and gh's own login. `salash token status` shows what is used.
#
# Give tokens the least they need:
# - GitHub: a fine-grained token for only the repos you sync, with Issues: read and
#   Metadata: read: https://github.com/settings/personal-access-tokens/new?name=salash&issues=read&metadata=read
# - Jira: "Create API token with scopes", app Jira, scope read:jira-work:
#   https://id.atlassian.com/manage-profile/security/api-tokens

# Sources you trust. A folder syncs only those it picks.
# [[github]]
# repo = "owner/transport-sim"
# path = "~/code/transport-sim"   # local clone for worktrees
# id_prefix = "TS"                # notes become TS-42-<slug>.md
# search = "assignee:@me"
# token_env = "GH_TOKEN"          # env var with the token (not the token itself)
# worktrees = true                # this source's own setting

# [[jira]]
# url = "https://yourcompany.atlassian.net"
# email = "you@example.com"
# token_env = "JIRA_API_TOKEN"    # env var with the token (not the token itself)
# jql = "assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC"
# path = "~/code/backend"
# worktrees = true
"#;

pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("SALASH_CONFIG") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("salash")
        .join("config.toml")
}

/// The data dir of the folder this process serves, set once at startup.
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Make `data_dir()` the folder's `.salash` for the rest of the process.
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

/// Where state, saved screens and status files live: the folder's `.salash`
/// in the server and in its tabs (`$SALASH_DATA`), the global one otherwise.
pub fn data_dir() -> PathBuf {
    if let Some(d) = DATA_DIR.get() {
        return d.clone();
    }
    if let Ok(p) = std::env::var("SALASH_DATA") {
        return PathBuf::from(p);
    }
    global_data_dir()
}

/// The data dir from before there was one per folder. `$SALASH_DATA` counts
/// unless it names a folder's `.salash`, as it does inside a tab.
pub fn global_data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("SALASH_DATA") {
        let p = PathBuf::from(p);
        if p.file_name() != Some(std::ffi::OsStr::new(crate::instance::DIR)) {
            return p;
        }
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("salash")
}

/// Expand a leading "~" to the home directory.
pub fn expand(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest.trim_start_matches(['/', '\\']));
        }
    }
    p.to_path_buf()
}

/// A directory given by the user (`~` expanded), made absolute.
pub fn dir_arg(d: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(expand(d)).with_context(|| format!("not a directory: {}", d.display()))
}

/// A path for display, with the home directory as "~".
pub fn shorten(p: &Path) -> String {
    match dirs::home_dir().and_then(|h| p.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => p.display().to_string(),
    }
}

impl Config {
    /// Load the config, writing a commented template on first run, with a
    /// folder's `.salash/config.toml` over it if given.
    pub fn load_for(instance_dir: Option<&Path>) -> Result<(Config, bool)> {
        Config::load_at(&config_path(), instance_dir)
    }

    fn load_at(path: &Path, instance_dir: Option<&Path>) -> Result<(Config, bool)> {
        let mut created = false;
        if !path.exists() {
            let d = Config::default();
            let text = TEMPLATE
                .replace("{VAULT}", &toml_escape(&d.vault))
                .replace("{WORKTREES}", &toml_escape(&d.worktrees_dir));
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, text)?;
            created = true;
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut table: toml::Table =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let own: Config = toml::Value::Table(table.clone())
            .try_into()
            .with_context(|| format!("parsing {}", path.display()))?;
        let mut ignored = vec![];
        let mut sources = path.display().to_string();
        // Your sources are a catalog: a folder syncs only what its config picks.
        let mut picked = toml::Table::new();
        if let Some(over) = instance_dir.map(|d| d.join("config.toml")).filter(|p| p.exists()) {
            let (mut t, mut dropped) = folder_overlay(&over)?;
            picked = select_sources(&table, &mut t, &mut dropped);
            if !dropped.is_empty() {
                eprintln!("ignored in {}: {}", over.display(), dropped.join(", "));
            }
            ignored = dropped;
            merge(&mut table, t);
            sources = format!("{sources} with {}", over.display());
        }
        for key in ["github", "jira"] {
            match picked.remove(key) {
                Some(v) => table.insert(key.into(), v),
                None => table.remove(key),
            };
        }
        let mut cfg: Config = toml::Value::Table(table)
            .try_into()
            .with_context(|| format!("parsing {sources}"))?;
        cfg.ignored = ignored;
        cfg.catalog_github = own.github;
        cfg.catalog_jira = own.jira;
        cfg.vault = expand(&cfg.vault);
        cfg.worktrees_dir = expand(&cfg.worktrees_dir);
        cfg.local_repo = cfg.local_repo.as_deref().map(expand);
        for g in cfg.github.iter_mut().chain(&mut cfg.catalog_github) {
            g.path = g.path.as_deref().map(expand);
        }
        for j in cfg.jira.iter_mut().chain(&mut cfg.catalog_jira) {
            j.path = j.path.as_deref().map(expand);
        }
        Ok((cfg, created))
    }

    /// What to say when this folder syncs nothing.
    pub fn no_sources_hint(&self) -> String {
        match self.catalog_github.len() + self.catalog_jira.len() {
            0 => format!("No [[github]] or [[jira]] sources in {}", shorten(&config_path())),
            n => format!(
                "No sources for this folder: `salash sources add <repo|url>` (your config lists {n})"
            ),
        }
    }

    pub fn notes_root(&self) -> PathBuf {
        self.vault.join(&self.notes_dir)
    }

    pub fn shell(&self) -> Vec<String> {
        if !self.shell_command.is_empty() {
            return self.shell_command.clone();
        }
        vec![std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())]
    }

    pub fn editor(&self) -> Vec<String> {
        if !self.editor_command.is_empty() {
            return self.editor_command.clone();
        }
        for var in ["VISUAL", "EDITOR"] {
            if let Ok(v) = std::env::var(var) {
                let parts: Vec<String> = v.split_whitespace().map(String::from).collect();
                if !parts.is_empty() {
                    return parts;
                }
            }
        }
        vec!["vi".into()]
    }

    /// The source an issue came from: this folder's pick, else your own entry,
    /// so notes from sources the folder doesn't sync still find their repo.
    fn github_for(&self, repo: Option<&str>) -> Option<&GithubSource> {
        let repo = repo?;
        self.github.iter().chain(&self.catalog_github).find(|g| same_repo(&g.repo, repo))
    }

    fn jira_for(&self, repo: Option<&str>) -> Option<&JiraSource> {
        let all = || self.jira.iter().chain(&self.catalog_jira);
        repo.and_then(|r| all().find(|j| same_site(&j.url, r))).or_else(|| all().next())
    }

    /// Resolve the local repository path for an issue.
    pub fn repo_path_for(&self, source: &str, repo: Option<&str>) -> Option<PathBuf> {
        match source {
            "github" => self.github_for(repo).and_then(|g| g.path.clone()),
            "jira" => self.jira_for(repo).and_then(|j| j.path.clone()),
            _ => self.local_repo.clone(),
        }
    }

    /// Whether an issue gets its own worktree: its source's setting, else the global one.
    pub fn worktrees_for(&self, source: &str, repo: Option<&str>) -> bool {
        let own = match source {
            "github" => self.github_for(repo).and_then(|g| g.worktrees),
            "jira" => self.jira_for(repo).and_then(|j| j.worktrees),
            _ => None,
        };
        own.unwrap_or(self.worktrees)
    }
}

/// What a folder's own config may set. A cloned repo can ship
/// `.salash/config.toml`, so nothing that runs commands or sends credentials:
/// no `*_command`, no repo paths (worktrees run their hooks), and no
/// `token_env` or Jira `email` (they choose which secret salash sends, and as whom).
/// `worktrees` only turns worktrees off or back on, so it may. `[[github]]` and
/// `[[jira]]` only pick from your own sources (`select_sources`).
const FOLDER_KEYS: &[&str] =
    &["vault", "notes_dir", "prefix", "local_prefix", "bell", "mouse", "worktrees", "github", "jira"];

/// Keys of a folder's source that only your own config may set.
const OWN_SOURCE_KEYS: &[(&str, &[&str])] = &[("github", &["path", "token_env"]), ("jira", &["path", "token_env", "email"])];

/// The key that names a source: GitHub `repo`, Jira `url`.
fn source_key(kind: &str) -> &'static str {
    if kind == "jira" { "url" } else { "repo" }
}

/// GitHub repo names ignore case.
pub fn same_repo(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Jira sites match with or without a trailing slash.
pub fn same_site(a: &str, b: &str) -> bool {
    a.trim_end_matches('/').eq_ignore_ascii_case(b.trim_end_matches('/'))
}

fn same_source(kind: &str, a: &str, b: &str) -> bool {
    if kind == "jira" { same_site(a, b) } else { same_repo(a, b) }
}

/// A folder's `.salash/config.toml`, cut down to `FOLDER_KEYS`, and the keys it lost.
fn folder_overlay(path: &Path) -> Result<(toml::Table, Vec<String>)> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let table: toml::Table =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let mut dropped = vec![];
    let mut kept = toml::Table::new();
    for (k, mut v) in table {
        if !FOLDER_KEYS.contains(&k.as_str()) {
            dropped.push(k);
            continue;
        }
        if let Some((_, keys)) = OWN_SOURCE_KEYS.iter().find(|(kind, _)| *kind == k) {
            for s in v.as_array_mut().into_iter().flatten().filter_map(|s| s.as_table_mut()) {
                for key in *keys {
                    let name = format!("{k}.{key}");
                    if s.remove(*key).is_some() && !dropped.contains(&name) {
                        dropped.push(name);
                    }
                }
            }
        }
        kept.insert(k, v);
    }
    Ok((kept, dropped))
}

/// Take the folder's `[[github]]` / `[[jira]]` out of `folder` and match each
/// to your own entry with the same repo / url: yours, with the folder's keys
/// (`search`, `jql`, `id_prefix`, `worktrees`) on top. One you don't list is
/// dropped and reported, so a cloned repo can't make salash send a token to a
/// source you never chose.
fn select_sources(own: &toml::Table, folder: &mut toml::Table, dropped: &mut Vec<String>) -> toml::Table {
    let mut picked = toml::Table::new();
    for kind in ["github", "jira"] {
        let Some(theirs) = folder.remove(kind) else { continue };
        let key = source_key(kind);
        let mine: Vec<&toml::Table> =
            own.get(kind).and_then(|v| v.as_array()).into_iter().flatten().filter_map(|m| m.as_table()).collect();
        let mut out: Vec<toml::Value> = vec![];
        for s in theirs.as_array().into_iter().flatten() {
            let Some(name) = s.get(key).and_then(|n| n.as_str()) else {
                dropped.push(format!("{kind} without {key}"));
                continue;
            };
            let found = mine.iter().find(|m| m.get(key).and_then(|v| v.as_str()).is_some_and(|v| same_source(kind, v, name)));
            let Some(m) = found else {
                dropped.push(format!("{kind} {name:?} (not in your config)"));
                continue;
            };
            let mut merged = (*m).clone();
            for (k, v) in s.as_table().into_iter().flatten() {
                if k != key {
                    merged.insert(k.clone(), v.clone());
                }
            }
            let dup = out.iter().any(|o| o.get(key).and_then(|v| v.as_str()) == merged.get(key).and_then(|v| v.as_str()));
            if !dup {
                out.push(toml::Value::Table(merged));
            }
        }
        picked.insert(kind.into(), toml::Value::Array(out));
    }
    picked
}

/// Put `over` on top of `base`: tables merge key by key, anything else
/// (including arrays like `[[github]]`) replaces what was there.
fn merge(base: &mut toml::Table, over: toml::Table) {
    for (k, v) in over {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

fn toml_escape(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_config_overrides_keys_and_replaces_arrays() {
        let mut base: toml::Table = toml::from_str(
            r#"
            vault = "/global"
            prefix = "ctrl-a"
            [[github]]
            repo = "a/one"
            [[github]]
            repo = "a/two"
            [extra]
            x = 1
            y = 2
            "#,
        )
        .unwrap();
        let over: toml::Table = toml::from_str(
            r#"
            vault = "/team"
            [[github]]
            repo = "b/three"
            [extra]
            y = 3
            "#,
        )
        .unwrap();
        merge(&mut base, over);
        assert_eq!(base["vault"].as_str(), Some("/team"));
        assert_eq!(base["prefix"].as_str(), Some("ctrl-a"));
        let gh = base["github"].as_array().unwrap();
        assert_eq!(gh.len(), 1);
        assert_eq!(gh[0]["repo"].as_str(), Some("b/three"));
        assert_eq!(base["extra"]["x"].as_integer(), Some(1));
        assert_eq!(base["extra"]["y"].as_integer(), Some(3));
    }

    #[test]
    fn seen_after_defaults_to_5_and_parses() {
        assert_eq!(toml::from_str::<Config>("").unwrap().seen_after_secs, 5);
        assert_eq!(toml::from_str::<Config>("seen_after_secs = 0").unwrap().seen_after_secs, 0);
        assert!(!FOLDER_KEYS.contains(&"seen_after_secs"));
    }

    #[test]
    fn worktrees_default_on_but_a_fresh_config_turns_them_off() {
        assert!(toml::from_str::<Config>("").unwrap().worktrees);
        assert!(!toml::from_str::<Config>("worktrees = false").unwrap().worktrees);
        let fresh = TEMPLATE.replace("{VAULT}", "/v").replace("{WORKTREES}", "/w");
        assert!(!toml::from_str::<Config>(&fresh).unwrap().worktrees);
    }

    #[test]
    fn a_source_overrides_the_global_worktrees_setting() {
        let cfg: Config = toml::from_str(
            r#"
            worktrees = true
            [[github]]
            repo = "a/off"
            worktrees = false
            [[github]]
            repo = "a/plain"
            [[jira]]
            url = "https://one.example"
            email = "x"
            worktrees = false
            [[jira]]
            url = "https://two.example"
            email = "x"
            "#,
        )
        .unwrap();
        assert!(!cfg.worktrees_for("github", Some("a/off")));
        assert!(cfg.worktrees_for("github", Some("a/plain")));
        assert!(cfg.worktrees_for("github", Some("a/unknown")));
        assert!(!cfg.worktrees_for("jira", Some("https://one.example")));
        assert!(cfg.worktrees_for("jira", Some("https://two.example")));
        // An unknown Jira site falls back to the first, as its repo path does.
        assert!(!cfg.worktrees_for("jira", None));
        assert!(cfg.worktrees_for("local", None));

        let off = Config { worktrees: false, ..cfg };
        assert!(!off.worktrees_for("github", Some("a/plain")));
        assert!(!off.worktrees_for("local", None));
        assert!(!off.worktrees_for("jira", Some("https://two.example")));
    }

    #[test]
    fn a_folder_config_may_turn_worktrees_off() {
        let dir = std::env::temp_dir().join(format!("salash-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let over = dir.join("config.toml");
        std::fs::write(
            &over,
            r#"
            worktrees = false
            [[github]]
            repo = "a/one"
            worktrees = true
            "#,
        )
        .unwrap();
        let (t, dropped) = folder_overlay(&over).unwrap();
        assert!(dropped.is_empty(), "{dropped:?}");
        assert_eq!(t["worktrees"].as_bool(), Some(false));
        assert_eq!(t["github"][0]["worktrees"].as_bool(), Some(true));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_folder_config_cannot_run_commands() {
        let dir = std::env::temp_dir().join(format!("salash-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let over = dir.join("config.toml");
        std::fs::write(
            &over,
            r#"
            vault = "/team"
            claude_command = ["sh", "-c", "echo pwned"]
            shell_command = ["sh"]
            worktree_command = ["sh"]
            local_repo = "/evil.git"
            [[github]]
            repo = "a/one"
            path = "/evil.git"
            [[jira]]
            url = "https://evil.example"
            email = "x"
            token_env = "AWS_SECRET_ACCESS_KEY"
            path = "/evil.git"
            "#,
        )
        .unwrap();
        let (t, dropped) = folder_overlay(&over).unwrap();
        assert_eq!(t["vault"].as_str(), Some("/team"));
        assert_eq!(t["github"][0]["repo"].as_str(), Some("a/one"));
        assert!(t["github"][0].get("path").is_none());
        assert_eq!(t["jira"][0]["url"].as_str(), Some("https://evil.example"));
        for key in ["email", "token_env", "path"] {
            assert!(t["jira"][0].get(key).is_none(), "jira.{key} kept");
        }
        for k in ["claude_command", "shell_command", "worktree_command", "local_repo", "github.path", "jira.email", "jira.token_env", "jira.path"] {
            assert!(dropped.iter().any(|d| d == k), "{k} not dropped: {dropped:?}");
            assert!(!t.contains_key(k));
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_folder_config_cannot_pick_token_env() {
        let dir = std::env::temp_dir().join(format!("salash-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let over = dir.join("config.toml");
        std::fs::write(
            &over,
            r#"
            [[github]]
            repo = "a/one"
            token_env = "AWS_SECRET_ACCESS_KEY"
            "#,
        )
        .unwrap();
        let (t, dropped) = folder_overlay(&over).unwrap();
        assert_eq!(t["github"][0]["repo"].as_str(), Some("a/one"));
        assert!(t["github"][0].get("token_env").is_none());
        assert_eq!(dropped, vec!["github.token_env"]);
        std::fs::remove_dir_all(dir).ok();
    }

    /// A temp dir with your own config and, if given, a folder config.
    fn setup(own: &str, folder: Option<&str>) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("salash-test-{}", uuid::Uuid::new_v4()));
        let inst = dir.join(".salash");
        std::fs::create_dir_all(&inst).unwrap();
        std::fs::write(dir.join("own.toml"), own).unwrap();
        if let Some(f) = folder {
            std::fs::write(inst.join("config.toml"), f).unwrap();
        }
        (dir, inst)
    }

    fn load(own: &str, folder: Option<&str>) -> Config {
        let (dir, inst) = setup(own, folder);
        let cfg = Config::load_at(&dir.join("own.toml"), Some(&inst)).unwrap().0;
        std::fs::remove_dir_all(dir).ok();
        cfg
    }

    const CATALOG: &str = r#"
        [[github]]
        repo = "a/one"
        path = "~/code/one"
        token_env = "WORK_GH_TOKEN"
        id_prefix = "ONE"
        search = "assignee:@me"
        [[github]]
        repo = "a/two"
        [[jira]]
        url = "https://co.atlassian.net"
        email = "me@example.com"
        token_env = "CO_JIRA"
        path = "~/code/backend"
        jql = "assignee = currentUser()"
        [[jira]]
        url = "https://other.atlassian.net"
        email = "me@example.com"
    "#;

    #[test]
    fn a_folder_syncs_only_the_sources_it_picks() {
        let cfg = load(CATALOG, Some("[[github]]\nrepo = \"a/two\"\n"));
        let repos: Vec<_> = cfg.github.iter().map(|g| g.repo.as_str()).collect();
        assert_eq!(repos, ["a/two"]);
        assert!(cfg.jira.is_empty());
        assert_eq!(cfg.catalog_github.len(), 2);
        assert_eq!(cfg.catalog_jira.len(), 2);
        assert!(cfg.ignored.is_empty(), "{:?}", cfg.ignored);
    }

    #[test]
    fn no_folder_config_means_no_sources() {
        let cfg = load(CATALOG, None);
        assert!(cfg.github.is_empty() && cfg.jira.is_empty());
        assert_eq!(cfg.catalog_github.len(), 2);
        assert!(cfg.no_sources_hint().contains("your config lists 4"), "{}", cfg.no_sources_hint());
        // A folder config without sources also syncs nothing.
        let cfg = load(CATALOG, Some("vault = \"/team\"\n"));
        assert!(cfg.github.is_empty() && cfg.jira.is_empty());
        assert_eq!(cfg.vault, PathBuf::from("/team"));
        // Nor does salash outside any folder.
        let (dir, _) = setup(CATALOG, None);
        let cfg = Config::load_at(&dir.join("own.toml"), None).unwrap().0;
        assert!(cfg.github.is_empty() && cfg.jira.is_empty());
        assert_eq!(cfg.catalog_jira.len(), 2);
        std::fs::remove_dir_all(dir).ok();
        let empty = load("", None);
        assert!(empty.no_sources_hint().starts_with("No [[github]] or [[jira]] sources in"));
    }

    #[test]
    fn a_folder_jira_takes_email_token_env_and_path_from_yours() {
        let cfg = load(
            CATALOG,
            Some("[[jira]]\nurl = \"https://co.atlassian.net/\"\njql = \"project = BE\"\nworktrees = false\n"),
        );
        assert_eq!(cfg.jira.len(), 1);
        let j = &cfg.jira[0];
        assert_eq!(j.url, "https://co.atlassian.net");
        assert_eq!(j.email, "me@example.com");
        assert_eq!(j.token_env.as_deref(), Some("CO_JIRA"));
        assert_eq!(j.path, Some(expand(Path::new("~/code/backend"))));
        assert_eq!(j.jql.as_deref(), Some("project = BE"));
        assert_eq!(j.worktrees, Some(false));
        assert!(!cfg.worktrees_for("jira", Some("https://co.atlassian.net")));
    }

    #[test]
    fn a_folder_github_inherits_your_keys_and_overrides_its_own() {
        let cfg = load(CATALOG, Some("[[github]]\nrepo = \"A/One\"\nsearch = \"label:bug\"\n"));
        let g = &cfg.github[0];
        assert_eq!(g.repo, "a/one", "your spelling wins");
        assert_eq!(g.token_env.as_deref(), Some("WORK_GH_TOKEN"));
        assert_eq!(g.path, Some(expand(Path::new("~/code/one"))));
        assert_eq!(g.id_prefix.as_deref(), Some("ONE"), "note ids stay the same");
        assert_eq!(g.search.as_deref(), Some("label:bug"));
    }

    #[test]
    fn a_folder_source_you_dont_list_is_dropped_and_reported() {
        let cfg = load(
            CATALOG,
            Some(
                r#"
                [[github]]
                repo = "evil/repo"
                token_env = "AWS_SECRET_ACCESS_KEY"
                [[github]]
                search = "no repo"
                [[jira]]
                url = "https://evil.example"
                email = "x"
                [[github]]
                repo = "a/one"
                [[github]]
                repo = "a/one"
                "#,
            ),
        );
        let repos: Vec<_> = cfg.github.iter().map(|g| g.repo.as_str()).collect();
        assert_eq!(repos, ["a/one"], "unknown dropped, duplicate kept once");
        assert!(cfg.jira.is_empty());
        for d in ["github \"evil/repo\" (not in your config)", "github without repo", "jira \"https://evil.example\" (not in your config)", "github.token_env", "jira.email"] {
            assert!(cfg.ignored.iter().any(|i| i == d), "{d} not reported: {:?}", cfg.ignored);
        }
        // Nothing for the dropped source, so no token is ever looked up for it.
        let names: Vec<_> = crate::providers::from_config(&cfg).iter().map(|p| p.name()).collect();
        assert_eq!(names, ["github:a/one"]);
    }

    #[test]
    fn notes_from_sources_the_folder_skips_still_find_their_repo() {
        let cfg = load(CATALOG, Some("[[github]]\nrepo = \"a/two\"\n"));
        assert_eq!(cfg.repo_path_for("github", Some("a/one")), Some(expand(Path::new("~/code/one"))));
        assert_eq!(cfg.repo_path_for("jira", Some("https://co.atlassian.net")), Some(expand(Path::new("~/code/backend"))));
        assert_eq!(cfg.repo_path_for("github", None), None);
    }

    #[test]
    fn source_names_match_loosely() {
        assert!(same_repo("Owner/Repo", "owner/repo"));
        assert!(!same_repo("owner/repo", "owner/repo2"));
        assert!(same_site("https://x.atlassian.net/", "https://x.atlassian.net"));
        assert!(!same_site("https://x.atlassian.net", "https://y.atlassian.net"));
    }
}
