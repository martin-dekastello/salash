//! Starting panes: Claude's argv, where a tab starts, worktrees, new tabs.

use super::types::WorktreeJob;
use super::*;

/// Where a tab that needs the issue's checkout starts.
pub(super) enum Workdir {
    Ready(PathBuf),
    /// The worktree is being created; the tab starts when it's done.
    Pending,
    /// Worktrees are off: the repo itself.
    Repo(PathBuf),
    NoRepo,
}

/// Claude's argv: the configured command, salash's hooks settings, plugin and
/// MCP config when given, then `--resume <session>` or, on a first launch, `first_args`.
pub(super) fn claude_argv(
    cmd: &[String],
    settings: Option<&Path>,
    plugin: Option<&Path>,
    mcp: Option<&Path>,
    session: Option<&str>,
    first_args: Vec<String>,
) -> Vec<String> {
    let mut v = cmd.to_vec();
    if let Some(s) = settings {
        v.push("--settings".into());
        v.push(s.to_string_lossy().into_owned());
    }
    if let Some(p) = plugin {
        v.push("--plugin-dir".into());
        v.push(p.to_string_lossy().into_owned());
    }
    // One argument: `--mcp-config` takes several values, so a separate path
    // would take a first prompt after it as another.
    if let Some(m) = mcp {
        v.push(format!("--mcp-config={}", m.display()));
    }
    match session {
        Some(sid) => {
            v.push("--resume".into());
            v.push(sid.into());
        }
        None => v.extend(first_args),
    }
    v
}

impl App {
    // ---------- tabs ----------

    pub(super) fn rt_for(&mut self, id: &str) -> &mut IssueRt {
        if !self.rt.contains_key(id) {
            let st = self.state.issues.get(id).cloned().unwrap_or_default();
            let rt = IssueRt {
                active: st.active_tab.min(st.tabs.len().saturating_sub(1)),
                worktree: st.worktree.clone(),
                tabs: st.tabs.into_iter().map(Tab::new).collect(),
                closed: st.closed,
                ..Default::default()
            };
            self.rt.insert(id.to_string(), rt);
        }
        self.rt.get_mut(id).unwrap()
    }

    pub fn current_rt(&self) -> Option<&IssueRt> {
        self.rt.get(self.selected.as_ref()?)
    }

    pub fn current_tabs(&self) -> Option<(Vec<&Tab>, usize)> {
        let id = self.selected.as_ref()?;
        let rt = self.rt.get(id)?;
        Some((rt.tabs.iter().collect(), rt.active))
    }

    pub fn saved_tabs(&self, id: &str) -> usize {
        self.rt
            .get(id)
            .map(|r| r.tabs.len())
            .unwrap_or_else(|| self.state.issues.get(id).map(|s| s.tabs.len()).unwrap_or(0))
    }

    pub(super) fn persist(&mut self, id: &str) {
        if let Some(rt) = self.rt.get(id) {
            self.state.issues.insert(
                id.to_string(),
                IssueState {
                    worktree: rt.worktree.clone(),
                    tabs: rt.tabs.iter().map(|t| t.st.clone()).collect(),
                    active_tab: rt.active,
                    closed: rt.closed.clone(),
                },
            );
            self.state.save();
        }
    }

    pub fn default_dir(&self) -> PathBuf {
        usable_default(self.state.default_dir.as_deref(), &self.folder)
    }

    /// Working directory for an issue's tab. The worktree is created the first
    /// time a tab needs it, in the background, and the tab starts once it exists.
    pub(super) fn workdir(&mut self, note: &Note, tab: &str) -> Workdir {
        let rt = self.rt_for(&note.id);
        if let Some(wt) = rt.worktree.clone().filter(|p| p.exists()) {
            return Workdir::Ready(wt);
        }
        if let Some(job) = &mut rt.creating {
            if !job.waiting.iter().any(|t| t == tab) {
                job.waiting.push(tab.to_string());
            }
            return Workdir::Pending;
        }
        let repo = note
            .repo_path
            .clone()
            .or_else(|| self.cfg.repo_path_for(&note.source, note.repo.as_deref()));
        let Some(repo) = repo else { return Workdir::NoRepo };
        if !self.cfg.worktrees_for(&note.source, note.repo.as_deref()) {
            return Workdir::Repo(repo);
        }
        let command = self.cfg.worktree_command.clone();
        let dir = self.cfg.worktrees_dir.clone();
        let (stem, issue) = (note.stem(), note.id.clone());
        // Plain git falls back to the repo as it always has; a script the user
        // wrote reports its failure instead, as the repo is unlikely what they want.
        let fallback = command.is_empty().then(|| repo.clone());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = if command.is_empty() {
                worktree::ensure(&repo, &dir, &stem)
            } else {
                worktree::run_script(&command, &repo, &dir, &stem, &issue)
            };
            let _ = tx.send(r.map_err(|e| format!("{e:#}")));
        });
        let rt = self.rt_for(&note.id);
        rt.worktree_error = None;
        rt.creating = Some(WorktreeJob { rx, waiting: vec![tab.to_string()], fallback });
        Workdir::Pending
    }

    /// Start the tabs whose worktree has been created, or report why it wasn't.
    pub(super) fn poll_worktrees(&mut self) {
        let ids: Vec<String> = self.rt.iter().filter(|(_, r)| r.creating.is_some()).map(|(id, _)| id.clone()).collect();
        for id in ids {
            let Some(rt) = self.rt.get_mut(&id) else { continue };
            let Some(job) = &rt.creating else { continue };
            let result = match job.rx.try_recv() {
                Ok(r) => r,
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => Err("worktree creation stopped unexpectedly".into()),
            };
            let Some(job) = rt.creating.take() else { continue };
            let cwd = match (result, job.fallback) {
                (Ok(p), _) => {
                    rt.worktree = Some(p.clone());
                    self.persist(&id);
                    p
                }
                (Err(e), Some(repo)) => {
                    self.flash(format!("Worktree failed, using repo: {e}"));
                    repo
                }
                (Err(e), None) => {
                    // The script's own last words usually say what went wrong.
                    let last = e.lines().rfind(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_string();
                    rt.worktree_error = Some(e);
                    self.flash_error(format!("{id}: worktree script failed: {last}"));
                    continue;
                }
            };
            let Some(note) = self.notes.iter().find(|n| n.id == id).cloned() else { continue };
            for tab in job.waiting {
                let rt = self.rt_for(&id);
                // It may have been closed, or started elsewhere, while it waited.
                if let Some(idx) = rt.tabs.iter().position(|t| t.st.id == tab && t.pane.is_none()) {
                    self.start_pane(&id, Some(&note), idx, cwd.clone());
                }
            }
        }
    }

    pub(super) fn spawn_pane(&mut self, id: &str, idx: usize) {
        // Only Scratch has no note.
        let note = self.notes.iter().find(|n| n.id == id).cloned();
        if (note.is_none() && id != SCRATCH) || self.refuse_retired(id) {
            return;
        }
        let Some(tab) = self.rt_for(id).tabs.get(idx) else { return };
        let (kind, tab_id) = (tab.st.kind, tab.st.id.clone());
        // A session can only be resumed from the directory it started in.
        let chosen = tab.st.cwd.clone();
        let cwd = match chosen.filter(|p| p.is_dir()) {
            Some(p) => p,
            // Editing the note needs no checkout, so it never creates one.
            None if kind == TabKind::Notes => match self.rt_for(id).worktree.clone().filter(|p| p.is_dir()) {
                Some(p) => p,
                None => self.default_dir(),
            },
            // Scratch has no repo, so it starts where an issue with no repo would.
            None => match note.as_ref().map_or(Workdir::NoRepo, |n| self.workdir(n, &tab_id)) {
                Workdir::Ready(p) => p,
                Workdir::Pending => return,
                // Kept in the tab, so it resumes here even once worktrees are back on.
                // No home fallback as in settle_dir: a session on an issue with a repo
                // started in the repo or in its worktree, and that is returned first.
                Workdir::Repo(repo) => {
                    let Some(tab) = self.rt_for(id).tabs.get_mut(idx) else { return };
                    let dir = tab.st.cwd.get_or_insert(repo).clone();
                    self.persist(id);
                    dir
                }
                Workdir::NoRepo => {
                    let default = self.default_dir();
                    let Some(tab) = self.rt_for(id).tabs.get_mut(idx) else { return };
                    let dir = tab.st.settle_dir(&default, &home());
                    // Saved before Claude starts, so its session is never without one.
                    self.persist(id);
                    dir
                }
            },
        };
        self.start_pane(id, note.as_ref(), idx, cwd);
    }

    /// Start a tab's process. `note` is None only for Scratch.
    fn start_pane(&mut self, id: &str, note: Option<&Note>, idx: usize, cwd: PathBuf) {
        let (rows, cols) = (self.term_rect.height.max(5), self.term_rect.width.max(20));
        let rt = self.rt_for(id);
        let Some(tab) = rt.tabs.get_mut(idx) else { return };
        let st = tab.st.clone();
        let first_args = std::mem::take(&mut tab.first_args);
        let argv: Vec<String> = match st.kind {
            TabKind::Claude => claude_argv(
                &self.cfg.claude_command,
                self.hooks_settings.as_deref(),
                self.plugin_dir.as_deref(),
                self.mcp_config.as_deref(),
                st.claude_session.as_deref(),
                first_args,
            ),
            TabKind::Shell => self.cfg.shell(),
            TabKind::Notes => {
                let Some(note) = note else { return };
                let mut v = self.cfg.editor();
                v.push(note.path.to_string_lossy().into_owned());
                v
            }
        };
        let mut env = vec![
            ("SALASH_TAB".to_string(), st.id.clone()),
            ("SALASH_ISSUE".to_string(), id.to_string()),
            // Hooks and `salash tab new` in the tab reach this salash, not another folder's.
            ("SALASH_DATA".to_string(), crate::config::data_dir().to_string_lossy().into_owned()),
        ];
        if let Some(note) = note {
            env.push(("SALASH_NOTE".to_string(), note.path.to_string_lossy().into_owned()));
        }
        if let Ok(exe) = std::env::current_exe() {
            env.push(("SALASH_BIN".to_string(), exe.to_string_lossy().into_owned()));
        }
        status::clear_status(&st.id);
        let result = Pane::spawn(&argv, &cwd, &env, rows, cols);
        let rt = self.rt_for(id);
        let tab = &mut rt.tabs[idx];
        match result {
            Ok(p) => {
                tab.pane = Some(p);
                tab.status = if st.kind == TabKind::Claude { ClaudeStatus::Starting } else { ClaudeStatus::Idle };
                tab.status_ts = now_ms();
                state::remove_snapshot(&st.id);
            }
            Err(e) => self.flash(format!("Spawn failed: {e:#}")),
        }
    }

    pub(super) fn new_tab(&mut self, kind: TabKind) {
        self.new_tab_in(kind, None);
    }

    pub(super) fn new_tab_in(&mut self, kind: TabKind, cwd: Option<PathBuf>) {
        let Some(id) = self.selected.clone() else {
            self.flash("Select an issue first".into());
            return;
        };
        if kind == TabKind::Notes && id == SCRATCH {
            self.flash("Scratch has no note".into());
            return;
        }
        if self.refuse_retired(&id) {
            return;
        }
        if kind == TabKind::Claude {
            self.start_work(&id);
        }
        self.add_tab(&id, kind, None, cwd, vec![], true);
        self.focus = Focus::Terminal;
    }

    /// Add a tab to an issue and start it. Returns the new tab's id. Unless
    /// `activate`, the issue's active tab stays as it was.
    pub(super) fn add_tab(
        &mut self,
        issue: &str,
        kind: TabKind,
        name: Option<String>,
        cwd: Option<PathBuf>,
        first_args: Vec<String>,
        activate: bool,
    ) -> String {
        let default = default_name(kind);
        let tab_id = uuid::Uuid::new_v4().to_string();
        let mut tab = Tab::new(TabState {
            id: tab_id.clone(),
            custom_name: name.is_some(),
            name: name.unwrap_or_else(|| default.into()),
            kind,
            claude_session: None,
            cwd,
        });
        tab.first_args = first_args;
        let rt = self.rt_for(issue);
        rt.tabs.push(tab);
        let idx = rt.tabs.len() - 1;
        if activate {
            rt.active = idx;
        }
        self.persist(issue);
        self.spawn_pane(issue, idx);
        tab_id
    }
}

/// A new tab's name, and what clearing a shell or note tab's rename goes back to.
pub(super) fn default_name(kind: TabKind) -> &'static str {
    match kind {
        TabKind::Claude => "claude",
        TabKind::Shell => "shell",
        TabKind::Notes => "notes",
    }
}
