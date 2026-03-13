---
name: openloop
description: "Use when working inside a repository linked to Openloop for task-driven planning, implementation, policy checks, and promotion decisions based on .openloop control-plane files."
---

# Openloop Repository Skill

## Purpose

This skill defines how Openloop operates inside this linked repository. It does not replace any existing `AGENTS.md` owned by the project.

## Control Plane Files

Before planning or changing code, read these files:

1. `.openloop/tasks.json` — task ledger with status, risk, scope, and acceptance criteria
2. `.openloop/policy.yaml` — scope rules (`allowGlobs`, `denyGlobs`, `highRiskAreas`), risk classes, and promotion modes
3. `.openloop/project.json` — validation commands, Pi model, runtime settings
4. Relevant spec under `.openloop/specs/` if one exists for the current task

Pi-specific system prompts live in `.pi/SYSTEM.md` and `.pi/APPEND_SYSTEM.md`.

## Workflow

1. Read the task ledger and select the assigned task.
2. Check risk classification and promotion mode from project policy.
3. If the task's acceptance criteria are vague, plan first — produce a spec under `.openloop/specs/` with scope, steps, and criteria.
4. Implement with minimal, focused changes within the task's declared scope.
5. Run the project's validation commands (lint, test, typecheck) as configured in `project.json`.
6. Stop on validation failure — do not retry in a loop.
7. Commit changes with a descriptive message.

## Scope Policy

This repository defines its own unattended scope in `.openloop/policy.yaml`.

- Do not touch paths in `denyGlobs`.
- Stay within `allowGlobs` when defined.
- Note that `highRiskAreas` paths escalate the task's risk classification.
- Do not assume fixed protected paths — use the project policy.

## What The Runtime Manages

Do not modify these files directly — the Openloop runtime manages them:

- `.openloop/tasks.json` (task status, attempts, run summaries)
- `.openloop/runs/` (run summary markdown files)
- `.openloop/promotions/` and `.openloop/promotion-results/` (promotion artifacts)
