# TUI Design Contract

Status: implemented. This is the current contract for the ratatui TUI —
what every view must look like and which interaction rules hold. The
pre-redesign analysis it replaced lives in git history (see the TUI
redesign commit).

## Why it was redesigned

The first port replicated the TypeScript TUI 1:1 and inherited its
weaknesses: no list scrolling, fixed-width columns that truncated on
real terminals, text-dump views without structure, per-view ad-hoc input
boxes, destructive actions (including promotion apply — a fast-forward
merge into the base branch) bound to bare keystrokes, and no mouse.
The redesign keeps the feature set but replaces the interaction model.

## Global chrome

- Tab bar with icons and grouped sections, brand block on the left;
  falls back to icon+number chips on narrow terminals.
- Status bar: daemon/project/budget on the left, version + help on the
  right.
- Per-view key hints render as `[key] label` chips at the bottom of each
  view (single authoritative place per view, mirrored in the `?` help).

## Interaction rules

1. A confirm/picker modal owns ALL routing while armed — global keys
   (digits, Tab, `?`, `q`) must not hide it or act underneath it; only
   Ctrl+C quits.
2. Every destructive or git-mutating action (task delete, project remove,
   promotion apply/reject, daemon stop/restart, force re-init) goes
   through `confirm_modal()`; nothing fires on a bare keystroke.
3. Every list is a ratatui `Table` through a persistent `TableState`:
   selection implies scrolling, and selections clamp when lists shrink.
4. Every text input renders a `▏` caret; Esc always cancels.
5. Mouse: wheel scrolls the focused list (and the Service action list);
   clicking the tab bar switches views; clicks are ignored while a modal
   is armed.

## Components (`src/tui/ui.rs`)

- `panel()` / `panel_untitled()` — rounded, padded, titled blocks; every
  view composes these instead of hand-rolling boxes.
- `stat_card()` — label + styled lines (dashboard Daemon/Budget/Run
  cards); `bar()` — unicode progress gauge.
- `input_modal()` / `confirm_modal()` / `picker_modal()` — shared centered
  modals with `Clear` underlay. The picker is generic (task status, events
  project filter, future option lists).
- `key_hint_line()` — the chip row.
- `browser_modal()` — filter-first directory browser for `project add`:
  starts at `$HOME`, every typed character narrows the listing,
  navigation is arrow-only (typing never jumps), `→` descends (symlinks
  included), `←`/Backspace goes up, `Ctrl+H` toggles hidden files,
  `Enter` accepts the listed directory, which is resolved/normalized
  exactly like the CLI.

## View responsibilities

- **Dashboard**: Daemon/Budget/Active-run stat cards (budget gauge with
  green/yellow/red thresholds) → projects table (`PROJECT QUEUE BLOCKED
  LAST RESULT AGE`) → recent-activity strip (newest events, colored).
- **Projects**: live git columns (branch, working-tree state: clean /
  dirty (n) / not a repo / missing) + repo detail card (branch drift,
  changed-file count, agent + model). `Enter` activates, `a` adds,
  `i` init templates, `I` force re-init (confirm), `x` remove (confirm).
- **Tasks**: table + rich detail (kind/risk/branch/attempts/dependsOn/
  promotion/cost/notes/last run). `y` approves awaiting-approval tasks,
  `u` opens the status picker, `f` filters, `d` deletes (confirm),
  `r` recovers stuck tasks.
- **Service**: daemon state panel (with action feedback) + full action
  list (Start/Stop/Restart/Pause/Resume/Run once/View logs) with
  confirmation on Stop/Restart; `[b]` sets the daily budget in place.
  "Run once" is refused while the daemon is running — only one process
  may execute ticks.
- **Config / Events / Logs / Run / Promotions**: same panel + table
  treatment; promotions confirm before apply/reject.

## Shared with the CLI

Daemon lifecycle (pid inspection/start/stop/restart) lives in
`src/core/service_control.rs` — one implementation, two surfaces.

## Accepted gaps (CLI-only by design)

Approval packets, doctor, report, digest, setup, issue sources, the
dashboard server, and MCP: one-shot or server surfaces where a terminal
UI adds nothing. Raw git mutations (stage/commit/branch ops) stay
internal to the agent flow — the TUI surfaces state, humans decide.
