# Tab names

A tab's stored name (`TabState::name`, saved in `state.json`) is `claude`, `shell` or `notes`
until something renames it. Claude tabs take Claude's OSC title, unless the tab has a custom
name from `,`, `salash tab new --name` or MCP `open_tab`. This page covers what a shell tab
shows in place of that name.

## Shell tabs show the running command

Decided in [#47](https://github.com/martin-dekastello/salash/issues/47). While a command runs
in front of a shell's prompt, its tab shows `$ 1:vim` in place of `$ 1:shell`. This is iTerm2's
"Job" and tmux's `#{pane_current_command}`. The name is read the way tmux reads it
(`osdep_get_name` in osdep-darwin.c / osdep-linux.c):
- `tcgetpgrp` on the PTY master gives the foreground process group (`Pane::foreground_pid`);
- the short name (comm) of that group's leader is shown (`term::process_name`).

So `sudo vim` shows `sudo`, `npm run dev` shows `node`, and Linux cuts names off at 15
characters. Stopped and background jobs don't count, because the shell has the terminal back.
OSC titles from programs in a shell tab are ignored. Only the process is used, so a prompt
that sets the title doesn't overwrite it.

Rules a reader couldn't guess from the code, all in `CmdLabel` (`src/app/types.rs`):

- **Display only.** The command is never stored as the tab's name. It isn't written to
  `state.json`, MCP `list_tabs`/`get_tab` report the stored name, and so do the rename prompt,
  the close confirm and the not-running placeholder. Only the tab bar uses `Tab::shown_name`.
  Storing it would have saved a stale `vim` across restarts. It would also have sent MCP
  clients a tab-change notification for every command. After a restart, a shell tab starts
  idle.
- **A 1-second settle.** The label is worked out as a state: idle, or (process group, name).
  It changes only once a new state has held for 1s (`CMD_SETTLE`). This applies to a command
  starting, ending, being replaced, or its leader getting a new name. So `ls` followed by
  `vim` never flashes `ls`, and a script of short commands keeps the last label that lasted.
  tmux's `automatic-rename` re-reads the name on a 500ms timer and doesn't settle. We do,
  because the tab bar's width moves with the label.
- **`exec` detection by a list of shells.** `foreground_pid` treats the tab's own process group
  as idle, so `exec vim` would look like a shell at its prompt. To catch that, the own process's
  name is checked against a list of shells (`SHELLS`: sh, bash, zsh, fish, …, plus the basename
  of `shell_command[0]`; a login shell's leading `-` is dropped). Any other name is its own
  state, (own pgid, name), and goes through the same 1s settle, so `exec vim` shows `vim`. This
  works like iTerm2's "Jobs to Ignore". The first version used the first name it read as the
  baseline instead, and the PR #49 review found that a wrapper (`sh -c 'cd x && exec zsh'`) or a
  `.bashrc` ending in `exec fish` then showed `zsh`/`fish` for the whole session
  ([#47](https://github.com/martin-dekastello/salash/issues/47)). The cost of the list is that an
  `exec bash` typed in zsh stays `shell`, and `shell_command = ["vim"]` shows `shell` while vim
  runs, because vim is the tab's shell.
- **An unreadable name falls back.** If the foreground job's name can't be read, the own process
  is checked as above, so an exec'd `vim` still shows. If that can't be read either, as on an OS
  that isn't supported, the tab is idle. Without this the tab would show an empty label. On
  macOS the name is read with `PROC_PIDT_SHORTBSDINFO`, as tmux does. `proc_name` uses
  `PROC_PIDTBSDINFO`, which refuses other users' processes, so with it `sudo vim` would show
  `shell`.
- **The Done confirm agrees with the label.** Moving an issue to Done stops its idle tabs without
  asking, and lists the busy ones first. A shell tab counts as busy whenever its label would show
  a command, including an exec'd program. The check uses the same reading without the settle,
  and also counts a job whose name can't be read.
- **Custom names win.** A shell tab with a custom name never shows commands, and the label is
  held reset (nothing tracked) while the name is set.
- **Clearing a rename** (an empty `,`, or `,` confirmed unchanged on the default `shell` or
  `notes`) sets a shell tab's name back to `shell` and a note tab's to `notes`, since neither
  has a title to take over. It also restarts the 1s tracking from idle: with `vim` running, the
  tab shows `shell` for 1s, then `vim`. A Claude tab keeps its name, and its next OSC title
  renames it, as before.
- **Exited shells** show their stored name with `✕`, never the last command.
