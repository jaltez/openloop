#!/usr/bin/env python3
"""Fabricate a believable OPENLOOP_HOME + project control planes for the
README screenshot. Idempotent wipe of /tmp/openloop-demo."""
import json, os, shutil, time

ROOT = "/tmp/openloop-demo"
HOME = f"{ROOT}/home"
NOW = time.time()

def iso(delta_s):
    return time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime(NOW + delta_s))

def day(days):
    return time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime(NOW - days * 86400))

shutil.rmtree(ROOT, ignore_errors=True)
os.makedirs(f"{HOME}/run")

projects = [("atlas-api", 41), ("ledger-web", 27), ("infra-tf", 12), ("docs-site", 6)]

# --- global config ---------------------------------------------------------
json.dump({
    "version": 1,
    "model": "zai/glm-4.6",
    "defaultProvider": "pi",
    "activeProjectAlias": "atlas-api",
    "budgets": {"dailyCostUsd": 10.0, "estimatedCostPerRunUsd": 0.12},
    "runtime": {
        "runTimeoutSeconds": 1800,
        "maxAttemptsPerTask": 3,
        "noProgressRepeatLimit": 2,
        "tickIntervalSeconds": 5,
        "maxConcurrentRuns": 2,
        "prRefreshIntervalSeconds": 900,
        "projectSelectionStrategy": "round-robin",
    },
}, open(f"{HOME}/config.json", "w"), indent=2)

# --- project registry ------------------------------------------------------
json.dump({
    "version": 1,
    "projects": [
        {"alias": a, "path": f"{ROOT}/repos/{a}", "defaultBranch": "main",
         "initialized": True, "createdAt": day(d), "updatedAt": iso(-3600 * (i + 1))}
        for i, (a, d) in enumerate(projects)
    ],
}, open(f"{HOME}/projects.json", "w"), indent=2)

# --- daemon state ----------------------------------------------------------
run_a = {"projectAlias": "atlas-api", "taskId": "TASK-7f3a", "mode": "code",
         "role": "implementer", "startedAt": iso(-745), "deadlineAt": iso(+1035),
         "attemptNumber": 2, "pauseRequestedAt": None}
run_b = {"projectAlias": "ledger-web", "taskId": "TASK-c41d", "mode": "review",
         "role": "reviewer", "startedAt": iso(-402), "deadlineAt": iso(+1398),
         "attemptNumber": 1, "pauseRequestedAt": None}
json.dump({
    "version": 1,
    "startedAt": iso(-7540),  # ~2h05m uptime
    "pid": 48213,
    "activeProject": "atlas-api",
    "paused": False,
    "totalBudgetSpentUsd": 41.87,
    "budgetDate": time.strftime("%Y-%m-%d", time.gmtime(NOW)),
    "budgetSpentUsd": 3.42,
    "budgetBlocked": False,
    "currentRun": run_a,
    "currentRuns": [run_a, run_b],
    "projects": [
        {"alias": "atlas-api", "queueSize": 3, "paused": False,
         "lastIterationAt": iso(-745), "lastResult": "running", "blockedTasks": 0},
        {"alias": "ledger-web", "queueSize": 5, "paused": False,
         "lastIterationAt": iso(-402), "lastResult": "failed", "blockedTasks": 2},
        {"alias": "infra-tf", "queueSize": 1, "paused": False,
         "lastIterationAt": iso(-918), "lastResult": "promoted", "blockedTasks": 0},
        {"alias": "docs-site", "queueSize": 0, "paused": False,
         "lastIterationAt": iso(-3960), "lastResult": "completed", "blockedTasks": 0},
    ],
}, open(f"{HOME}/run/daemon-state.json", "w"), indent=2)

# --- event log (chronological, within the 1h dashboard window) -------------
events = [
    ("daemon_started", None, None, {"pid": 48213}, -3540),
    ("schedule_fired", "ledger-web", "TASK-c41d", {}, -3130),
    ("run_started", "atlas-api", "TASK-2b90",
     {"mode": "code", "role": "implementer"}, -2820),
    ("pi_completed", "infra-tf", "TASK-2b90",
     {"exitCode": 0, "mode": "code", "role": "implementer",
      "taskStatus": "done", "budgetSnapshotUsd": 2.71}, -2460),
    ("dependency_satisfied", "ledger-web", "TASK-9d10",
     {"detail": "atlas-api@main promoted — TASK-2b90"}, -2110),
    ("run_started", "ledger-web", "TASK-c41d",
     {"mode": "code", "role": "implementer"}, -1740),
    ("pi_completed", "atlas-api", "TASK-2b90",
     {"exitCode": 0, "mode": "code", "role": "implementer",
      "taskStatus": "promoted", "budgetSnapshotUsd": 3.05}, -1320),
    ("task_skipped", "docs-site", None, {"mode": "idle", "stoppedBy": "no-op"}, -960),
    ("run_started", "atlas-api", "TASK-7f3a",
     {"mode": "code", "role": "implementer"}, -745),
    ("hook_failed", "ledger-web", None,
     {"hookEvent": "run_completed", "hookType": "command",
      "error": "notify-matrix.sh: token expired"}, -240),
]
with open(f"{HOME}/run/events.jsonl", "w") as f:
    for name, proj, task, extra, delta in events:
        e = {"ts": iso(delta), "event": name, **extra}
        if proj: e["project"] = proj
        if task: e["taskId"] = task
        f.write(json.dumps(e) + "\n")

# --- per-project task ledgers ----------------------------------------------
def task(tid, title, kind, status, risk, delta, *, attempts=0,
         promo="pull-request", source=("cli", "manual"), branch=None, deps=None):
    t = {
        "id": tid, "title": title, "kind": kind, "status": status, "risk": risk,
        "source": {"type": source[0], "ref": source[1]},
        "acceptanceCriteria": ["Change is covered by tests", "CI green on main"],
        "attempts": attempts, "promotion": promo,
        "createdAt": iso(delta), "updatedAt": iso(delta / 4),
    }
    if branch: t["branch"] = branch
    if deps: t["dependsOn"] = deps
    return t

ledgers = {
    "atlas-api": [
        task("TASK-7f3a", "Harden JWT refresh-token rotation", "feature",
             "in_progress", "medium-risk", -5 * 86400, attempts=2,
             source=("github-issue", "482"), branch="openloop/task-7f3a"),
        task("TASK-8c22", "Add rate-limit headers to /v1/keys", "feature",
             "ready", "low-risk", -3 * 86400, source=("github-issue", "497")),
        task("TASK-9d10", "Migrate session store to Redis", "refactor",
             "blocked", "high-risk", -8 * 86400, deps=["TASK-8c22"]),
        task("TASK-2b90", "Fix flaky webhook-delivery e2e test", "bugfix",
             "promoted", "low-risk", -6 * 86400, attempts=1, promo="auto-merge",
             branch="openloop/task-2b90"),
        task("TASK-4e55", "Document key-rotation runbook", "docs",
             "proposed", "low-risk", -1 * 86400),
    ],
    "ledger-web": [
        task("TASK-c41d", "Reconcile double-entry drift on refunds", "bugfix",
             "in_progress", "medium-risk", -4 * 86400, attempts=1,
             source=("github-issue", "1123"), branch="openloop/task-c41d"),
        task("TASK-b30f", "Upgrade to React 19 stable", "feature",
             "awaiting-approval", "medium-risk", -2 * 86400, promo="manual-only"),
    ],
    "infra-tf": [
        task("TASK-e19", "Pin module versions in staging", "refactor",
             "ready", "low-risk", -5 * 86400),
    ],
    "docs-site": [
        task("TASK-11e", "Refresh install docs for npm shim", "docs",
             "ready", "low-risk", -2 * 86400),
    ],
}
for alias, tasks in ledgers.items():
    d = f"{ROOT}/repos/{alias}/.openloop"
    os.makedirs(d)
    json.dump({"version": 1, "updatedAt": iso(-600), "tasks": tasks},
              open(f"{d}/tasks.json", "w"), indent=2)

print("demo state ready at", ROOT)
