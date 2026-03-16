export interface GlobalConfig {
  version: 1;
  model?: string | null;
  activeProjectAlias?: string | null;
  budgets: {
    dailyCostUsd: number;
    estimatedCostPerRunUsd?: number;
  };
  runtime: {
    runTimeoutSeconds: number;
    maxAttemptsPerTask: number;
    noProgressRepeatLimit: number;
    tickIntervalSeconds?: number;
    projectSelectionStrategy?: "round-robin" | "priority" | "focus";
  };
  notifications?: {
    onTaskComplete: string | null;
    onTaskFailed: string | null;
    onBudgetBlocked: string | null;
    onAllTasksDone: string | null;
  };
}

export interface ProjectConfig {
  version: 1;
  project: {
    alias: string | null;
    repoRoot: string | null;
    initializedAt: string | null;
  };
  pi: {
    model: string | null;
    promptFiles: string[];
  };
  agent?: {
    type: "pi" | "claude" | "aider" | "custom";
    command: string | null;
  };
  runtime: {
    autoCommit: boolean;
    useWorktree: boolean;
    branchPrefix: string;
    prCommand?: string | null;
  };
  validation: {
    lintCommand: string | null;
    testCommand: string | null;
    typecheckCommand: string | null;
  };
  risk: {
    defaultUnknownAreaClassification: "low-risk" | "medium-risk" | "high-risk";
    requirePolicyForAutoMerge: boolean;
  };
}

export interface LinkedProject {
  alias: string;
  path: string;
  defaultBranch: string | null;
  initialized: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface ProjectsRegistry {
  version: 1;
  projects: LinkedProject[];
}

export interface DaemonProjectState {
  alias: string;
  queueSize: number;
  paused: boolean;
  lastIterationAt: string | null;
  lastResult: string | null;
  blockedTasks: number;
}

export interface DaemonCurrentRunState {
  projectAlias: string;
  taskId: string | null;
  mode: SchedulerSelection["mode"];
  role: WorkerRole;
  startedAt: string;
  deadlineAt: string;
  attemptNumber: number;
  pauseRequestedAt: string | null;
}

export interface DaemonState {
  version: 1;
  startedAt: string;
  pid: number;
  activeProject: string | null;
  paused: boolean;
  pausedAt: string | null;
  totalBudgetSpentUsd: number;
  budgetDate: string;
  budgetSpentUsd: number;
  budgetBlocked: boolean;
  currentRun: DaemonCurrentRunState | null;
  projects: DaemonProjectState[];
}

export type RunStoppedBy = "none" | "timeout" | "budget" | "pause" | "no-progress";

export type WorkerRole = "sdd-planner" | "implementer" | "repo-improver" | "ci-healer";

export interface TaskSource {
  type: "human" | "issue" | "discovery" | "ci" | "spec";
  ref: string;
}

export const SUPPORTED_SELF_HEALING_TASK_KINDS = ["lint-fix", "type-fix", "localized-test-fix"] as const;

export type SupportedSelfHealingTaskKind = (typeof SUPPORTED_SELF_HEALING_TASK_KINDS)[number];

export interface TaskScope {
  paths: string[];
}

export interface ProjectTask {
  id: string;
  title: string;
  kind:
    | "feature"
    | "bugfix"
    | "test"
    | "refactor"
    | "docs"
    | "lint-fix"
    | "type-fix"
    | "localized-test-fix"
    | "ci-heal"
    | "discovery"
    | "scope-proposal";
  status: "proposed" | "planned" | "ready" | "in_progress" | "blocked" | "done" | "failed" | "cancelled" | "promoted";
  risk: "low-risk" | "medium-risk" | "high-risk";
  scope?: TaskScope | null;
  source: TaskSource;
  specId: string | null;
  branch: string | null;
  owner: string | null;
  acceptanceCriteria: string[];
  attempts: number;
  lastFailureSignature: string | null;
  promotion: "auto-merge" | "pull-request" | "manual-only";
  promotedAt?: string | null;
  notes?: string[];
  lastRun?: TaskRunSummary;
  createdAt: string;
  updatedAt: string;
}

export interface TaskLedger {
  version: 1;
  updatedAt: string;
  tasks: ProjectTask[];
}

export interface SchedulerSelection {
  task: ProjectTask | null;
  mode: "implement" | "plan" | "idle";
  reason: string;
}

export interface SchedulerResult {
  projectAlias: string;
  taskId: string | null;
  mode: "implement" | "plan" | "idle";
  role: WorkerRole | null;
  reason: string;
  model: string | null;
  exitCode: number | null;
  prompt?: string | null;
  validation: ValidationSummary[];
  promotionDecision: "none" | "auto-merge-eligible" | "manual-review" | "blocked";
  promotionAction: "none" | "queue-auto-merge" | "queue-review" | "block";
  promotionArtifactPath: string | null;
  promotionResultArtifactPath: string | null;
  taskStatus: ProjectTask["status"] | null;
  promotedAt: string | null;
  stoppedBy: RunStoppedBy;
  attemptNumber: number | null;
  dirtyTreeDetected: boolean;
  budgetSnapshotUsd: number | null;
}

export interface ValidationSummary {
  name: "lint" | "test" | "typecheck";
  command: string;
  exitCode: number;
}

export interface TaskRunSummary {
  completedAt: string;
  mode: "implement" | "plan";
  role?: WorkerRole;
  piExitCode: number | null;
  outcome: "planned" | "completed" | "validation-failed" | "pi-failed" | "error";
  baseBranch: string | null;
  validation: ValidationSummary[];
  promotionDecision: SchedulerResult["promotionDecision"];
  effectivePromotionMode: ProjectTask["promotion"];
  promotionAction: SchedulerResult["promotionAction"];
  promotionArtifactPath: string | null;
  promotionArtifactState: PromotionArtifactStatus;
  promotionResultArtifactPath: string | null;
}

export type PromotionArtifactStatus = "pending" | "applied" | "rejected";

export interface PromotionArtifact {
  version: 1;
  createdAt: string;
  projectAlias: string;
  taskId: string;
  baseBranch: string | null;
  decision: SchedulerResult["promotionDecision"];
  action: SchedulerResult["promotionAction"];
  effectivePromotionMode: ProjectTask["promotion"];
  validation: ValidationSummary[];
  piExitCode: number | null;
  outcome: TaskRunSummary["outcome"];
  status: PromotionArtifactStatus;
  processedAt: string | null;
  note: string | null;
}

export interface PromotionResultArtifact {
  version: 1;
  createdAt: string;
  projectAlias: string;
  taskId: string;
  sourcePromotionArtifactPath: string;
  sourcePromotionAction: SchedulerResult["promotionAction"];
  sourcePromotionDecision: SchedulerResult["promotionDecision"];
  result: "applied" | "rejected";
  branch: string | null;
  baseBranch: string | null;
  note: string | null;
  prUrl?: string | null;
}

export interface ProjectPolicy {
  version: 1;
  scope: {
    allowGlobs: string[];
    denyGlobs: string[];
    highRiskAreas: string[];
  };
  riskClasses: Record<
    "low-risk" | "medium-risk" | "high-risk",
    {
      autoMergeAllowed: boolean;
      requiresHumanReview: boolean;
    }
  >;
  selfHealing: {
    enabled: boolean;
    allowedTaskKinds: SupportedSelfHealingTaskKind[];
  };
  promotion: {
    lowRiskMode: ProjectTask["promotion"];
    mediumRiskMode: ProjectTask["promotion"];
    highRiskMode: ProjectTask["promotion"];
  };
}