# OpenLoop Rules

## Purpose

This repository builds `openloop`, a global CLI and daemon for unattended AI-driven development across linked project repositories. Pi is the default agent, but the runtime supports Claude Code, Aider, Codex, OpenCode, ka, Oh My Pi, and custom commands via a provider abstraction.

## Product Boundaries

OpenLoop itself is not a target project.

Its responsibility is to:

- register linked repositories by alias
- materialize project control-plane templates into target repos
- schedule and run the Pi agent loop
- manage daemon state and service commands
- keep global config separate from per-project config

## Repository Layout

- `src/` contains the Rust application code (`core/` domain, `cli/` commands, `tui/` interactive interface)
- `tests/` contains Rust integration tests (unit logic is exercised through the library target)
- `templates/project/` contains files materialized into linked repositories (embedded into the binary at build time)
- `bin/` and `scripts/` hold the npm distribution shim (the npm package wraps the native binary; there are no runtime JavaScript dependencies)
- `docs/` holds design and port-history notes

Do not treat root-level files in this repository as if they were the live control plane of a target project.

## Implementation Priorities

1. Keep the app multi-project from the start.
2. Keep config split between global app state and per-project state.
3. Prefer simple JSON/YAML files over a database.
4. Keep the daemon single-threaded with subprocess execution for agent tasks; no async runtime.
5. Avoid coupling to fixed project path conventions.

## Global State

The app-level runtime uses `~/.openloop/` for:

- global config
- linked project registry
- daemon pid/state/log files
- the append-only event audit log

## Per-Project State

The app materializes project-local control-plane files into linked repositories via `templates/project/`.

Those templates define `.openloop/`, `.agents/skills/openloop/SKILL.md`, and `.pi/` for the target repository.

## Git Rules

- Never rewrite unrelated user changes in a linked project.
- Allow dirty repositories, but do not overwrite foreign modifications.
- Runs may use an isolated git worktree (opt-in via `runtime.useWorktree`); worktree support is required when enabled. Worktree setup commands run under the run timeout budget and fail closed.

## Decision Style

- prefer standard Rust choices when no stronger reason exists (std threads/subprocesses over async runtimes; serde for state)
- prefer minimal, composable layers: core, cli, tui
- prefer explicit project alias targeting over cwd magic
- prefer deterministic file-based state over hidden service state
- subprocess kills must always reap children (SIGTERM → SIGKILL escalation → wait)
