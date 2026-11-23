#!/usr/bin/env bash
# Re-record the README demo into docs/demo.gif:  demo/record.sh
#
# demo/ holds everything the recording sees: the issues (vault/), the project they are
# about (tiles/), the config and the script of keystrokes (demo.tape). Each take copies it
# to /tmp/salash-demo, because salash moves notes and adds worktrees, so the
# committed files never change and every take starts from the same state.
#
# Needs vhs (brew install vhs) and claude, logged in. Claude is real (Haiku): its wording
# varies a little between takes, the keystrokes never do.
set -euo pipefail
cd "$(dirname "$0")/.."
DEMO=/tmp/salash-demo   # demo.tape uses this path too

stop_demo() {
  local lock=$DEMO/work/.salash/server.lock
  if [[ -f $lock ]]; then kill -TERM "$(cat "$lock")" 2>/dev/null || true; sleep 1; fi
}

cargo build --release

stop_demo
rm -rf "$DEMO"
mkdir -p "$DEMO/work" "$DEMO/worktrees"
cp -R demo/vault demo/tiles "$DEMO/"
sed "s#@DEMO@#$DEMO#g" demo/config.toml > "$DEMO/config.toml"
git -C "$DEMO/tiles" init -q -b main
git -C "$DEMO/tiles" add -A
git -C "$DEMO/tiles" -c user.name=demo -c user.email=demo@example.com commit -qm "Initial tiles"

# Run from inside salash or Claude Code, the recording would inherit that session's
# variables: SALASH_* would point at your real salash, CLAUDE* make Claude a child session.
unset $(compgen -e | grep -E '^(SALASH_|CLAUDE)') || true

trap stop_demo EXIT
vhs demo/demo.tape
ls -lh docs/demo.gif
