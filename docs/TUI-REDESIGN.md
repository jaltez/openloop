# TUI Redesign — Coverage Analysis and Proposal

Status: implemented. This document records the analysis that drove the
redesign and the design contract the views now follow.

## 1. CLI vs TUI coverage (before the redesign)

| Capability | CLI | TUI before | Assessment |
|---|---|---|---|
| Multi-project overview | `status` | read-only table | weak: no run/budget/blocked signal |
| Project add / list | `project add/list` | add + list | ok |
| Project activate / remove / init | `project activate/remove/init` | add + list only | gap — plus no git visibility |
| Task add / list / filter / delete | `task …` | yes | ok |
| **Task approve** | `task approve` | — | **critical gap** — the core human-in-the-loop action was CLI-only |
| Task update (status/risk) | `task update` | — | gap — retrying a blocked task meant leaving the TUI |
| Run one iteration | `service run` | — | gap |
| Service start / stop / restart | `service …` | pause/resume only | **gap** — a stopped daemon could not be started from the TUI |
| Pause / resume | `service pause/resume` | yes | ok |
| Promotions list / detail / apply / reject | `promotion …` | yes | ok, but apply/reject fired on a single key with no confirmation |
| Approval packets | `approval list/show` | — | accepted gap: packets are rare, `tasks` awaiting-approval covers the loop |
| Config view / edit | `config show/set` | raw key/value edit | ok for the settable subset |
| Events (since/project filters) | `events` | yes | ok |
| Logs / run summaries | `logs` | yes | ok |
| Ad-hoc agent run | `run` | Ctrl+R prompt + model | ok |
| Doctor / report / digest / setup / issue / dashboard / mcp | yes | — | accepted gaps: one-shot or server surfaces, CLI is the right home |
| Raw git mutations (stage/commit/branch ops) | internal (agent flow) | — | accepted gap: the daemon owns the git lifecycle; the TUI surfaces state, humans decide |

## 2. UX findings (why it felt compact and unfriendly)

1. **No viewport scrolling.** Lists rendered every row into a `Paragraph`;
   once a list outgrew the screen the selection scrolled out of view.
2. **Hand-rolled fixed-width columns** (`pad()` + hardcoded widths) truncated
   titles at 30 chars on wide terminals and misaligned on narrow ones.
3. **No structural components.** Only two views used borders; the rest were
   1-line headers and text dumps with zero breathing room.
4. **Dashboard wasted the prime area**: no current-run card, no budget gauge,
   no blocked-task signal, no recent activity.
5. **Duplicated chrome**: daemon status + budget appeared in both the
   dashboard header and the status bar.
6. **Ad-hoc 3-row input boxes** drawn at hardcoded coordinates, one variant
   per view, no shared modal.
7. **No confirmation for destructive or git-mutating actions**: task delete,
   promotion apply (fast-forward merge into the base branch!) fired
   instantly on a single key.
8. **No mouse support.**

## 3. Design contract (implemented)

### Global chrome
- Tab bar with icons and grouped sections, brand block on the left.
- Status bar: daemon/project/budget on the left, version + help on the right.
- Per-view key hints rendered as `[key] label` chips at the bottom of each
  view (single authoritative place per view, also mirrored in `?` help).

### Input ergonomics
- **Directory browser** replaces free-text path entry for `project add`:
  starts at `$HOME`, filter-first (every typed character narrows the
  listing; navigation is arrow-only so typing never jumps), `→` descends,
  `←`/Backspace goes up (Backspace trims the filter first), `Ctrl+H`
  toggles hidden files, `Enter` accepts the listed directory. The accepted
  path is resolved and normalized exactly like the CLI.
- **Input caret**: every text input renders a `▏` caret at the edit point.
- **Generic picker modal**: reused for task status, events project filter,
  and any future option list.
- **Service `[b]`** sets the daily budget in place (validated `f64 >= 0`)
  instead of navigating through the Config tab.
- **Responsive tab bar**: falls back to icon+number chips when labels
  would overflow narrow terminals.

### Components (`src/tui/ui.rs`)
- `panel()` — rounded, padded, titled block used by every view.
- `stat_card()` — label + big value + optional footnote, used for the
  dashboard Daemon/Budget/Run cards.
- `bar()` — unicode progress gauge for budget usage.
- `input_modal()` / `confirm_modal()` / `picker_modal()` — shared centered
  modals with `Clear` underlay; every input and every destructive action
  goes through them.
- `key_hint_line()` — the chip row.

### Tables
Every list is a ratatui `Table` with percentage constraints (responsive)
rendered through a persistent `TableState`, so selection implies scrolling.
Selection style: full-row background + `▌` gutter marker.

### View layouts
- **Dashboard**: stat cards row (daemon, budget gauge, current run) →
  projects table (`PROJECT QUEUE BLOCKED LAST RESULT AGE`) → recent
  activity strip (last events, colored).
- **Projects**: table with live git columns (current branch, working-tree
  state: clean / dirty (n) / not a repo / missing) + detail panel (default
  vs current branch, changed-file count, agent + model from project config,
  queue/blocked/last result). `Enter` activates, `a` adds (path resolved and
  validated like the CLI), `i` materializes templates, `I` force re-init
  with backup (confirm), `x` removes (confirm).
- **Tasks**: table + rich detail (kind/risk/branch/attempts/dependsOn/
  promotion/cost/notes/last run). `y` approves awaiting-approval tasks,
  `u` opens a status picker, `d` deletes (confirm).
- **Service**: state panel with budget gauge + full action list
  (Start/Stop/Restart/Pause/Resume/Run once/View logs) with confirmation on
  Stop/Restart; `Run once` executes a worker tick in the background and
  reports the outcome.
- **Config / Events / Logs / Run / Promotions**: same panel + table
  treatment; promotions confirm before apply/reject.

### Mouse
Wheel scrolls the focused list; clicking the tab bar switches views.

## 4. Sharing with the CLI

Daemon lifecycle inspection/start/stop/restart lives in
`src/core/service_control.rs` and is used by both the CLI `service`
subcommands and the TUI service view — one implementation, two surfaces.

## 5. Accepted gaps

Approval packets, doctor, report, digest, setup, issue sources, dashboard
server, and MCP remain CLI-only by design (one-shot or server surfaces
where a terminal UI adds nothing).
