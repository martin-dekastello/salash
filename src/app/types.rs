//! A tab and the runtime state of one issue's tabs.

use super::*;

pub struct Tab {
    pub st: TabState,
    pub pane: Option<Pane>,
    pub status: ClaudeStatus,
    pub status_ts: i64,
    pub seen_ts: i64,
    /// When Esc was sent while Claude was busy; cleared once that is resolved.
    pub esc_ts: Option<i64>,
    /// Set when the process exits, until the tab is checked on the next poll;
    /// true if it never got as far as a hook, as when `--resume` fails.
    pub(super) exit_check: Option<bool>,
    /// Extra Claude arguments for the first launch only (mode, first prompt);
    /// a resumed session never gets them again.
    pub(super) first_args: Vec<String>,
    /// The session `I` opened this tab to resume, until its first hook
    /// records it as `claude_session`.
    pub(super) resuming: Option<String>,
}

impl Tab {
    pub(super) fn new(st: TabState) -> Tab {
        Tab { st, pane: None, status: ClaudeStatus::Exited, status_ts: 0, seen_ts: 0, esc_ts: None, exit_check: None, first_args: vec![], resuming: None }
    }

    /// The tab runs or holds this Claude session, or is starting it.
    pub(super) fn has_session(&self, sid: &str) -> bool {
        self.st.claude_session.as_deref() == Some(sid) || self.resuming.as_deref() == Some(sid)
    }

    /// Status as shown: "Done" turns into "Idle" once the tab has been on view
    /// for `seen_after_secs`.
    pub fn shown_status(&self) -> ClaudeStatus {
        if self.status == ClaudeStatus::Done && self.status_ts <= self.seen_ts {
            ClaudeStatus::Idle
        } else {
            self.status
        }
    }

    pub fn running(&self) -> bool {
        self.pane.as_ref().is_some_and(|p| p.exited.is_none())
    }

    /// A Claude session with no process: readable, and `--resume` continues it.
    pub fn suspended(&self) -> bool {
        self.st.kind == TabKind::Claude && self.pane.is_none() && self.st.claude_session.is_some()
    }

    /// False for a session with no messages yet, which `--resume` can't find.
    pub(super) fn resumable(&self) -> bool {
        let Some(sid) = &self.st.claude_session else { return false };
        match status::read_status(&self.st.id) {
            Some(sf) if sf.session_id.as_ref() == Some(sid) => {
                sf.transcript.is_none_or(|t| Path::new(&t).exists())
            }
            _ => true,
        }
    }

    /// Save the screen, with the conversation above it if the terminal kept none.
    pub(super) fn save_screen(&self) {
        let Some(p) = &self.pane else { return };
        let mut history = vec![];
        if !p.has_scrollback() {
            if let Some(t) = status::read_status(&self.st.id).and_then(|s| s.transcript) {
                history = transcript::history(Path::new(&t));
            }
        }
        state::write_snapshot(&self.st.id, &p.snapshot(&history));
    }

    /// Apply the hook status file. Returns the session id if it's a new one.
    pub(super) fn sync_status(&mut self) -> Option<String> {
        let sf = status::read_status(&self.st.id)?;
        if sf.ts > self.status_ts || sf.status != self.status {
            self.status = sf.status;
            self.status_ts = sf.ts;
        }
        let sid = sf.session_id?;
        if self.st.claude_session.as_ref() == Some(&sid) {
            return None;
        }
        self.st.claude_session = Some(sid.clone());
        Some(sid)
    }

    pub(super) fn busy(&self) -> bool {
        matches!(self.status, ClaudeStatus::Working | ClaudeStatus::Attention)
    }

    /// What keeps the tab busy, as the confirm before stopping it names it;
    /// None if stopping it loses nothing under way. A shell is busy while a
    /// command runs in front of its prompt; an open note editor always is.
    pub(super) fn busy_label(&self) -> Option<String> {
        if !self.running() {
            return None;
        }
        match self.st.kind {
            TabKind::Claude => match self.status {
                ClaudeStatus::Working => Some("Claude: working".into()),
                ClaudeStatus::Starting => Some("Claude: starting".into()),
                ClaudeStatus::Attention => Some("Claude: waiting for you".into()),
                _ => None,
            },
            TabKind::Shell => {
                let pid = self.pane.as_ref()?.foreground_pid()?;
                Some(format!("shell: {}", term::process_name(pid).unwrap_or_else(|| "running command".into())))
            }
            TabKind::Notes => Some("note editor".into()),
        }
    }

    /// True once a pending Esc went unanswered by any hook. A hook after the
    /// Esc means Claude carried on, so the Esc is dropped.
    pub(super) fn take_interrupt(&mut self, now: i64) -> bool {
        let Some(esc) = self.esc_ts else { return false };
        if !self.busy() || self.status_ts >= esc {
            self.esc_ts = None;
            return false;
        }
        if now - esc < INTERRUPT_SETTLE_MS {
            return false;
        }
        self.esc_ts = None;
        true
    }
}

/// A worktree being created in the background, and the tabs waiting to start in it.
pub struct WorktreeJob {
    pub(super) rx: Receiver<Result<PathBuf, String>>,
    pub(super) waiting: Vec<String>,
    /// Where the tabs start if creating it fails; None reports the failure instead.
    pub(super) fallback: Option<PathBuf>,
}

#[derive(Default)]
pub struct IssueRt {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub worktree: Option<PathBuf>,
    /// Closed Claude tabs for `R`. Only resumable sessions are ever put here
    /// (`close_tab_at`, `retire_tabs`), so they aren't checked again.
    pub closed: Vec<TabState>,
    pub creating: Option<WorktreeJob>,
    /// Why the worktree script last failed, until it's tried again.
    pub worktree_error: Option<String>,
}

/// How busy a tab keeps its issue, for the activity sort; higher first.
/// `status` is the shown status of a Claude tab whose process runs.
pub(super) fn activity(kind: TabKind, status: Option<ClaudeStatus>) -> u8 {
    match (kind, status) {
        (TabKind::Claude, Some(ClaudeStatus::Attention)) => 5,
        (TabKind::Claude, Some(ClaudeStatus::Working | ClaudeStatus::Starting)) => 4,
        (TabKind::Claude, Some(ClaudeStatus::Done)) => 3,
        (TabKind::Claude, _) => 2,
        (TabKind::Shell, _) => 1,
        (TabKind::Notes, _) => 0,
    }
}

pub(super) fn scratch_shown(tabs: usize, selected: bool) -> bool {
    tabs > 0 || selected
}

/// Track the tab on view. True once the same tab has been on view for `after`.
pub(super) fn dwell(on_view: &mut Option<(String, Instant)>, tab: Option<&str>, now: Instant, after: Duration) -> bool {
    match (on_view.as_ref(), tab) {
        (_, None) => {
            *on_view = None;
            false
        }
        (Some((id, since)), Some(t)) if id == t => now.saturating_duration_since(*since) >= after,
        (_, Some(t)) => {
            *on_view = Some((t.to_string(), now));
            after.is_zero()
        }
    }
}

impl IssueRt {
    /// The tab is waiting for its worktree to be created.
    pub fn awaits_worktree(&self, tab: &Tab) -> bool {
        self.creating.as_ref().is_some_and(|j| j.waiting.contains(&tab.st.id))
    }

    pub fn rollup(&self) -> Option<ClaudeStatus> {
        self.tabs
            .iter()
            .filter(|t| t.st.kind == TabKind::Claude && t.pane.is_some())
            .map(|t| t.shown_status())
            .max_by_key(|s| s.urgency())
    }

    /// Take a tab out, keeping the same tab active unless it was this one.
    pub(super) fn remove_tab(&mut self, idx: usize) -> Tab {
        let t = self.tabs.remove(idx);
        if idx < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        t
    }

    /// Remember a closed tab, forgetting the oldest beyond MAX_CLOSED.
    pub(super) fn push_closed(&mut self, st: TabState) {
        self.closed.push(st);
        if self.closed.len() > MAX_CLOSED {
            state::remove_snapshot(&self.closed.remove(0).id);
        }
    }
}
