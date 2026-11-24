# salash

A terminal app: issues on the left, Claude Code tabs on the right, one Obsidian note per issue.
See README.md for features and config.

## Architecture reference
tmux is a close cousin of this project: a multiplexer that owns PTYs, runs its own terminal
emulation, keeps sessions alive and lets you pick them up again, and shows status per window.
When a question is about that kind of architecture (PTY handling, terminal emulation,
scrollback, resizing, detach/resume, status lines, key handling), look at how tmux solves it and
cite what you found. Use it as a reference, not as something to copy.

## Prior art
If a problem is already solved on GitHub (a crate, library, algorithm or known pattern), give the
references with links and suggest it as a solution before building our own.

## Design decisions
When work on an issue settles something a reader couldn't guess from the code, write it down in
`docs/` and link the GitHub issue that caused it
(`[#28](https://github.com/martin-dekastello/salash/issues/28)`). Two kinds count:
- **Big design decisions** that shape how the app works, e.g. one server per folder with
  clients attaching over a socket (`docs/architecture.md`).
- **Small but non-obvious rules** some behaviour depends on, e.g. why a check exists or an
  order matters, where the "obvious" simplification would break it.

Put it in the doc that covers that area (add a new `docs/<topic>.md` if none fits): say what was
decided, why, and what it rules out, with the issue link. Do this in the same branch/PR as the
code change. Routine fixes and anything the code already makes clear don't need an entry.
