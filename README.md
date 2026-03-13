# OpenLoop

**Ship while you sleep.** OpenLoop turns [Pi](https://pi.dev) into a tireless developer that works across all your repositories — planning tasks, writing code, running validations, and promoting changes — on a loop, without manual intervention.

One CLI. One daemon. Every linked repo gets its own control plane, task ledger, and safety policy. You define the work; OpenLoop handles the rest.

## Quick Start

### 1. Install

```bash
git clone <repo-url> && cd openloop
npm install
npm run build
npm link        # makes `openloop` available globally
```

### 2. Link a project

```bash
openloop project add myapp ~/projects/my-app --init
```

This registers the project and materializes its control-plane files into `~/projects/my-app/.openloop/`.

You can also split the two steps (`project add` then `project init`) if you want to review config before initialization.

### 3. Configure validation (recommended)

OpenLoop auto-detects lint, test, and typecheck commands from `package.json` during `init`. Verify they look right:

```bash
openloop config project-show myapp
```

To override, edit `~/projects/my-app/.openloop/project.json`:

```json
{
  "validation": {
    "lintCommand": "npm run lint",
    "testCommand": "npm test",
    "typecheckCommand": "npm run check"
  }
}
```

### 4. Add a task

```bash
openloop task add --project myapp --title "Add input validation to the signup form"
```

Or enqueue from an issue reference:

```bash
openloop enqueue --project myapp --ref "https://github.com/org/repo/issues/42"
```

### 5. Run

Run a single scheduler iteration to try things out:

```bash
openloop run-once --project myapp
```

Or start the daemon for continuous, unattended operation:

```bash
openloop service start
openloop service status   # check progress
openloop service stop     # shut down
```

### 6. Review and promote

Check what was done and land the results:

```bash
openloop task list --project myapp
openloop task show --project myapp --task <task-id>
openloop promotion list --project myapp
openloop promotion apply --project myapp --task <task-id>
```

Low-risk tasks with passing validations can auto-merge. Everything else waits for your review.

## Requirements

- Node.js 20+
- `pi` available on `PATH`
- `git` available on `PATH`

## Development

```bash
npm run check     # type-check
npm test          # run tests
npm run build     # build CLI
```

## How It Works

OpenLoop cycles through three modes automatically:

1. **Active development** — picks the next eligible task, runs Pi with a role-specific prompt, validates the output, and decides whether to auto-merge or create a review branch.
2. **Self-healing** — detects and fixes lint errors, type errors, and localized test failures without being asked.
3. **Continuous improvement** — when idle, discovers gaps like missing validations or loose scope policies and proposes tasks to address them.

Each run is labeled with a worker role that shapes Pi's behavior:

### Runtime Model

- Pi runs as a subprocess via the `pi` binary.
- Model resolution is deterministic: CLI flag > project config > global config.
- A single resident daemon manages scheduling across all linked projects.
- One active worker per project at a time; the daemon sleeps between iterations.
- Daemon state persists under `~/.openloop/run/`.

### Worker Roles

| Role | When |
|------|------|
| `sdd-planner` | A task needs planning before code changes |
| `implementer` | A task is ready for implementation |
| `repo-improver` | The scheduler is doing discovery or idle improvement work |
| `ci-healer` | The scheduler is handling ledger-driven self-healing |

### Self-Healing

V1 supports automatic fixes for:

- `lint-fix` — linter violations
- `type-fix` — type-checker errors
- `localized-test-fix` — isolated test failures

Anything outside these categories is blocked rather than attempted unattended.

### Risk and Promotion

| Risk | Auto-merge | Human review |
|------|------------|--------------|
| `low-risk` | Allowed when policy permits and validations pass | Optional |
| `medium-risk` | Not allowed | Required |
| `high-risk` | Never | Required |

Tasks can declare `scope.paths`, and the runtime enforces project policy:

- **allow** — `allowGlobs` whitelist
- **block** — `denyGlobs` blacklist
- **escalate** — `highRiskAreas` require human review
- **default** — unknown scope is classified conservatively

### Safety Guardrails

| Guard | Default | Details |
|-------|---------|----------|
| Budget ceiling | $25/day | Daemon pauses when exhausted |
| Max attempts | 3 per task | Task is blocked after repeated failures |
| Run timeout | 30 min | Hard cap per Pi invocation |
| No-progress detection | On | Blocks tasks with ineffective diffs or repeated errors |
| Scope policy | Per-project | `allowGlobs`, `denyGlobs`, `highRiskAreas` in `.openloop/policy.yaml` |
| Auto-merge | Low-risk only | All validations must pass; medium/high-risk always requires review |

## CLI Reference

### Project Management

| Command | Description |
|---------|-------------|
| `project add <alias> <path> [--init]` | Register a repository |
| `project init <alias>` | Materialize control-plane templates |
| `project list` | List linked projects |
| `project show <alias>` | Show project details |

### Tasks

| Command | Description |
|---------|-------------|
| `task add --project <alias> --title <text>` | Add a task (accepts `--kind`, `--risk`, `--scope`) |
| `task list --project <alias>` | List tasks (accepts `--status`, `--risk` filters) |
| `task show --project <alias> --task <id>` | Show task detail with promotion history |
| `enqueue --project <alias> --ref <ref>` | Create a task from an issue or external reference |

### Execution

| Command | Description |
|---------|-------------|
| `run --project <alias> --prompt <text>` | Run Pi directly with a prompt |
| `run-once --project <alias>` | Execute one scheduler iteration |
| `service start\|stop\|status\|restart` | Manage the resident daemon |
| `pause` / `resume` | Pause/resume the daemon globally |

### Promotion

| Command | Description |
|---------|-------------|
| `promotion list --project <alias>` | List promotion artifacts |
| `promotion show --project <alias> --task <id>` | Show promotion detail |
| `promotion history --project <alias> --task <id>` | Chronological promotion log |
| `promotion apply --project <alias> --task <id>` | Apply a pending promotion (merge or checkout) |
| `promotion reject --project <alias> --task <id>` | Reject a pending promotion |

### Configuration

| Command | Description |
|---------|-------------|
| `config show` | Show global configuration |
| `config set-model <model>` | Set default Pi model |
| `config project-show <project>` | Show project-local config |
| `config project-set-model <project> <model>` | Set project Pi model |

## Project Structure

```
~/.openloop/              # global state
  config.json             # model, budgets, runtime limits
  projects.json           # linked project registry
  run/                    # daemon pid, state, logs

<project>/.openloop/      # per-project control plane
  project.json            # project runtime config
  policy.yaml             # scope and risk policy
  tasks.json              # task ledger
  specs/                  # planning artifacts
  runs/                   # run summaries
  promotions/             # promotion decision artifacts
  promotion-results/      # promotion outcome records

<project>/.agents/skills/openloop/
  SKILL.md                # agent instructions inside linked repositories

<project>/.pi/
  SYSTEM.md               # base Pi system prompt
  APPEND_SYSTEM.md        # runtime-specific Pi rules
```

## Release

Before publishing:

```bash
npm run release:verify    # validates tarball contents
npm run release:pack      # verify + pack
```

The npm tarball includes `README.md`, `dist/`, and `templates/project/` only. Internal docs and source maps are excluded.

## Roadmap (Post-V1)

- Cron / timer-triggered execution
- `git worktree` support
- Webhook-driven CI self-healing
- Direct spec-file generation
- Broader heuristic backlog discovery