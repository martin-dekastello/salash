//! Event entry point, mouse, paste and text selection.

use super::*;

impl App {
    // ---------- input ----------

    pub fn handle(&mut self, ev: Event) {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Event::Paste(s) => self.on_paste(s),
            Event::Mouse(m) if self.cfg.mouse => self.on_mouse(m),
            _ => {}
        }
    }

    pub(super) fn on_paste(&mut self, s: String) {
        if let Mode::Prompt(_, buf) = &mut self.mode {
            buf.push_str(s.lines().next().unwrap_or(""));
            self.completion = None;
            return;
        }
        if self.focus == Focus::Terminal {
            if let Some(t) = self.active_tab_mut().filter(|t| t.pane.is_some()) {
                t.seen_ts = now_ms();
            }
            if let Some(p) = self.active_pane_mut() {
                if p.bracketed_paste() {
                    let mut b = b"\x1b[200~".to_vec();
                    b.extend_from_slice(s.as_bytes());
                    b.extend_from_slice(b"\x1b[201~");
                    p.write(&b);
                } else {
                    p.write(s.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
                }
            }
        }
    }

    /// Offer a mouse event to the active pane, clamped to its area. False if
    /// the pane's app doesn't want it.
    fn forward_mouse(&mut self, m: MouseEvent) -> bool {
        let r = self.term_rect;
        if self.mode != Mode::Normal || r.is_empty() {
            return false;
        }
        let local = MouseEvent {
            column: m.column.clamp(r.left(), r.right() - 1) - r.left(),
            row: m.row.clamp(r.top(), r.bottom() - 1) - r.top(),
            ..m
        };
        self.active_pane_mut().is_some_and(|p| p.send_mouse(&local))
    }

    /// A press the pane's app didn't want starts a selection, in shell tabs only:
    /// Claude Code handles its own.
    fn start_select(&mut self, pos: Position) {
        let r = self.term_rect;
        self.pane_select = None;
        if self.pane_drag {
            return;
        }
        let Some(tab) = self.active_tab().filter(|t| t.st.kind == TabKind::Shell) else { return };
        self.pane_select = Some((tab.st.id.clone(), pos));
        if let Some(p) = self.active_pane_mut() {
            p.select_start(pos.y - r.top(), pos.x - r.left());
        }
    }

    /// The pane of the selection under way, unless its tab is no longer in view.
    fn select_pane(&mut self) -> Option<&mut Pane> {
        let id = self.pane_select.as_ref()?.0.clone();
        self.active_tab().filter(|t| t.st.id == id)?;
        self.active_pane_mut()
    }

    /// Stretch the selection to the mouse, clamped to the pane.
    fn extend_select(&mut self) {
        let r = self.term_rect;
        let Some((_, pos)) = self.pane_select.clone() else { return };
        if r.is_empty() {
            return;
        }
        let row = pos.y.clamp(r.top(), r.bottom() - 1) - r.top();
        let col = pos.x.clamp(r.left(), r.right() - 1) - r.left();
        if let Some(p) = self.select_pane() {
            p.select_to(row, col);
        }
    }

    /// Scroll while the mouse is held above or below the pane.
    pub(super) fn autoscroll_select(&mut self) {
        let r = self.term_rect;
        let Some((_, pos)) = self.pane_select.clone() else { return };
        let delta = if pos.y < r.top() {
            1
        } else if pos.y >= r.bottom() {
            -1
        } else {
            return;
        };
        if let Some(p) = self.select_pane().filter(|p| !p.alternate_screen()) {
            p.scroll_by(delta);
            self.extend_select();
        }
    }

    fn finish_select(&mut self) {
        let Some(p) = self.select_pane() else {
            self.pane_select = None;
            return;
        };
        let text = p.selected_text();
        if text.is_none() {
            p.selection = None;
        }
        self.pane_select = None;
        if let Some(text) = text {
            self.flash(format!("Copied {} characters", text.chars().count()));
            self.outbox.push(ServerMsg::Clipboard(text));
        }
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        // A click must not change which tab an open prompt applies to, but a
        // release still has to end any drag in progress.
        if self.mode != Mode::Normal && !matches!(m.kind, MouseEventKind::Up(_)) {
            return;
        }
        let pos = Position::new(m.column, m.row);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.quit_hit.contains(pos) {
                    self.mode = Mode::Confirm(ConfirmKind::Quit);
                } else if self.sort_hit.contains(pos) {
                    self.flip_sort();
                    self.focus = Focus::Sidebar;
                } else if let Some((_, i)) = self.close_hits.iter().find(|(r, _)| r.contains(pos)).cloned() {
                    self.switch_tab(i);
                    self.mode = Mode::Confirm(ConfirmKind::CloseTab);
                } else if let Some((_, i)) = self.tab_hits.iter().find(|(r, _)| r.contains(pos)).cloned() {
                    self.switch_tab(i);
                    self.view_tab();
                } else if let Some((_, s)) = self.stage_hits.iter().find(|(r, _)| r.contains(pos)).cloned() {
                    self.set_view(s);
                    self.focus = Focus::Sidebar;
                } else if let Some((_, id)) = self.item_hits.iter().find(|(r, _)| r.contains(pos)).cloned() {
                    // A click only selects: focus stays in (or returns to) the
                    // sidebar, so its keys act on the issue. The terminal still
                    // shows the issue's tab, dimmed. Unlike ⏎, a click never
                    // starts, restarts or resumes a tab.
                    self.select_id(id);
                    self.focus = Focus::Sidebar;
                } else if self.frame_rect.contains(pos) {
                    if self.can_focus_terminal() {
                        self.focus = Focus::Terminal;
                    }
                } else if self.term_rect.contains(pos) {
                    if self.can_focus_terminal() {
                        self.focus = Focus::Terminal;
                    }
                    self.pane_drag = self.forward_mouse(m);
                    self.start_select(pos);
                } else if self.sidebar_rect.contains(pos) {
                    self.focus = Focus::Sidebar;
                }
            }
            MouseEventKind::Down(_) if self.term_rect.contains(pos) => {
                self.pane_drag = self.forward_mouse(m);
            }
            MouseEventKind::Drag(_) if self.pane_drag => {
                self.forward_mouse(m);
            }
            MouseEventKind::Up(_) if self.pane_drag => {
                self.forward_mouse(m);
                self.pane_drag = false;
            }
            MouseEventKind::Drag(_) if self.pane_select.is_some() => {
                if let Some((_, last)) = &mut self.pane_select {
                    *last = pos;
                }
                self.extend_select();
            }
            MouseEventKind::Up(_) if self.pane_select.is_some() => self.finish_select(),
            MouseEventKind::Moved if self.term_rect.contains(pos) => {
                self.forward_mouse(m);
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = m.kind == MouseEventKind::ScrollUp;
                if self.term_rect.contains(pos) {
                    if !self.forward_mouse(m) {
                        self.scroll_active(if up { 3 } else { -3 });
                        self.extend_select();
                    }
                } else if self.sidebar_rect.contains(pos) {
                    self.move_selection(if up { -1 } else { 1 });
                    self.after_issue_switch();
                }
            }
            _ => {}
        }
    }
}
