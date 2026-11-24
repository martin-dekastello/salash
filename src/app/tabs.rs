//! Tab actions: requests, resume, switching, closing, suspending, scrolling.

use super::*;

/// What was pasted into `I`: the session id itself, or a command holding
/// `--resume <id>`, such as a `## Sessions` line of the note. The directory
/// comes from a `cd <dir> &&` in front of it, if there is one.
pub(super) fn parse_session_id(input: &str) -> Option<(String, Option<PathBuf>)> {
    let mut words = input.split_whitespace().map(|w| w.trim_matches('`'));
    let word = if input.contains("--resume") {
        words.skip_while(|w| *w != "--resume").nth(1)?
    } else {
        let w = words.next()?;
        if words.next().is_some() {
            return None;
        }
        w
    };
    // Claude's ids are hyphenated; the other forms the uuid crate takes aren't ids.
    let sid = (word.len() == 36).then(|| uuid::Uuid::try_parse(word).ok()).flatten()?.hyphenated().to_string();
    let dir = input.split_once("&&").and_then(|(before, _)| {
        let (_, dir) = before.rsplit_once("cd ")?;
        match shell_words::split(dir).ok()?.as_slice() {
            [d] => Some(PathBuf::from(d)),
            _ => None,
        }
    });
    Some((sid, dir))
}

impl App {
    /// Open tabs asked for with `salash tab new`, in the background.
    pub(super) fn poll_requests(&mut self) {
        for (id, req) in api::take_requests() {
            let reply = match req {
                Ok(req) => self.open_requested(req),
                Err(e) => Reply::err(format!("{e:#}")),
            };
            api::reply(&id, &reply);
        }
    }

    pub fn open_requested(&mut self, req: Request) -> Reply {
        if let Err(e) = req.check() {
            return Reply::err(format!("{e:#}"));
        }
        if req.issue == SCRATCH {
            if req.kind == TabKind::Notes {
                return Reply::err("scratch has no note");
            }
        } else if !self.notes.iter().any(|n| n.id == req.issue) {
            return Reply::err(format!("no issue {}", req.issue));
        } else if self.retired(&req.issue) {
            return Reply::err(format!("{} is finished (Done or Archive): Move to Active first (u)", req.issue));
        }
        let args = if req.kind == TabKind::Claude { req.claude_args() } else { vec![] };
        let tab = self.add_tab(&req.issue, req.kind, req.name.clone(), req.cwd.clone(), args, false);
        let rt = self.rt_for(&req.issue);
        // A tab waiting for its worktree starts once it exists.
        let waiting = rt.creating.as_ref().is_some_and(|j| j.waiting.contains(&tab));
        let Some(t) = rt.tabs.iter().find(|t| t.st.id == tab) else {
            return Reply::err("tab closed");
        };
        if t.pane.is_none() && !waiting {
            return Reply::err("tab didn't start, see salash");
        }
        let name = t.st.name.clone();
        self.flash(format!("New tab \"{name}\" in {}", req.issue));
        Reply::ok(tab)
    }

    /// Starting a Claude session on a To-do issue moves it to Active. Runs
    /// before the pane spawns so SALASH_NOTE points at the moved file.
    pub(super) fn start_work(&mut self, id: &str) {
        let Some(note) = self.selected_note().filter(|n| n.id == id && n.stage == Stage::Todo).cloned() else {
            return;
        };
        match self.vault.move_to(&note, Stage::Active) {
            Ok(_) => {
                self.rescan();
                self.view = Stage::Active;
                self.flash(format!("{} → {}", note.id, Stage::Active.label()));
            }
            Err(e) => self.flash(format!("Move failed: {e:#}")),
        }
    }

    /// Start a Claude tab in a typed directory; empty means the issue's default.
    pub(super) fn new_claude_in(&mut self, input: String) {
        let dir = input.trim();
        if dir.is_empty() {
            self.new_tab(TabKind::Claude);
        } else if let Some(p) = resolve_dir(dir) {
            self.msg = None;
            self.new_tab_in(TabKind::Claude, Some(p));
        } else {
            self.flash(format!("Not a directory: {dir}"));
            self.mode = Mode::Prompt(PromptKind::ClaudeDir, input);
        }
    }

    /// Open a Claude tab on the selected issue running `claude --resume <id>`,
    /// where `c` would start one, or in the directory a pasted `cd` names.
    /// A session already in a tab, in any issue, is only shown, and one
    /// closed on this issue comes back as it was.
    pub(super) fn resume_session(&mut self, input: String) {
        let Some((sid, dir)) = parse_session_id(&input) else {
            self.flash(format!("Not a session ID: {}", input.trim()));
            self.mode = Mode::Prompt(PromptKind::ResumeId, input);
            return;
        };
        let Some(id) = self.selected.clone() else { return };
        if self.refuse_retired(&id) {
            return;
        }
        if let Some((issue, idx)) = self.find_session(&sid) {
            if issue != id {
                if let Some(stage) = self.notes.iter().find(|n| n.id == issue).map(|n| n.stage) {
                    self.view = stage;
                }
                self.select_id(issue.clone());
            }
            self.switch_tab(idx);
            self.view_tab();
            self.flash(if issue == id { "That session is open in this tab".into() } else { format!("That session is open in {issue}") });
            return;
        }
        self.msg = None;
        self.start_work(&id);
        let rt = self.rt_for(&id);
        // A closed tab comes back as it was: its name, directory and saved screen.
        if let Some(i) = rt.closed.iter().position(|st| st.claude_session.as_deref() == Some(sid.as_str())) {
            rt.tabs.push(Tab::new(rt.closed.remove(i)));
            rt.active = rt.tabs.len() - 1;
            let idx = rt.active;
            self.persist(&id);
            self.spawn_pane(&id, idx);
        } else {
            // A new tab as `c` opens, so it starts in the same directory, and
            // its first hook records the session and writes its Sessions line.
            let cwd = dir.filter(|d| d.is_dir());
            let tab = self.add_tab(&id, TabKind::Claude, None, cwd, vec!["--resume".into(), sid.clone()], true);
            if let Some(t) = self.rt_for(&id).tabs.iter_mut().find(|t| t.st.id == tab) {
                t.resuming = Some(sid);
            }
        }
        self.focus = Focus::Terminal;
    }

    /// The issue and tab holding a Claude session, saved tabs of issues not
    /// loaded yet included.
    fn find_session(&mut self, sid: &str) -> Option<(String, usize)> {
        let saved: Vec<String> = self
            .state
            .issues
            .iter()
            .filter(|(id, s)| !self.rt.contains_key(*id) && s.tabs.iter().any(|t| t.claude_session.as_deref() == Some(sid)))
            .map(|(id, _)| id.clone())
            .collect();
        for id in saved {
            self.rt_for(&id);
        }
        self.rt.iter().find_map(|(id, rt)| Some((id.clone(), rt.tabs.iter().position(|t| t.has_session(sid))?)))
    }

    /// Save a typed directory as the app's default; empty keeps the current one.
    /// Tabs that already exist stay where they are.
    pub(super) fn set_default_dir(&mut self, kind: PromptKind, input: String) {
        let dir = input.trim();
        if dir.is_empty() {
            return;
        }
        if let Some(p) = resolve_dir(dir) {
            self.flash(format!("Default directory: {}", crate::config::shorten(&p)));
            self.state.default_dir = Some(p);
            self.state.save();
        } else {
            self.flash(format!("Not a directory: {dir}"));
            self.mode = Mode::Prompt(kind, input);
        }
    }

    /// Open the issue: focus the terminal, starting/resuming the active tab.
    pub(super) fn open_issue(&mut self) {
        let Some(id) = self.selected.clone() else { return };
        let rt = self.rt_for(&id);
        if rt.tabs.is_empty() {
            self.new_tab(TabKind::Claude);
            return;
        }
        let idx = rt.active;
        if rt.tabs[idx].pane.is_none() {
            self.spawn_pane(&id, idx);
        }
        self.focus = Focus::Terminal;
    }

    /// Show the active tab after moving to it. A suspended tab stays stopped,
    /// so it can be read without starting Claude.
    pub(super) fn view_tab(&mut self) {
        if self.active_tab().is_some_and(Tab::suspended) {
            self.focus = Focus::Terminal;
        } else {
            self.open_issue();
        }
    }

    pub(super) fn switch_tab(&mut self, idx: usize) {
        let Some(id) = self.selected.clone() else { return };
        let rt = self.rt_for(&id);
        if idx < rt.tabs.len() {
            rt.active = idx;
            self.persist(&id);
        }
    }

    pub(super) fn cycle_tab(&mut self, delta: isize) {
        let Some(rt) = self.current_rt() else { return };
        let n = rt.tabs.len() as isize;
        if n == 0 {
            return;
        }
        let next = ((rt.active as isize + delta).rem_euclid(n)) as usize;
        self.switch_tab(next);
    }

    pub(super) fn close_tab(&mut self) {
        let Some(id) = self.selected.clone() else { return };
        let idx = self.rt_for(&id).active;
        self.close_tab_at(&id, idx);
    }

    /// Close a tab. A Claude session that can be resumed is kept for `R`.
    /// Returns whether it was.
    pub(super) fn close_tab_at(&mut self, id: &str, idx: usize) -> bool {
        let rt = self.rt_for(id);
        if idx >= rt.tabs.len() {
            return false;
        }
        let mut t = rt.remove_tab(idx);
        let reopenable = t.st.kind == TabKind::Claude && t.resumable();
        if reopenable {
            t.save_screen();
        }
        if let Some(p) = t.pane.as_mut() {
            p.kill();
        }
        status::clear_status(&t.st.id);
        if reopenable {
            rt.push_closed(t.st);
        } else {
            state::remove_snapshot(&t.st.id);
        }
        let emptied = rt.tabs.is_empty() && self.selected.as_deref() == Some(id);
        self.persist(id);
        if emptied {
            self.focus = Focus::Sidebar;
            // An empty Scratch hides, so move to the first issue.
            if id == SCRATCH {
                self.selected = self.visible().first().map(|n| n.id.clone());
                if let Some(next) = self.selected.clone() {
                    self.select_id(next);
                }
            }
        }
        reopenable
    }

    /// Bring back the most recently closed Claude tab, still suspended.
    pub(super) fn reopen_tab(&mut self) {
        let Some(id) = self.selected.clone() else { return };
        if self.refuse_retired(&id) {
            return;
        }
        let rt = self.rt_for(&id);
        let Some(st) = rt.closed.pop() else {
            self.flash("No closed Claude tabs for this issue".into());
            return;
        };
        rt.tabs.push(Tab::new(st));
        rt.active = rt.tabs.len() - 1;
        self.persist(&id);
        self.focus = Focus::Terminal;
    }

    /// Save a tab's screen and stop its process, leaving it suspended.
    fn park(&mut self, id: &str, idx: usize) {
        let Some(t) = self.rt.get_mut(id).and_then(|rt| rt.tabs.get_mut(idx)) else { return };
        t.save_screen();
        t.pane = None;
        t.esc_ts = None;
        t.exit_check = None;
        self.persist(id);
    }

    pub(super) fn request_suspend(&mut self) {
        let Some(t) = self.active_tab() else { return };
        if t.st.kind != TabKind::Claude {
            self.flash("Only Claude tabs can be suspended".into());
        } else if !t.running() {
            self.flash("This tab isn't running".into());
        } else if !t.resumable() {
            self.flash("Nothing to resume yet: this session has no messages".into());
        } else if t.busy() || t.status == ClaudeStatus::Starting {
            self.mode = Mode::Confirm(ConfirmKind::Suspend);
        } else {
            self.suspend_tab();
        }
    }

    pub(super) fn suspend_tab(&mut self) {
        let Some(id) = self.selected.clone() else { return };
        let idx = self.rt_for(&id).active;
        self.park(&id, idx);
        self.flash("Suspended: process stopped, ⏎ resumes".into());
    }

    /// Before quitting: keep Claude tabs readable, and don't leave a session
    /// id behind that `--resume` can't find.
    pub fn save_sessions(&mut self) {
        let ids: Vec<String> = self.rt.keys().cloned().collect();
        for id in ids {
            let rt = self.rt_for(&id);
            for t in rt.tabs.iter_mut().filter(|t| t.st.kind == TabKind::Claude) {
                if t.pane.is_none() {
                    continue;
                }
                if t.resumable() {
                    t.save_screen();
                } else {
                    t.st.claude_session = None;
                }
            }
            self.persist(&id);
        }
    }

    pub(super) fn active_tab(&self) -> Option<&Tab> {
        let rt = self.current_rt()?;
        rt.tabs.get(rt.active)
    }

    pub(super) fn can_focus_terminal(&self) -> bool {
        self.active_tab().is_some_and(|t| t.pane.is_some() || t.suspended())
    }

    pub(super) fn scroll_active(&mut self, delta: isize) {
        if let Some(p) = self.active_pane_mut() {
            p.scroll_by(delta);
        } else if let Some((_, s)) = self.viewer.as_mut() {
            s.scroll_by(delta);
        }
    }

    pub(super) fn scroll_page(&mut self, up: bool) {
        let lines = (self.term_rect.height as isize / 2).max(1);
        if let Some(p) = self.active_pane_mut() {
            p.scroll_page(up, lines);
        } else {
            self.scroll_active(if up { lines } else { -lines });
        }
    }

    /// Load the saved screen of the suspended tab on view, and only that one.
    pub fn sync_viewer(&mut self) {
        let rows = self.term_rect.height.saturating_sub(1);
        let Some(tab) = self.active_tab().filter(|t| t.suspended()).map(|t| t.st.id.clone()) else {
            self.viewer = None;
            return;
        };
        if self.viewer.as_ref().is_some_and(|(id, s)| *id == tab && s.rows == rows) {
            return;
        }
        self.viewer = Snapshot::load(&state::snapshot_path(&tab), rows).map(|s| (tab, s));
    }

    pub(super) fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let rt = self.rt.get_mut(self.selected.as_ref()?)?;
        rt.tabs.get_mut(rt.active)
    }

    pub(super) fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        self.active_tab_mut()?.pane.as_mut()
    }
}
