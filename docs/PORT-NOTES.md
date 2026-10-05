# Port Notes — TypeScript → Rust

The Rust implementation is the sole implementation at the repository root.
This note records what the port replaced, the compatibility contract that
survived it, and the known limits. The TypeScript tree itself only exists in
git history (`git show main:<path>`).

## Mapping

| TS subsystem | Rust counterpart | Status |
|---|---|---|
| `src/core/*` (37 modules) | `src/core/*` | 1:1 |
| `src/daemon/worker.ts` | `src/core/worker.rs` | 1:1 |
| `src/cli/commands/*` (17 commands) | `src/cli/*` (clap) | 1:1 |
| `src/tui/*` (OpenTUI/Solid) | `src/tui/*` (ratatui) | redesigned — see TUI-REDESIGN.md |
| `templates/project/` | embedded via `include_dir` | 1:1 |
| vitest suites (`tests/unit`, `tests/integration`) | Rust integration suites (`tests/*.rs`) | behavioral assertions ported; wiring/mock checks dropped |

Two review passes audited the port (round 1: parity/robustness findings;
round 2: fix-verification, test-quality, and cross-interference audits).
Their findings and fixes are part of the commit history.

## Compatibility contract

State formats are byte-compatible with the TypeScript era so a shared
`~/.openloop` keeps working across both lineages:

- Same JSON/YAML files: `config.json`, `projects.json`,
  `daemon-state.json`, `tasks.json`, promotions, approvals,
  `run/events.jsonl`, `daemon.log` (10 MiB rotation, `.1`–`.3` backups).
- camelCase keys, field-for-field with the TS structs; integral numbers
  serialize without a trailing `.0` (JSON.stringify parity); object key
  order follows struct declaration order.
- Artifact/summary filenames strip `[:.]` → `-`.
- `events --since 0` / truthiness rules (empty-string validation commands
  are "unconfigured") mirror the TS semantics exactly.

Breaking this contract breaks existing installs — changes here need an
explicit migration story.

## Known limitations

- `setup` and `watch` require a real TTY (same as TS).
- Error/usage wording is aligned but not byte-identical (clap vs yargs).
- `doctor` remains subprocess-bound (`which`/`--version`), so the binary's
  startup advantage does not apply to it.
- The dashboard HTTP server is localhost-only and hand-rolled (no TLS).

## Deliberately not ported

- Byte-level help-text and TTY-render test assertions (vitest wiring checks
  that re-pinned incidental output rather than behavior).
- Mock-echo unit tests with no behavioral teeth.
- The TUI's Solid/OpenTUI component tree (the ratatui TUI is a redesign,
  not a port — see TUI-REDESIGN.md).
