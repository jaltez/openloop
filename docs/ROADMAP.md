# OpenLoop roadmap

Status as of 2026-11-19 (`v0.8.0`). This is the durable distillation of the
product direction discussed across sessions — features shipped, debt we
deliberately carried, and the open questions that need a decision before work
starts. Keep it short; it is a decision record, not marketing.

## Where the product stands

OpenLoop is a global CLI + single-threaded daemon for unattended AI-driven
development across linked repositories. It owns the control plane
(project registry, per-project templates, ledger, events, budgets, schedules,
promotion pipeline) while pluggable providers (Pi by default; Claude Code,
Aider, Codex, OpenCode, `ka`, oh-my-pi, custom commands) do the actual work.

Surfaces: CLI, TUI, MCP stdio server, localhost HTTP dashboard, digest/report.

## Recently delivered

- **2026-11-17 — implicit state machine hardening** (`c4ba649`): notification
  queue identity per (channel, payload); cron catch-up keyed by matched minute
  with id-exact dedupe; stale `budgetBlocked` clears on budget raise; shutdown
  escalation SIGTERM → wait → SIGKILL; file-lock stale-break backoff; single
  shutdown-sentinel classifier; stale reviewer output removal; explicit
  `TaskStatus` daemon-edge table with `task_status_overridden` audit at the
  ledger choke point; TUI help/modal routing, off-tab run drain, cache and
  cursor correctness on project switch, picker titles.
- **2026-11-19 — agent-native write surface + observability**: MCP write tools
  (`openloop_task_update`, `openloop_task_approve`, `openloop_promotion_apply`,
  `openloop_promotion_reject`) routed through the same ledger choke point, so
  human/agent status overrides stay audited; `/metrics` Prometheus endpoint on
  the dashboard; `run_report` carries the `task_status_overridden` trail
  (`statusOverrides` in JSON, a "Status overrides" section in the table).

## Deliberately carried debt

Recorded by the 2026-11-17 audit; each is safe today but should be resolved
before it grows.

| Item | Why it is deferred |
|---|---|
| Run-mode strings (`"plan"/"implement"/"idle"`) across ~6 files | Parity-test-pinned vocabulary; typing it is churn with wire risk and zero behavioral gain |
| `RunStoppedBy::Pause`/`Budget` variants | Wire compatibility; nothing constructs them |
| `PromotionResultArtifact.result` string | Write-only display data (single writer in `pr_refresh.rs`); either surface it or delete it |
| No-progress counters in note strings | Persisted, human-displayed, parity-tested; a typed field would dual-write |
| `IdleStatus.kind` taxonomy string | Derived display value, low value to type |
| Issue-sync seen-set / `SyncedIssue.state` staleness | Bounded by issues ever seen, cosmetic |

## Open risks (found during hardening, not fixed)

- **Cron catch-up flood**: `SCHEDULE_MAX_CATCHUP_MINUTES = 7 days`. An
  every-minute schedule plus one day of daemon downtime now correctly fires
  one task per missed minute (~1,440 tasks; up to ~10k at the cap) across
  successive ticks. Needs a coalescing policy ("missed N minutes → one
  catch-up task") or a per-schedule catch-up cap.
- **Dead modal titles**: `picker_modal` was fixed to render its block, but
  `confirm_modal` and `input_modal` (`src/tui/ui.rs`) still build a block only
  for its `.inner()` — their borders/title never render. Two-line fix each.
- **Reviewer output freshness**: each review run deletes the previous
  `.openloop/reviews/{id}.json`, but unlike verifier verdicts there is no
  attempt marker, so a file written by a previous attempt in an exotic
  interleaving could still be re-imported.

## Next-tier candidates

1. **Bound the catch-up flood** (see risks) — smallest change with the largest
   reliability payoff for unattended fleets.
2. **Metrics depth**: `/metrics` currently exposes daemon/budget/task-status
   counters derived from the dashboard snapshot. Next: event-rate counters
   (schedule fires, overrides, promotion outcomes), active-run gauges, and
   per-project spend (already computed by `digest`, so reuse it rather than
   duplicating).
3. **MCP surface policy**: write tools are limited to task lifecycle and
   promotion resolution. Decide whether project registration / config edits
   stay human-only (current stance: yes — they change the control plane
   itself, and MCP clients are agents).
4. **Fleet / multi-host**: the daemon is single-machine and single-threaded by
   design (see `AGENTS.md`). Cross-machine coordination (shared registry,
   distributed locks on top of the existing file locks, event replication) is
   the one genuinely open strategic question. The August research sessions on
   this direction were never distilled into an artifact — this section is the
   placeholder until a decision is made; do not re-research before writing it
   down here.

## How to update this file

- Move shipped items from "next-tier" to "Recently delivered" with a commit
  hash and a one-line summary.
- Add debt rows only when the item is understood and intentionally deferred;
  link the file/symbol that carries it.
- Keep sections in this order; delete stale entries instead of archiving
  them.
