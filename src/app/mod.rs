use crate::api::{self, Reply, Request};
use crate::complete::{self, Completion};
use crate::config::Config;
use crate::keys;
use crate::plugin;
use crate::proto::ServerMsg;
use crate::providers::{self, SyncReport};
use crate::state::{self, IssueState, SortMode, State, TabKind, TabState};
use crate::status::{self, ClaudeStatus};
use crate::term::{self, Pane, Snapshot};
use crate::transcript;
use crate::vault::{NewNote, Note, Stage, Vault};
use crate::worktree;
use anyhow::Result;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

mod issues;
mod keyboard;
mod mouse;
mod spawn;
mod tabs;
#[cfg(test)]
mod tests;
mod tick;
mod types;

pub use types::{IssueRt, Tab};

/// How long after Esc a Claude tab may stay silent before it counts as interrupted.
const INTERRUPT_SETTLE_MS: i64 = 1500;
/// Closed Claude tabs kept per issue for reopening.
const MAX_CLOSED: usize = 10;
/// The issue id of Scratch: tabs that belong to no issue. It has no note.
pub const SCRATCH: &str = "scratch";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Terminal,
}

#[derive(Clone, PartialEq, Eq)]
pub enum PromptKind {
    NewIssue,
    RenameTab,
    ClaudeDir,
    DefaultDir,
    ResumeId,
}

impl PromptKind {
    fn is_dir(&self) -> bool {
        matches!(self, PromptKind::ClaudeDir | PromptKind::DefaultDir)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ConfirmKind {
    Quit,
    CloseTab,
    Suspend,
    /// Move the issue to Done or Archive, stopping its tabs, `busy` among them.
    Retire { issue: String, stage: Stage, busy: Vec<String> },
}

#[derive(Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Prefix,
    Prompt(PromptKind, String),
    Confirm(ConfirmKind),
    Help,
}

pub struct App {
    pub cfg: Config,
    /// The folder this salash runs in: where tabs on issues with no repo start.
    pub folder: PathBuf,
    pub vault: Vault,
    pub notes: Vec<Note>,
    pub view: Stage,
    pub selected: Option<String>,
    pub rt: HashMap<String, IssueRt>,
    /// Saved screen of the suspended tab on view, by tab id. Only one is ever
    /// loaded, so suspended tabs cost no memory.
    pub viewer: Option<(String, Snapshot)>,
    pub state: State,
    pub focus: Focus,
    pub mode: Mode,
    /// Tab matches on show in the directory prompt.
    pub completion: Option<Completion>,
    pub prefix: (KeyCode, KeyModifiers),
    pub msg: Option<(String, Instant)>,
    /// The message is an error: shown in red, for longer.
    pub msg_error: bool,
    sync_rx: Option<Receiver<SyncReport>>,
    pub syncing: bool,
    pub spinner: usize,
    pub quit: bool,
    /// The client whose input this was asked to detach.
    pub detach: bool,
    /// For the client whose input this is: what to copy or open on its side.
    pub outbox: Vec<ServerMsg>,
    hooks_settings: Option<PathBuf>,
    /// The `--mcp-config` file Claude tabs get, unless `mcp = false`.
    mcp_config: Option<PathBuf>,
    /// salash's plugin (its skills), passed as `claude --plugin-dir`.
    plugin_dir: Option<PathBuf>,
    bell_pending: bool,
    // Hit areas from the last draw.
    pub term_rect: Rect,
    /// The frame line under the tab bar.
    pub frame_rect: Rect,
    pub sidebar_rect: Rect,
    pub tab_hits: Vec<(Rect, usize)>,
    pub close_hits: Vec<(Rect, usize)>,
    pub quit_hit: Rect,
    /// The sort label in the sidebar header; a click reverses the sort.
    pub sort_hit: Rect,
    /// A mouse press went to the pane, so its drag and release go there too.
    pane_drag: bool,
    /// The shell tab being selected in, and where the mouse last was.
    pane_select: Option<(String, Position)>,
    pub item_hits: Vec<(Rect, String)>,
    pub stage_hits: Vec<(Rect, Stage)>,
    last_scan: Instant,
    last_status: Instant,
    last_spin: Instant,
    /// The tab on view and since when: it counts as seen after
    /// `seen_after_secs`. One for everyone while all clients share one view.
    /// Monotonic, so a clock step or sleep doesn't mark it seen.
    on_view: Option<(String, Instant)>,
    /// The selected issue and since when (ms, like `seen_ts`): a Done seen
    /// meanwhile keeps its rank, so the list doesn't reorder under the cursor.
    held: Option<(String, i64)>,
    /// Busy tabs last reported left running on an issue moved to Done or
    /// Archive outside salash, so the rescan flashes only when that changes.
    busy_left: HashMap<String, Vec<String>>,
    /// Issues whose resume lines couldn't be written to the note, so the
    /// rescan retrying it flashes only the first failure.
    write_failed: HashSet<String>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Absolute path of a typed directory, or None if it isn't one.
fn resolve_dir(input: &str) -> Option<PathBuf> {
    let dir = std::fs::canonicalize(crate::config::expand(Path::new(input))).ok()?;
    dir.is_dir().then_some(dir)
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The saved default directory if it still exists, otherwise salash's folder.
fn usable_default(saved: Option<&Path>, folder: &Path) -> PathBuf {
    saved.filter(|p| p.is_dir()).unwrap_or(folder).to_path_buf()
}

/// "1 tab", "3 busy tabs".
pub fn tabs_n(n: usize, adj: &str) -> String {
    format!("{n} {adj}{}", if n == 1 { "tab" } else { "tabs" })
}

/// A word as the shell reads it back: bare if it's only safe characters,
/// else quoted. Control characters, which would break the note's line, are
/// written as escapes in `$'…'`, which bash and zsh read.
fn shell_word(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%".contains(c)) {
        s.to_string()
    } else if s.chars().any(char::is_control) {
        let mut out = String::from("$'");
        for c in s.chars() {
            match c {
                '\\' | '\'' => out.extend(['\\', c]),
                // \x is the form every shell with `$'…'` reads; \u needs bash 4.2.
                c if c.is_ascii_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
                c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('\'');
        out
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}


impl App {
    pub fn new(cfg: Config, first_run: bool, folder: PathBuf) -> Result<App> {
        let vault = Vault::new(cfg.notes_root())?;
        let notes = vault.scan();
        let state = State::load();
        let hooks_settings = if cfg.inject_hooks { status::write_settings_file().ok() } else { None };
        let mcp_config = if cfg.mcp { status::write_mcp_file(&folder).ok() } else { None };
        // Not tied to inject_hooks: --plugin-dir leaves ~/.claude alone.
        let plugin_dir = crate::config::data_dir().join("plugin");
        let plugin_dir = match plugin::write_to(&plugin_dir) {
            Ok(()) => Some(plugin_dir),
            Err(e) => {
                eprintln!("writing salash's Claude plugin: {e:#}");
                None
            }
        };
        let mut app = App::with_parts(cfg, folder, vault, notes, state, hooks_settings, mcp_config, plugin_dir);
        if let Some(n) = app.selected_note() {
            app.view = n.stage;
        } else if app.count(Stage::Active) == 0 {
            app.view = Stage::Todo;
        }
        if app.selected.is_none() {
            app.selected = app.rows().first().cloned();
        }
        if let Some(id) = app.selected.clone() {
            app.rt_for(&id);
        }
        if first_run {
            app.flash(format!("Created {} — set your vault and sources there", crate::config::config_path().display()));
        } else if app.notes.is_empty() {
            app.flash("No issues yet: n = new local issue, S = sync GitHub/Jira".into());
        }
        app.start_sync();
        Ok(app)
    }

    /// The app over what `new` loaded, with no disk access and no sync.
    #[allow(clippy::too_many_arguments)]
    fn with_parts(cfg: Config, folder: PathBuf, vault: Vault, notes: Vec<Note>, state: State, hooks_settings: Option<PathBuf>, mcp_config: Option<PathBuf>, plugin_dir: Option<PathBuf>) -> App {
        let prefix = keys::parse_prefix(&cfg.prefix);
        App {
            selected: state.selected.clone().filter(|id| {
                notes.iter().any(|n| &n.id == id)
                    || (id == SCRATCH && state.issues.get(SCRATCH).is_some_and(|s| !s.tabs.is_empty()))
            }),
            cfg,
            folder,
            vault,
            notes,
            view: Stage::Active,
            rt: HashMap::new(),
            viewer: None,
            state,
            focus: Focus::Sidebar,
            mode: Mode::Normal,
            completion: None,
            prefix,
            msg: None,
            msg_error: false,
            sync_rx: None,
            syncing: false,
            spinner: 0,
            quit: false,
            detach: false,
            outbox: vec![],
            hooks_settings,
            mcp_config,
            plugin_dir,
            bell_pending: false,
            term_rect: Rect::default(),
            frame_rect: Rect::default(),
            sidebar_rect: Rect::default(),
            tab_hits: vec![],
            close_hits: vec![],
            quit_hit: Rect::default(),
            sort_hit: Rect::default(),
            pane_drag: false,
            pane_select: None,
            item_hits: vec![],
            stage_hits: vec![],
            last_scan: Instant::now(),
            last_status: Instant::now(),
            last_spin: Instant::now(),
            on_view: None,
            held: None,
            busy_left: HashMap::new(),
            write_failed: HashSet::new(),
        }
    }

    /// The configured Claude command as the note's resume lines write it.
    fn claude_words(&self) -> String {
        match self.cfg.claude_command.as_slice() {
            [] => "claude".into(),
            cmd => cmd.iter().map(|w| shell_word(w)).collect::<Vec<_>>().join(" "),
        }
    }

    pub fn flash(&mut self, s: String) {
        self.msg = Some((s, Instant::now()));
        self.msg_error = false;
    }

    pub fn flash_error(&mut self, s: String) {
        self.msg = Some((s, Instant::now()));
        self.msg_error = true;
    }

    pub fn prefix_label(&self) -> String {
        match self.prefix.0 {
            KeyCode::Char(' ') => "C-Space".into(),
            KeyCode::Char(c) => format!("C-{c}"),
            _ => "prefix".into(),
        }
    }
}
