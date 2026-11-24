//! The issue list: filtering, sorting, selection, stage moves, retiring tabs, sync.

use super::types::{activity, scratch_shown};
use super::*;

impl App {
    // ---------- issues ----------

    /// Issues of the current stage, in the chosen order. `notes` is newest-created
    /// first, and the sorts are stable, so that breaks ties. Reversing flips the
    /// key only: ties stay newest-created first, and undated issues stay last.
    pub fn visible(&self) -> Vec<&Note> {
        let mut v: Vec<&Note> = self.notes.iter().filter(|n| n.stage == self.view).collect();
        let rev = self.state.sort_reversed;
        let dir = |o: std::cmp::Ordering| if rev { o.reverse() } else { o };
        match self.state.sort {
            SortMode::Created => {
                if rev {
                    v.reverse();
                }
            }
            SortMode::Id => v.sort_by(|a, b| dir(natord::compare_ignore_case(&a.id, &b.id))),
            SortMode::Name => v.sort_by(|a, b| dir(a.title.to_lowercase().cmp(&b.title.to_lowercase()))),
            SortMode::Moved => {
                let since = |n: &Note| n.moved.clone().or_else(|| n.created.clone());
                v.sort_by(|a, b| {
                    let (a, b) = (since(a), since(b));
                    a.is_none().cmp(&b.is_none()).then_with(|| dir(b.cmp(&a)))
                });
            }
            SortMode::Activity => {
                let mut ranked: Vec<(u8, &Note)> = v.into_iter().map(|n| (self.activity_of(&n.id), n)).collect();
                ranked.sort_by(|a, b| dir(b.0.cmp(&a.0)));
                v = ranked.into_iter().map(|(_, n)| n).collect();
            }
        }
        v
    }

    /// The sort as the sidebar header shows it, with an arrow for its direction.
    pub fn sort_label(&self) -> String {
        let up = self.state.sort.ascending() != self.state.sort_reversed;
        format!("{} {}", if up { "↑" } else { "↓" }, self.state.sort.label())
    }

    pub(super) fn flip_sort(&mut self) {
        self.state.sort_reversed = !self.state.sort_reversed;
        self.state.save();
        self.flash(format!("Sort: {}", self.sort_label()));
    }

    /// The most urgent activity among the issue's tabs. Tabs of an issue not
    /// loaded yet have no process.
    fn activity_of(&self, id: &str) -> u8 {
        match self.rt.get(id) {
            Some(rt) => rt
                .tabs
                .iter()
                .map(|t| activity(t.st.kind, t.running().then(|| self.ranked_status(id, t))))
                .max(),
            None => self.state.issues.get(id).and_then(|s| s.tabs.iter().map(|t| activity(t.kind, None)).max()),
        }
        .unwrap_or(0)
    }

    /// Status a tab ranks by in the activity sort: as shown, except that a
    /// Done seen while its issue stayed selected still ranks as Done.
    pub(super) fn ranked_status(&self, id: &str, t: &Tab) -> ClaudeStatus {
        match &self.held {
            Some((h, since)) if h == id && t.status == ClaudeStatus::Done && t.seen_ts >= *since => ClaudeStatus::Done,
            _ => t.shown_status(),
        }
    }

    /// Scratch shows while it has tabs, or while it's selected so a first tab can be opened.
    pub fn shows_scratch(&self) -> bool {
        scratch_shown(self.saved_tabs(SCRATCH), self.scratch_selected())
    }

    pub fn scratch_selected(&self) -> bool {
        self.selected.as_deref() == Some(SCRATCH)
    }

    /// Ids of the sidebar rows: Scratch first in every view and sort, then the issues.
    pub fn rows(&self) -> Vec<String> {
        let scratch = self.shows_scratch().then(|| SCRATCH.to_string());
        scratch.into_iter().chain(self.visible().iter().map(|n| n.id.clone())).collect()
    }

    pub fn selected_note(&self) -> Option<&Note> {
        let id = self.selected.as_ref()?;
        self.notes.iter().find(|n| &n.id == id)
    }

    pub fn count(&self, st: Stage) -> usize {
        self.notes.iter().filter(|n| n.stage == st).count()
    }

    pub(super) fn rescan(&mut self) {
        self.notes = self.vault.scan();
        if self.selected_note().is_none() && !self.scratch_selected() {
            self.selected = self.rows().first().cloned();
        }
        self.retire_moved();
        self.last_scan = Instant::now();
    }

    /// The issue is in Done or Archive. Scratch never is.
    pub(super) fn retired(&self, id: &str) -> bool {
        self.notes.iter().any(|n| n.id == id && retires(n.stage))
    }

    /// Say so and return true if the issue is in Done or Archive, where no tab opens.
    pub(super) fn refuse_retired(&mut self, id: &str) -> bool {
        let retired = self.retired(id);
        if retired {
            self.flash("Move to Active first (u)".into());
        }
        retired
    }

    /// The issue has tabs or closed tabs, loaded or only saved.
    fn has_tabs(&self, id: &str) -> bool {
        match self.rt.get(id) {
            Some(rt) => !rt.tabs.is_empty() || !rt.closed.is_empty(),
            None => self.state.issues.get(id).is_some_and(|s| !s.tabs.is_empty() || !s.closed.is_empty()),
        }
    }

    /// Stop the tabs of issues moved to Done or Archive outside salash (in
    /// Obsidian, or before salash did it on a move). Nobody confirmed that,
    /// so busy tabs keep running, and a later rescan stops them once idle.
    fn retire_moved(&mut self) {
        let notes: Vec<Note> = self.notes.iter().filter(|n| retires(n.stage) && self.has_tabs(&n.id)).cloned().collect();
        self.busy_left.retain(|id, _| notes.iter().any(|n| &n.id == id));
        self.write_failed.retain(|id| notes.iter().any(|n| &n.id == id));
        let (mut touched_selected, mut said, mut failed) = (false, vec![], false);
        for note in notes {
            let (stopped, busy, error) = self.retire_tabs(&note, false);
            touched_selected |= stopped > 0 && self.selected.as_deref() == Some(note.id.as_str());
            let stage = note.stage.label();
            if let Some(e) = error {
                if self.write_failed.insert(note.id.clone()) {
                    self.flash_error(not_written(&note.id, &e));
                    failed = true;
                }
                continue;
            }
            self.write_failed.remove(&note.id);
            if busy.is_empty() {
                self.busy_left.remove(&note.id);
                if stopped > 0 {
                    said.push(format!("{} is in {stage}: {} stopped", note.id, tabs_n(stopped, "")));
                }
            } else if self.busy_left.get(&note.id) != Some(&busy) {
                said.push(format!(
                    "{} is in {stage}: left {} running ({}), stopped once idle",
                    note.id,
                    tabs_n(busy.len(), "busy "),
                    busy.join(", ")
                ));
                self.busy_left.insert(note.id.clone(), busy);
            }
        }
        // One message for all, so one issue's doesn't hide another's; a failure goes first.
        if !said.is_empty() && !failed {
            self.flash(said.join("; "));
        }
        // Only when a tab on view went; a tab waiting for its worktree keeps focus.
        if touched_selected {
            self.after_issue_switch();
        }
    }

    /// Stop an issue's tabs as it goes to Done or Archive. First each Claude
    /// session, closed ones included, gets a line in the note to resume it by;
    /// then saved screens and the closed tabs go too. Unless `force`, busy tabs
    /// keep running. If the lines can't be written, or a kept note editor
    /// would save over them, the sessions are kept as closed tabs instead, for
    /// `R` and a later try. Returns how many tabs stopped, what keeps the rest
    /// busy, and why writing the note failed.
    fn retire_tabs(&mut self, note: &Note, force: bool) -> (usize, Vec<String>, Option<String>) {
        let rt = self.rt_for(&note.id);
        let active = rt.tabs.get(rt.active).map(|t| t.st.id.clone());
        let (mut keep, mut stop, mut busy) = (vec![], vec![], vec![]);
        for t in std::mem::take(&mut rt.tabs) {
            match t.busy_label().filter(|_| !force) {
                Some(b) => {
                    busy.push(b);
                    keep.push(t);
                }
                None => stop.push(t),
            }
        }
        let mut closed = std::mem::take(&mut rt.closed);
        let had_closed = !closed.is_empty();
        let worktree = rt.worktree.clone();
        // Read before the status files go: a session with no messages can't be resumed.
        let (sessions, others): (Vec<Tab>, Vec<Tab>) =
            stop.into_iter().partition(|t| t.st.kind == TabKind::Claude && t.resumable());
        let stopped = sessions.len() + others.len();

        let when = chrono::Local::now().format("%Y-%m-%d %H:%M");
        let claude = self.claude_words();
        let mut seen = HashSet::new();
        let mut lines = vec![];
        for st in sessions.iter().map(|t| &t.st).chain(&closed) {
            let Some(sid) = st.claude_session.as_deref().filter(|s| seen.insert(s.to_string())) else { continue };
            // A tab started in the worktree keeps no directory of its own.
            let cmd = match st.cwd.as_ref().or(worktree.as_ref()) {
                Some(dir) => format!("cd {} && {claude} --resume {sid}", shell_word(&dir.to_string_lossy())),
                None => format!("{claude} --resume {sid}"),
            };
            lines.push(format!("- {when} · stopped · {} · `{cmd}`", st.name));
        }
        let editor_open = keep.iter().any(|t| t.st.kind == TabKind::Notes);
        let mut error = None;
        let written = !editor_open
            && (lines.is_empty()
                || match self.vault.append_session_lines(note, &lines) {
                    Ok(()) => true,
                    Err(e) => {
                        error = Some(format!("{e:#}"));
                        false
                    }
                });
        if written {
            for st in &closed {
                state::remove_snapshot(&st.id);
            }
            closed.clear();
        }
        for t in &sessions {
            if written {
                state::remove_snapshot(&t.st.id);
            } else {
                t.save_screen();
            }
            status::clear_status(&t.st.id);
        }
        for t in &others {
            status::clear_status(&t.st.id);
            state::remove_snapshot(&t.st.id);
        }
        if !written {
            // Not trimmed to MAX_CLOSED: none of these is in the note yet.
            closed.extend(sessions.iter().map(|t| t.st.clone()));
        }
        let changed = stopped > 0 || written && had_closed;
        let rt = self.rt_for(&note.id);
        rt.tabs = keep;
        rt.active = active.and_then(|a| rt.tabs.iter().position(|t| t.st.id == a)).unwrap_or(0);
        rt.closed = closed;
        if changed {
            self.persist(&note.id);
        }
        (stopped, busy, error)
    }

    pub(super) fn select_id(&mut self, id: String) {
        // Load saved tabs (without starting them) so the tab bar shows them.
        self.rt_for(&id);
        self.selected = Some(id);
        self.state.selected = self.selected.clone();
        self.state.save();
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        let ids = self.rows();
        if ids.is_empty() {
            return;
        }
        let cur = self
            .selected
            .as_ref()
            .and_then(|s| ids.iter().position(|i| i == s))
            .unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, ids.len() as isize - 1) as usize;
        self.select_id(ids[next].clone());
    }

    pub(super) fn set_view(&mut self, st: Stage) {
        self.view = st;
        // Scratch is in every view, so it stays selected.
        if !self.scratch_selected() && self.selected_note().map(|n| n.stage) != Some(st) {
            if let Some(id) = self.rows().first().cloned() {
                self.select_id(id);
            } else {
                self.selected = None;
            }
        }
    }

    /// Move the selected issue. Done and Archive stop its tabs, asking first
    /// if any of them is busy.
    pub(super) fn move_stage(&mut self, st: Stage) {
        let Some(id) = self.selected_note().map(|n| n.id.clone()) else { return };
        if retires(st) {
            let busy: Vec<String> = self.rt_for(&id).tabs.iter().filter_map(Tab::busy_label).collect();
            if !busy.is_empty() {
                self.mode = Mode::Confirm(ConfirmKind::Retire { issue: id, stage: st, busy });
                return;
            }
        }
        self.do_move_stage(&id, st);
    }

    pub(super) fn do_move_stage(&mut self, id: &str, st: Stage) {
        let Some(note) = self.notes.iter().find(|n| n.id == id).cloned() else { return };
        // Pick the neighbour before moving, so the cursor stays in place.
        let ids = self.rows();
        let pos = ids.iter().position(|i| i == &note.id).unwrap_or(0);
        match self.vault.move_to(&note, st) {
            Ok(moved) => {
                let mut msg = format!("{} → {}", note.id, st.label());
                // Before the rescan, which would leave busy tabs running.
                let mut error = None;
                if retires(st) {
                    let (stopped, _, e) = self.retire_tabs(&moved, true);
                    if stopped > 0 {
                        msg.push_str(&format!(" · {} stopped", tabs_n(stopped, "")));
                    }
                    error = e;
                }
                match error {
                    Some(e) => {
                        // The rescan below tries again; it needn't say so twice.
                        self.write_failed.insert(id.to_string());
                        self.flash_error(format!("{msg} · {}", not_written(id, &e)));
                    }
                    None => self.flash(msg),
                }
                self.rescan();
                let next = ids
                    .get(pos + 1)
                    .or_else(|| pos.checked_sub(1).and_then(|p| ids.get(p)))
                    .cloned();
                if let Some(id) = next {
                    self.select_id(id);
                } else {
                    self.selected = None;
                }
                self.focus = Focus::Sidebar;
            }
            Err(e) => self.flash(format!("Move failed: {e:#}")),
        }
    }

    pub(super) fn new_local_issue(&mut self, title: String) {
        let title = title.trim().to_string();
        if title.is_empty() {
            return;
        }
        let id = self.vault.next_local_id(&self.cfg.local_prefix);
        match self.vault.create(NewNote { id: &id, title: &title, source: "local", url: None, repo: None, stage: Stage::Todo }) {
            Ok((n, _)) => {
                self.rescan();
                self.view = Stage::Todo;
                self.select_id(n.id.clone());
                self.flash(format!("Created {}", n.path.display()));
            }
            Err(e) => self.flash(format!("Create failed: {e:#}")),
        }
    }

    pub(super) fn open_obsidian(&mut self) {
        let Some(n) = self.selected_note() else { return };
        let url = format!(
            "obsidian://open?path={}",
            urlencoding::encode(&n.path.to_string_lossy())
        );
        self.outbox.push(ServerMsg::Open(url));
    }

    // ---------- sync ----------

    pub fn start_sync(&mut self) {
        if self.syncing {
            return;
        }
        let providers = providers::from_config(&self.cfg);
        if providers.is_empty() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(providers::fetch_all(&providers));
        });
        self.sync_rx = Some(rx);
        self.syncing = true;
    }

    pub(super) fn poll_sync(&mut self) {
        let Some(rx) = &self.sync_rx else { return };
        let Ok(rep) = rx.try_recv() else { return };
        self.sync_rx = None;
        self.syncing = false;
        let mut created = 0;
        for i in &rep.fetched {
            let r = self.vault.create(NewNote {
                id: &i.id,
                title: &i.title,
                source: i.source,
                url: i.url.as_deref(),
                repo: i.repo.as_deref(),
                stage: Stage::Todo,
            });
            if matches!(r, Ok((_, true))) {
                created += 1;
            }
        }
        self.rescan();
        let mut m = format!("Synced {} issues, {} new", rep.fetched.len(), created);
        if !rep.errors.is_empty() {
            m.push_str(&format!(" — {}", rep.errors.join("; ")));
        }
        self.flash(m);
    }
}

/// Issues in these stages are finished: their tabs are stopped, and no new ones open.
fn retires(st: Stage) -> bool {
    matches!(st, Stage::Done | Stage::Archive)
}

/// Why an issue's stopped sessions are still in salash and not in its note.
fn not_written(id: &str, e: &str) -> String {
    format!("{id}: resume commands not written to the note, sessions kept for R: {e}")
}
