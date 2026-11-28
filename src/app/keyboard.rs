//! Key routing: modes, terminal, sidebar, command and prefix keys.

use super::*;

impl App {
    pub(super) fn on_key(&mut self, k: KeyEvent) {
        match self.mode.clone() {
            Mode::Help => {
                self.mode = Mode::Normal;
            }
            Mode::Confirm(kind) => {
                self.mode = Mode::Normal;
                if matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter) {
                    match kind {
                        ConfirmKind::Quit => self.quit = true,
                        ConfirmKind::CloseTab => self.close_tab(),
                        ConfirmKind::Suspend => self.suspend_tab(),
                        ConfirmKind::Retire { issue, stage, .. } => self.do_move_stage(&issue, stage),
                    }
                }
            }
            Mode::Prompt(kind, mut buf) => {
                let mut completion = self.completion.take();
                match k.code {
                    KeyCode::Esc => self.mode = Mode::Normal,
                    KeyCode::Enter => {
                        self.mode = Mode::Normal;
                        match kind {
                            PromptKind::NewIssue => self.new_local_issue(buf),
                            PromptKind::RenameTab => self.rename_tab(buf),
                            PromptKind::ClaudeDir => self.new_claude_in(buf),
                            PromptKind::DefaultDir => self.set_default_dir(kind, buf),
                            PromptKind::ResumeId => self.resume_session(buf),
                        }
                    }
                    KeyCode::Tab | KeyCode::BackTab if kind.is_dir() => {
                        if !complete::tab(&mut buf, &mut completion, k.code == KeyCode::Tab) {
                            self.flash("No matching directories".into());
                        }
                        self.completion = completion;
                        self.mode = Mode::Prompt(kind, buf);
                    }
                    KeyCode::Backspace => {
                        buf.pop();
                        self.mode = Mode::Prompt(kind, buf);
                    }
                    KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                        // A selected match already ends in a separator.
                        let cycling = completion.is_some_and(|c| c.selected.is_some());
                        if !(cycling && std::path::is_separator(c)) {
                            buf.push(c);
                        }
                        self.mode = Mode::Prompt(kind, buf);
                    }
                    _ => {}
                }
            }
            Mode::Prefix => {
                self.mode = Mode::Normal;
                self.on_prefix_command(k);
            }
            Mode::Normal => {
                if keys::is_prefix(&k, &self.prefix) {
                    self.mode = Mode::Prefix;
                    return;
                }
                match self.focus {
                    Focus::Sidebar => self.on_sidebar_key(k),
                    Focus::Terminal => self.on_terminal_key(k),
                }
            }
        }
    }

    fn on_terminal_key(&mut self, k: KeyEvent) {
        let Some(id) = self.selected.clone() else { return };
        let rt = self.rt_for(&id);
        let idx = rt.active;
        let Some(tab) = rt.tabs.get_mut(idx) else {
            if k.code == KeyCode::Enter {
                self.open_issue();
            }
            return;
        };
        if tab.suspended() {
            let page = (self.term_rect.height as isize / 2).max(1);
            match k.code {
                KeyCode::Enter => self.spawn_pane(&id, idx),
                KeyCode::Up => self.scroll_active(1),
                KeyCode::Down => self.scroll_active(-1),
                KeyCode::PageUp => self.scroll_active(page),
                KeyCode::PageDown => self.scroll_active(-page),
                KeyCode::Char('g') | KeyCode::Home => self.scroll_active(isize::MAX / 2),
                KeyCode::Char('G') | KeyCode::End => self.scroll_active(isize::MIN / 2),
                _ => {}
            }
            return;
        }
        if k.code == KeyCode::Esc && tab.st.kind == TabKind::Claude && tab.busy() {
            tab.esc_ts = Some(now_ms());
        }
        match tab.pane.as_mut() {
            Some(p) if p.exited.is_none() => {
                let bytes = keys::encode(&k, p.application_cursor());
                if !bytes.is_empty() {
                    p.write(&bytes);
                    // Typing into the tab means you've read it.
                    tab.seen_ts = now_ms();
                }
            }
            _ => {
                if k.code == KeyCode::Enter {
                    tab.pane = None;
                    self.spawn_pane(&id, idx);
                }
            }
        }
    }

    pub(super) fn on_sidebar_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('g') | KeyCode::Home => self.move_selection(-10_000),
            KeyCode::Char('G') | KeyCode::End => self.move_selection(10_000),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => self.open_issue(),
            KeyCode::Char('s') => {
                self.state.sort = self.state.sort.next();
                self.state.sort_reversed = false;
                self.state.save();
                self.flash(format!("Sort: {}", self.sort_label()));
            }
            KeyCode::Char('i') => self.flip_sort(),
            KeyCode::Char('0') => self.set_view(Stage::Todo),
            KeyCode::Char('1') => self.set_view(Stage::Active),
            KeyCode::Char('2') => self.set_view(Stage::Done),
            KeyCode::Char('3') => self.set_view(Stage::Archive),
            KeyCode::Char('v') | KeyCode::Char('h') | KeyCode::Left => {
                let next = match self.view {
                    Stage::Todo => Stage::Active,
                    Stage::Active => Stage::Done,
                    Stage::Done => Stage::Archive,
                    Stage::Archive => Stage::Todo,
                };
                self.set_view(next);
            }
            KeyCode::Tab | KeyCode::Esc => {
                if self.can_focus_terminal() {
                    self.focus = Focus::Terminal;
                }
            }
            _ => {
                // A typed character that isn't a sidebar key was likely meant for the tab.
                let typed = matches!(k.code, KeyCode::Char(_)) && k.modifiers.difference(KeyModifiers::SHIFT).is_empty();
                if !self.on_command(k) && typed && self.can_focus_terminal() {
                    self.flash(format!("Sidebar has focus — Tab or {} s to type in the terminal", self.prefix_label()));
                }
            }
        }
    }

    /// Commands shared by the sidebar (bare keys) and the prefix table.
    fn on_command(&mut self, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Char('c') => self.new_tab(TabKind::Claude),
            KeyCode::Char(c @ ('C' | 'I')) => match self.selected.clone() {
                None => self.flash("Select an issue first".into()),
                Some(id) if self.refuse_retired(&id) => {}
                Some(_) => {
                    let kind = if c == 'C' { PromptKind::ClaudeDir } else { PromptKind::ResumeId };
                    self.mode = Mode::Prompt(kind, String::new());
                }
            },
            KeyCode::Char('p') => self.mode = Mode::Prompt(PromptKind::DefaultDir, String::new()),
            KeyCode::Char('t') => self.new_tab(TabKind::Shell),
            KeyCode::Char('e') => self.new_tab(TabKind::Notes),
            KeyCode::Char('o') => self.open_obsidian(),
            KeyCode::Char('w') => {
                if let Some(u) = self.selected_note().and_then(|n| n.url.clone()) {
                    self.outbox.push(ServerMsg::Open(u));
                } else {
                    self.flash("This issue has no URL".into());
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.mode = Mode::Prompt(PromptKind::NewIssue, String::new())
            }
            KeyCode::Char('d') => self.move_stage(Stage::Done),
            KeyCode::Char('a') => self.move_stage(Stage::Archive),
            KeyCode::Char('u') => self.move_stage(Stage::Active),
            KeyCode::Char('T') => self.move_stage(Stage::Todo),
            KeyCode::Char('S') | KeyCode::Char('r') => {
                if providers::from_config(&self.cfg).is_empty() {
                    self.flash("No [[github]] or [[jira]] sources in config".into());
                } else {
                    self.start_sync();
                    self.flash("Syncing…".into());
                }
            }
            KeyCode::Char('x') => {
                if self.current_rt().is_some_and(|r| !r.tabs.is_empty()) {
                    self.mode = Mode::Confirm(ConfirmKind::CloseTab);
                }
            }
            KeyCode::Char('z') => self.request_suspend(),
            KeyCode::Char('R') => self.reopen_tab(),
            KeyCode::Char(',') => {
                if let Some(rt) = self.current_rt() {
                    if let Some(t) = rt.tabs.get(rt.active) {
                        self.mode = Mode::Prompt(PromptKind::RenameTab, t.st.name.clone());
                    }
                }
            }
            KeyCode::Char('`') => {
                self.select_id(SCRATCH.into());
                self.focus_tab_if_live();
            }
            KeyCode::Char('q') => self.mode = Mode::Confirm(ConfirmKind::Quit),
            KeyCode::Char('D') => self.detach = true,
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => return false,
        }
        true
    }

    fn on_prefix_command(&mut self, k: KeyEvent) {
        if keys::is_prefix(&k, &self.prefix) {
            if let Some(p) = self.active_pane_mut() {
                let b = keys::encode(&k, false);
                p.write(&b);
            }
            return;
        }
        match k.code {
            KeyCode::Char('s') | KeyCode::Tab => {
                let can_focus = self.can_focus_terminal();
                self.focus = match self.focus {
                    Focus::Sidebar if can_focus => Focus::Terminal,
                    _ => Focus::Sidebar,
                };
            }
            KeyCode::Char(c @ '1'..='9') => {
                self.switch_tab(c as usize - '1' as usize);
                self.view_tab();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.cycle_tab(1);
                self.view_tab();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.cycle_tab(-1);
                self.view_tab();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                self.after_issue_switch();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                self.after_issue_switch();
            }
            KeyCode::Char('[') | KeyCode::PageUp => {
                self.scroll_page(true);
            }
            KeyCode::Char(']') | KeyCode::PageDown => {
                self.scroll_page(false);
            }
            _ => {
                self.on_command(k);
            }
        }
    }

    /// Focus the active tab if it has a pane (running or exited) or a saved
    /// screen, else the sidebar. Never starts, restarts or resumes anything.
    pub(super) fn focus_tab_if_live(&mut self) {
        self.focus = if self.can_focus_terminal() { Focus::Terminal } else { Focus::Sidebar };
    }

    /// When moving between issues from the terminal (prefix `j`/`k`, the
    /// sidebar wheel), stay in the terminal only if the target's tab is live.
    pub(super) fn after_issue_switch(&mut self) {
        if self.focus == Focus::Terminal && !self.can_focus_terminal() {
            self.focus = Focus::Sidebar;
        }
    }

    pub(super) fn rename_tab(&mut self, name: String) {
        let Some(id) = self.selected.clone() else { return };
        let name = name.trim().to_string();
        let rt = self.rt_for(&id);
        if let Some(t) = rt.tabs.get_mut(rt.active) {
            t.st.custom_name = !name.is_empty();
            if !name.is_empty() {
                t.st.name = name;
            } else if t.st.kind != TabKind::Claude {
                // A Claude tab keeps its name until its title renames it; a
                // shell or note tab has no title, so it goes back to its default.
                t.st.name = super::spawn::default_name(t.st.kind).into();
                t.cmd.reset(Instant::now());
            }
        }
        self.persist(&id);
    }
}
