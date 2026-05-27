# OpenLoop Backlog

> Active post-V1 backlog translated from external research across Aider, Plandex, OpenHands, and Claude Code. The resolved March 2026 audit backlog is preserved in the archive section at the bottom of this file.

## Current Status

- Audit date: 2026-03-16
- Audit completion: 32 of 32 audit items resolved
- Active backlog: this file
- README role: concise product roadmap in [README.md](README.md)
- Canonical repo docs: [README.md](README.md) and [AGENTS.md](AGENTS.md)

## Prioritization Guardrails

- Keep OpenLoop focused on local, unattended, multi-project orchestration.
- Prefer extensibility, trust, and low-friction task intake over GUI-first parity.
- Treat model and provider choice as a backend detail, not the product center.
- Favor features that strengthen the daemon loop, promotion pipeline, and reporting surfaces.
- Do not prioritize cloud-hosted control planes or chat-centric pair-programming UX.

## Now

### P1. Lifecycle hooks and policy engine

- Effort: Large
- Why this matters: Claude Code's hooks model is the clearest pattern for making automation composable. OpenLoop already has notifications, issue sync, provider abstraction, and event logs, but they are isolated features.
- Translate into repo:
  - Add global and project hook configuration.
  - Support `command` and `webhook` handlers first; leave Slack, Discord, and Linear as integrations built on top.
  - Fire hooks for daemon, task, validation, budget, and promotion events.
  - Let hooks attach structured notes to run summaries and, for approval-oriented events, block or require manual review.
- Acceptance criteria:
  - Users can register hooks without editing source code.
  - Hook payloads include project alias, task id, mode, outcome, validation summary, and budget snapshot.
  - Hook failures are logged to the daemon log and event log without crashing the loop.
  - One end-to-end sample integration works from config to delivery.

### P2. Approval packets for planned work

- Effort: Medium
- Why this matters: OpenLoop already pauses medium and high-risk tasks after planning, but the approval step is still thin. Plandex and Claude Code both make review artifacts first-class.
- Translate into repo:
  - Generate a review packet whenever a task moves to `awaiting-approval`.
  - Include the spec path, intended scope, likely high-risk paths, validation plan, and effective promotion mode.
  - Surface the packet in `task show`, `promotion show`, and the report output.
- Acceptance criteria:
  - Every `awaiting-approval` task has a generated review packet artifact.
  - Packets link back to the spec, task source, and scope policy.
  - Approving a task records reviewer metadata and timestamp in the ledger or artifact history.

### P3. Automated reviewer pass and confidence digest

- Effort: Medium
- Why this matters: The biggest adoption barrier across comparable tools is trust. OpenLoop needs a fast answer to "why should I believe this change is safe?"
- Translate into repo:
  - Add a reviewer pass after implementation and validation.
  - Inspect diff scope, validation results, and promotion mode to produce findings and a confidence digest.
  - Use reviewer findings to block auto-merge when unresolved risk signals remain.
- Acceptance criteria:
  - Successful implement runs emit a review summary with findings severity.
  - Auto-merge refuses tasks with blocking reviewer findings.
  - `task show` and `report` expose the digest without forcing users to read raw artifacts.

## Next

### P4. Context packs and repo maps

- Effort: Medium
- Why this matters: Aider and Plandex both improve large-repo behavior by persisting a project map instead of rebuilding context from scratch every run.
- Translate into repo:
  - Generate `.openloop/context/` artifacts with stack, entrypoints, risky directories, validation commands, and project glossary.
  - Feed the context pack into prompt construction and discovery heuristics.
  - Add a manual refresh command plus lightweight regeneration triggers on key config changes.
- Acceptance criteria:
  - Each initialized project can produce a stable context pack.
  - Prompt construction can include the context pack without requiring ad hoc file discovery each time.
  - Context generation is fast enough to run during setup or doctor without noticeable friction.

### P5. Inline task capture from source comments

- Effort: Medium
- Why this matters: Aider's `AI!` watch mode lowers capture friction. OpenLoop should borrow the intake pattern, not the interactive editing model.
- Translate into repo:
  - Watch for `OPENLOOP:` markers in code or docs.
  - Convert markers into scoped tasks tied to the file path and surrounding context.
  - Support deduplication and a clear way to resolve or ignore stale markers.
- Acceptance criteria:
  - Users can create backlog items by leaving inline `OPENLOOP:` markers.
  - Created tasks automatically record source file scope.
  - Repeated scans do not create duplicate tasks for the same marker.

### P6. Named autonomy profiles

- Effort: Small to medium
- Why this matters: Plandex frames autonomy as a controllable dial. OpenLoop already has the low-level controls, but they are too scattered for quick adoption.
- Translate into repo:
  - Add named profiles such as `safe`, `balanced`, and `overnight`.
  - Let profiles set approval thresholds, validation strictness, cooldowns, and auto-merge behavior.
  - Show the active profile in status, watch, and report output.
- Acceptance criteria:
  - Users can set a profile globally or per project.
  - Profile changes update the effective runtime behavior without hand-editing multiple config keys.
  - Reports and status surfaces show which profile governed a run.

## Later

### P7. Scheduled and event-driven execution

- Effort: Medium
- Why this matters: Claude Code routines and OpenHands GitHub actions confirm the demand for scheduled and event-triggered unattended work. This fits OpenLoop's daemon model better than more UI.
- Translate into repo:
  - Add cron-like schedules at global and project scope.
  - Add event triggers for issue sync, CI failure ingestion, or explicit webhook events.
  - Let schedules invoke `run-once`, issue sync, and morning report generation with guardrails.
- Acceptance criteria:
  - Users can define schedules in config without external cron wiring.
  - Scheduled runs respect budgets, pause state, and project policies.
  - Event-triggered runs are visible in the event log and daily reports.

### P8. Rich morning report

- Effort: Medium
- Why this matters: The existing report is useful but still mechanical. Overnight automation needs a decision-ready summary.
- Translate into repo:
  - Expand reporting with per-task cost, confidence digest, blocker reason, and recommended next action.
  - Highlight needs-attention items before neutral activity summaries.
  - Emit a compact machine-readable report for notification and webhook consumers.
- Acceptance criteria:
  - `openloop report` answers what finished, what failed, what changed, what it cost, and what needs a human next.
  - The report can be emitted as both readable text and structured JSON.
  - Notifications can include a condensed morning summary payload.

## Deprioritized

- Full GUI-first workflows as a primary product surface
- Cloud-hosted orchestration as the default operating model
- Generic chat-centric pair-programming features that do not strengthen unattended execution

## Audit Archive

### March 2026 audit status

- Audit date: 2026-03-16
- Audit completion: 32 of 32 audit items resolved

### Completed audit coverage

| Tier                     | Total  | Resolved |
| ------------------------ | ------ | -------- |
| T1 -- Safety-Critical    | 6      | 6        |
| T2 -- Usability-Critical | 10     | 10       |
| T3 -- Workflow           | 10     | 10       |
| T4 -- Strategic          | 6      | 6        |
| **Total**                | **32** | **32**   |

### Audit scope covered

- Daemon reliability
- CLI consistency
- Error handling
- Feedback loops
- Template and init gaps
- Workflow coherence
- Safety controls

### Archive note

The detailed item-by-item audit backlog was removed once every entry was implemented. Keeping finished tasks in the active backlog made the document misleading and left the old progress table incorrect.
