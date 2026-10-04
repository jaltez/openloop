# OpenLoop Rust Port

A 1:1 functional port of the TypeScript application to Rust. The port is
now the sole implementation at the repository root; the TypeScript tree was
removed in the cutover commit.

## Build

```sh
cargo build --release
# binary: target/release/openloop (4.7 MB, system libc linked)
```

## Scope of the port

| TS subsystem | Rust counterpart | Status |
|---|---|---|
| `src/core/*` (37 modules) | `src/core/*` | 1:1 |
| `src/daemon/worker.ts` | `src/core/worker.rs` | 1:1 |
| `src/cli/commands/*` (17 commands) | `src/cli/*` (clap) | 1:1 |
| `src/tui/*` (OpenTUI/Solid) | `src/tui/*` (ratatui) | approximate — redesigned beyond parity, see TUI-REDESIGN.md |
| `templates/project/` | embedded in the binary (`include_dir`) | 1:1 |

Design decisions:

- **Pure std, no tokio**: the daemon is single-threaded with subprocesses,
  exactly like the original. Agent timeouts are enforced by polling plus
  `kill(-pgid)` with SIGTERM→SIGKILL escalation; every kill path reaps its
  child so no zombies outlive the daemon.
- **Identical state formats**: same JSON/YAML files (`config.json`,
  `projects.json`, `daemon-state.json`, `tasks.json`, promotions, approvals,
  events.jsonl). One `~/.openloop` home can be shared by the TS and Rust
  binaries, including artifact filename conventions (`[:.]` → `-`).
- **8 agent providers** (pi, claude, aider, codex, opencode, ka, omp, custom)
  with TS-parity usage parsing (last-line JSON vs whole-object per provider;
  custom intentionally parses nothing).
- **TUI**: ratatui with 9 views (Dashboard, Projects, Tasks, Service, Config,
  Run, Promotions, Events, Logs), tab bar, status bar, help overlay (`?`),
  1-9/Tab navigation, inline inputs. Polling is gated on the active tab and
  log/event reads are tail-bounded.
- Outbound HTTP (webhooks, GitHub/GitLab) via `ureq`; the web dashboard runs
  on a hand-rolled HTTP server (GET-only, localhost-only, snapshot-cached).

## Metrics (measured on this repository, 2026-10-01)

| Metric | TS (node dist) | TS (bun dist) | Rust release | Rust/Node ratio |
|---|---|---|---|---|
| `--help` wall | 76.8 ms | 182.2 ms | **1.2 ms** | ~64x faster |
| `status` wall | 78.7 ms | — | **1.1 ms** | ~72x |
| `events` wall | 79.1 ms | — | **1.1 ms** | ~72x |
| `doctor` wall | 270.9 ms | — | **~170 ms** | ~1.6x (subprocess-bound) |
| RSS `--help` | 79.3 MB | 95.0 MB | **3.4 MB** | ~23x less |
| RSS idle daemon | ~71–75 MB | — | **5.0–5.2 MB** | ~14x less |
| Deployable artifact | dist 756 KB + node_modules 258 MB + node 118 MB | same | **4.7 MB binary** | ~80x less disk |
| Runtime dependency | Node >= 22 / Bun | same | none (standard libc) | |

Notes:

- The Rust doctor remains dominated by `which`/`--version` subprocesses; the
  startup advantage does not apply to externally bound commands.
- The Rust daemon's RSS is stable (5.0→5.2 MB across 16 s of ticking);
  Node's drifts 74→71 MB after the first GC.
- Per-tick CPU is negligible in both; the daemon sleeps between 5 s ticks.

## Review-driven hardening (second pass)

A three-part review (performance, feature parity, live functionality)
produced a ranked fix list; all of it landed with regression coverage:

**Blocking (both reproduced live, then fixed and re-verified)**

1. SIGTERM during an active agent run now interrupts the run within ~250 ms:
   the subprocess poll loop observes the shutdown flag, kills the agent's
   process group, and the daemon removes its pid file and exits. Previously
   `service stop` could leave the daemon (and billable agents) alive for up
   to the 30-minute run timeout.
2. `config set` now accepts every integer-typed tunable (`runTimeoutSeconds`,
   `maxAttemptsPerTask`, `noProgressRepeatLimit`, `tickIntervalSeconds`).
   Whole numbers serialize as JSON integers so the u64/u32 config fields
   round-trip; non-finite values (`Infinity`, `NaN`) are rejected like TS.

**Robustness / resource**

3. The worktree setup command runs under the run's remaining timeout budget
   and fails closed (previously it could block the single-threaded daemon
   forever).
4. All timeout/shutdown kill paths reap their children (SIGTERM → 5 s grace →
   SIGKILL → `wait`), eliminating zombie accumulation.
5. The 4 MiB agent-output capture buffer trims in 256 KiB quanta instead of
   rebuilding per 16 KiB chunk (~256x fewer large memcpys for chatty agents).

**Parity**

6. Artifact/summary filenames strip both `:` and `.` (TS `[:.]` → `-`),
   keeping a shared home byte-compatible.
7. PR creation treats a non-zero exit as failure and never adopts failing
   stdout as a PR URL.
8. The daily budget window anchors to the daemon's fixed start date (TS
   semantics); a running daemon never rolls over at local midnight.
9. Corrupt promotion artifacts and malformed `policy.yaml` are hard errors
   instead of silent defaults (scope enforcement can no longer vanish).
10. `enqueue --ref` is a flag again; enum CLI flags (`--status/--risk/--kind`,
    promotion `--status`) validate values and reject typos; junk config keys
    (`__unused*`) are gone.
11. TUI polling is gated on the active tab; `events.jsonl` and `daemon.log`
    reads are tail-bounded (256 KiB / 64 KiB).
12. Issue comments require HTTP 201 exactly and match the TS body joins;
    webhook timeout errors distinguish timeouts from transport failures.
13. Minor drift batch: the `__iteration_timeout__` sentinel no longer leaks
    into `daemon-state.lastResult`; the LLM reviewer is skipped when the git
    diff is unavailable; a missing policy risk class errors instead of
    weakening promotion gating; `daemon.log` lines use the `[iso] message`
    format; `state`/`checks` keys are omitted (not null) on non-refreshed
    promotion results; the secret matcher drops a no-counterpart variant;
    digest cost parsing takes the first matching line per field; an
    unparseable `lastSyncedAt` no longer triggers an immediate re-sync.

## Tests

`cargo test` runs 92 tests across 14 suites:

- Unit: cron (5-field matcher incl. dom/dow OR-rule), secret scanning and
  scope-drift review checks, policy glob semantics (single-star crossing,
  double-star, `?`, deny/allow/high-risk escalation), scheduler selection
  (aging, dependsOn gating, cross-project refs), promotion decisions,
  no-progress repeat counting, run-cost resolution, usage parsing, digest
  confidence, stack detection (node/npm/bun runners, python/go/rust/
  java/gradle/maven/dotnet/ruby), atomic JSON writes and tail reads.
- Regression: config-set integer coercion (the exact `10.0` → u64 failure),
  worktree setup timeout (a 30 s sleep under a 300 ms budget must be killed),
  filename charset, and the SIGTERM-during-run path (verified live with a
  sleeping agent; the daemon exits in <250 ms and the agent group dies).

Deliberately NOT ported from the TS vitest suite: byte-level help-text
assertions, TTY/render tests, and other wiring checks that re-pin incidental
output rather than behavior.

## Verification (functional parity)

An identical scenario ran against both binaries with isolated homes
(`project add --init` → `task add` low-risk → custom provider → `run-once`
plan → commit → `run-once` implement → promotion list):

| Field | TS | Rust |
|---|---|---|
| plan: mode/role/exitCode/taskStatus | plan / sdd-planner / 0 / ready | identical |
| implement: taskStatus/stoppedBy | blocked / no-progress | identical |
| implement: decision/action | manual-review / queue-review | identical |
| promotion queue | pending manual-review | identical |
| costUsd/costSource | 0.1 / estimated | identical |
| dirtyTreeDetected | false | identical |

Also verified live in Rust: full daemon lifecycle (start/5 s ticks/pause/
resume/stop, events.jsonl, daily digest, budget window anchored to start
date), MCP stdio (initialize/tools/list/tools/call), web dashboard (HTML +
`/api/*`), and the TUI in a PTY (9 tabs, help overlay, clean quit).

## Known limitations

- `setup` and `watch` require a real TTY (same as TS).
- Error/usage messages are aligned but not byte-identical (clap vs yargs).
- The TS vitest suite is not ported wholesale; the Rust suite covers the
  pure-logic units plus regressions for every fix above.
