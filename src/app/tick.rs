//! Periodic work: status polling, marking tabs seen, the bell, resizing.

use super::types::dwell;
use super::*;

impl App {
    /// Mark the tab on view as seen once it has stayed on view for
    /// `seen_after_secs`, so flicking past a finished tab doesn't count as
    /// reading it. Switching issue or tab changes the id on view, which
    /// restarts the timer; so does an overlay covering the pane.
    pub(super) fn mark_seen(&mut self, now: Instant) {
        if self.held.as_ref().map(|(id, _)| id) != self.selected.as_ref() {
            self.held = self.selected.clone().map(|id| (id, now_ms()));
        }
        let covered = !matches!(self.mode, Mode::Normal | Mode::Prefix);
        let tab = if covered { None } else { self.active_tab().map(|t| t.st.id.clone()) };
        if dwell(&mut self.on_view, tab.as_deref(), now, Duration::from_secs(self.cfg.seen_after_secs)) {
            if let Some(t) = self.active_tab_mut() {
                t.seen_ts = now_ms();
            }
        }
    }

    // ---------- periodic work ----------

    pub fn tick(&mut self) {
        self.poll_sync();
        self.autoscroll_select();
        if self.last_spin.elapsed() >= Duration::from_millis(120) {
            self.spinner = self.spinner.wrapping_add(1);
            self.last_spin = Instant::now();
        }
        if self.last_scan.elapsed() >= Duration::from_secs(3) {
            // Picks up notes moved/renamed/created in Obsidian.
            self.rescan();
        }
        self.poll_worktrees();
        if let Some((_, t)) = &self.msg {
            if t.elapsed() > Duration::from_secs(if self.msg_error { 15 } else { 6 }) {
                self.msg = None;
            }
        }

        self.mark_seen(Instant::now());
        let now = now_ms();

        if self.last_status.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.last_status = Instant::now();
        self.poll_requests();
        let selected = self.selected.clone();
        let mut new_sessions: Vec<(String, String, String)> = vec![];
        let mut renamed: Vec<String> = vec![];
        // Tabs whose process exited, to close. As in tmux with `remain-on-exit`
        // off, only a Claude that failed to start stays, to show why.
        let mut ended: Vec<(String, usize)> = vec![];
        for (issue, rt) in self.rt.iter_mut() {
            let active = rt.active;
            for (i, t) in rt.tabs.iter_mut().enumerate() {
                if t.pane.is_none() {
                    continue;
                }
                let visible = selected.as_deref() == Some(issue.as_str()) && i == active;
                let before = t.shown_status();
                let (was_exited, exited, title, bell) = {
                    let p = t.pane.as_mut().unwrap();
                    (p.exited.is_some(), p.poll_exit(), p.title(), p.take_bell())
                };
                let claude = t.st.kind == TabKind::Claude;
                if t.st.kind == TabKind::Shell && !exited {
                    t.observe_command(Instant::now());
                }
                if claude && !was_exited {
                    if let Some(sid) = t.sync_status() {
                        new_sessions.push((issue.clone(), sid, t.st.name.clone()));
                    }
                }
                if exited {
                    if !claude && !was_exited {
                        ended.push((issue.clone(), i));
                    } else if claude && !was_exited {
                        t.exit_check = Some(t.status == ClaudeStatus::Starting);
                    } else if let Some(no_hooks) = t.exit_check.take() {
                        // A poll later, so the reader has the last of the output.
                        let hint = t.pane.as_ref().and_then(Pane::resume_id);
                        if let (None, Some(sid)) = (&t.st.claude_session, &hint) {
                            t.st.claude_session = Some(sid.clone());
                            new_sessions.push((issue.clone(), sid.clone(), t.st.name.clone()));
                        }
                        if !t.resumable() {
                            t.st.claude_session = None;
                            renamed.push(issue.clone());
                        }
                        if !no_hooks || hint.is_some() {
                            ended.push((issue.clone(), i));
                        }
                    }
                    t.status = ClaudeStatus::Exited;
                } else if claude {
                    if t.take_interrupt(now) {
                        status::mark_interrupted(&t.st.id);
                    }
                    if !t.st.custom_name {
                        if let Some(title) = title {
                            if title != "Claude Code" && title != t.st.name {
                                t.st.name = title;
                                renamed.push(issue.clone());
                            }
                        }
                    }
                }
                let after = t.shown_status();
                let changed_to_notice = after != before
                    && matches!(after, ClaudeStatus::Attention | ClaudeStatus::Done);
                if !visible && (changed_to_notice || bell) {
                    self.bell_pending = true;
                }
            }
        }
        for (issue, sid, name) in new_sessions {
            if let Some(note) = self.notes.iter().find(|n| n.id == issue).cloned() {
                let when = chrono::Local::now().format("%Y-%m-%d %H:%M");
                let claude = self.claude_words();
                let _ = self.vault.append_session_lines(&note, &[format!("- {when} · {name} · `{claude} --resume {sid}`")]);
            }
            self.persist(&issue);
        }
        renamed.sort();
        renamed.dedup();
        for issue in renamed {
            self.persist(&issue);
        }
        // Highest index first, so the ones still to close keep theirs.
        ended.sort_by_key(|e| std::cmp::Reverse(e.1));
        for (issue, idx) in ended {
            if self.close_tab_at(&issue, idx) && selected.as_deref() == Some(issue.as_str()) {
                self.flash("Claude exited, tab closed: R reopens it".into());
            }
        }
    }

    /// True if the clients' terminals should ring.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell_pending) && self.cfg.bell
    }

    /// Keep panes of the visible issue sized to the terminal area.
    pub fn resize_visible(&mut self) {
        let r = self.term_rect;
        if r.width == 0 || r.height == 0 {
            return;
        }
        if let Some(id) = self.selected.clone() {
            if let Some(rt) = self.rt.get_mut(&id) {
                for t in rt.tabs.iter_mut() {
                    if let Some(p) = t.pane.as_mut() {
                        p.resize(r.height, r.width);
                    }
                }
            }
        }
    }

    pub fn running_claude_tabs(&self) -> usize {
        self.rt
            .values()
            .flat_map(|r| r.tabs.iter())
            .filter(|t| t.st.kind == TabKind::Claude && t.running())
            .count()
    }

    pub fn suspended_claude_tabs(&self) -> usize {
        let loaded = self.rt.values().flat_map(|r| r.tabs.iter()).filter(|t| t.suspended()).count();
        let saved = self
            .state
            .issues
            .iter()
            .filter(|(id, _)| !self.rt.contains_key(*id))
            .flat_map(|(_, s)| s.tabs.iter())
            .filter(|t| t.kind == TabKind::Claude && t.claude_session.is_some())
            .count();
        loaded + saved
    }
}
