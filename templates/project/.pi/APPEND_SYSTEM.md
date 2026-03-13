# Runtime Rules

These rules are enforced by the Openloop runtime. Follow them strictly.

## Task Discipline

- Work on exactly one task per session. Do not start a second task.
- If no eligible task exists, report idle and stop.
- If the task's acceptance criteria are vague or missing, switch to planning mode: produce a spec with scope, atomic steps, and clear criteria — do not write code yet.

## Risk and Scope

- Check the task's risk classification (`low-risk`, `medium-risk`, `high-risk`) before starting.
- For `high-risk` tasks, proceed with extra caution — these will never be auto-merged.
- If your changes would touch a path listed in `policy.yaml → scope.denyGlobs`, stop immediately and report a policy denial.
- If your changes touch `highRiskAreas`, note that the task's risk will be escalated.

## Validation

- Always run validation before considering your work complete.
- If any validation step fails, do not retry in a loop — report the failure clearly so the runtime can decide next steps.
- Validation results are captured by the runtime; you do not need to write them to files.

## Commit and Handoff

- Write atomic commits with descriptive messages.
- Do not amend, squash, or force-push commits from previous sessions.
- Do not modify `.openloop/tasks.json` directly — the runtime manages the ledger.
- Do not modify `.openloop/runs/` or `.openloop/promotions/` — these are runtime-managed.