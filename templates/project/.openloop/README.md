# Project Control Plane

This directory contains Openloop control-plane files for a linked repository.

## Files

| File / Directory | Purpose |
|-----------------|----------|
| `project.json` | Project-local runtime, Pi model, and validation settings |
| `policy.yaml` | Scope rules (`allowGlobs`, `denyGlobs`, `highRiskAreas`), risk classes, and promotion modes |
| `tasks.json` | Task ledger — backlog, execution state, and run history |
| `tasks.schema.json` | Machine-readable task contract (JSON Schema) |
| `specs/` | Specs generated during the planning workflow |
| `runs/` | Run summary markdown files |
| `promotions/` | Promotion decision artifacts (`pending` → `applied` or `rejected`) |
| `promotion-results/` | Outcome records for applied or rejected promotions |

## Task Scope

Task entries may include `scope.paths` so the runtime can enforce policy allowlists, deny lists, and high-risk areas before execution begins.

## Runtime-Managed

The Openloop runtime manages `tasks.json`, `runs/`, `promotions/`, and `promotion-results/`. Do not edit them manually during active runs.
