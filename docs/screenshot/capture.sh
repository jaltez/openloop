#!/usr/bin/env bash
# Regenerate the README TUI screenshots: fabricate a demo OPENLOOP_HOME,
# run the real TUI in a detached tmux pane, capture the pane with escapes,
# and convert to SVG. Requires: python3, tmux, a release build.
set -euo pipefail
cd "$(dirname "$0")"
ROOT=$(git rev-parse --show-toplevel 2>/dev/null || echo ../..)
BIN=${OPENLOOP_BIN:-$ROOT/target/release/openloop}
W=112 H=34

python3 make_demo_state.py

capture() { # capture <tab-key|-> <out>
    tmux kill-session -t oldemo 2>/dev/null || true
    tmux new-session -d -s oldemo -x "$W" -y "$H" \
        "env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor \
         OPENLOOP_HOME=/tmp/openloop-demo/home '$BIN'; sleep 90"
    sleep 11.5   # let the 1–10 s staggered refreshes populate every view
    [ "$1" != - ] && tmux send-keys -t oldemo "$1"
    [ "$1" != - ] && sleep 3.5
    tmux capture-pane -t oldemo -e -p > "$2"
    tmux send-keys -t oldemo q
    sleep 0.6
    tmux kill-session -t oldemo 2>/dev/null || true
}

RAW=/tmp/openloop-demo   # raw captures are intermediates, kept out of the tree

capture -  "$RAW/dashboard.txt"
python3 ansi2svg.py "$RAW/dashboard.txt" tui-dashboard.svg "openloop — dashboard"
capture 3 "$RAW/tasks.txt"
python3 ansi2svg.py "$RAW/tasks.txt" tui-tasks.svg "openloop — tasks (atlas-api)"
echo "done: tui-dashboard.svg tui-tasks.svg"
