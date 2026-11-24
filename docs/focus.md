# Sidebar and terminal focus

Keys go to one of two places: the **sidebar** (salash's own keys: `d`, `a`, `x`, `c`, `j`/`k`…)
or the **terminal** (the active tab's shell, Claude or editor). `App::focus` holds which one,
per client (see [multi-client.md](multi-client.md)).

## Focus is shown, not guessed

Since [#26](https://github.com/martin-dekastello/salash/issues/26) the terminal frame is heavy
when the terminal has focus, and the side without focus is dimmed (`src/ui.rs`, like tmux's
`window-style`). The sidebar's header and key footer stay lit, so the way back is always
readable.

## A click on an issue selects it and keeps focus in the sidebar

Decided in [#44](https://github.com/martin-dekastello/salash/issues/44), which reverted
[#20](https://github.com/martin-dekastello/salash/issues/20).

#20 made a click on an issue row also jump into its last-used tab. That was a workaround for
not seeing where focus was. It made the sidebar hard to use with the mouse: after clicking an
issue to act on it, `d` or `x` went to the shell or Claude instead. Once #26 made focus
visible, the jump had no reason left.

The rule now (`App::on_mouse`, the `item_hits` branch):

- Clicking an issue row selects it and sets focus to the sidebar, also when the terminal had
  focus and the issue was already selected.
- The terminal area still shows the issue's last-used tab, dimmed.
- A click never starts, restarts or resumes a tab; `⏎` does.
- Moving into the tab is explicit: `⏎`, `Tab` / `prefix s`, or a click on the terminal, a tab
  header or the frame line.

What this rules out: don't put `focus_tab_if_live()` back on the issue click "for convenience".
The sidebar's single-letter keys depend on the click leaving focus where it is.
`focus_tab_if_live()` is still right for the `` ` `` scratch key, which is an explicit "go to
scratch" command.

For comparison, tmux's status line holds no keys of its own: clicking a window name there
switches to that window, and keys go to its pane. salash's sidebar is a place with its own keys,
so a click on it keeps them there.
