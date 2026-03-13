# OpenLoop App Rules

## Purpose

This repository builds `openloop`, a global CLI and daemon for unattended Pi-driven development across linked project repositories.

## Product Boundaries

OpenLoop itself is not a target project.

Its responsibility is to:

- register linked repositories by alias
- materialize project control-plane templates into target repos
- schedule and run the Pi agent loop
- manage daemon state and service commands
- keep global config separate from per-project config

## Repository Layout

- `src/` contains the application code
- `templates/project/` contains files materialized into linked repositories
- `tests/` contains integration and regression tests

Do not treat root-level files in this repository as if they were the live control plane of a target project.

## Implementation Priorities

1. Keep the app multi-project from the start.
2. Keep config split between global app state and per-project state.
3. Prefer simple JSON/YAML files over a database in V1.
4. Keep the daemon single-threaded with subprocess execution for Pi tasks.
5. Avoid coupling to fixed project path conventions.

## Global State

The app-level runtime should use `~/.openloop/` for:

- global config
- linked project registry
- daemon pid/state/log files

## Per-Project State

The app materializes project-local control-plane files into linked repositories via `templates/project/`.

Those templates define `.openloop/`, `.agents/skills/openloop/SKILL.md`, and `.pi/` for the target repository.

## Git Rules

- Never rewrite unrelated user changes in a linked project.
- Allow dirty repositories, but do not overwrite foreign modifications.
- In V1, use isolated branches; do not assume `git worktree` support exists.

## Decision Style

- prefer the most standard Node/TypeScript choices when no stronger reason exists
- prefer minimal, composable layers: core, cli, daemon, templates
- prefer explicit project alias targeting over cwd magic
- prefer deterministic file-based state over hidden service state
