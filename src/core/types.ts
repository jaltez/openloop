export interface GlobalConfig {
  version: 1;
  model?: string | null;
  defaultProvider?: string | null;
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
    maxPendingReviewsPerProject?: number;
    projectSelectionStrategy?: "round-robin" | "priority" | "focus";
  };
  notifications?: {
    onTaskComplete: string | null;
    onTaskFailed: string | null;
    onBudgetBlocked: string | null;
    onAllTasksDone: string | null;
  };
  notificationChannels?: NotificationChannelConfig[];
  hooks?: LifecycleHookConfig[];
  dashboard?: {
    port: number;
    enabled: boolean;
  };
}

export interface LifecycleHookConfig {
  type: "command" | "webhook";
  events: string[];
  command?: string | null;
  url?: string | null;
  timeoutSeconds?: number | null;
  disabled?: boolean;
}

export interface IssueSourceConfig {
  provider: "github" | "gitlab";
  repo: string;
  label: string;
  token?: string | null;
  autoSync?: boolean;
  syncIntervalMinutes?: number;
  postStatusComments?: boolean;
  lastSyncedAt?: string | null;
}

export interface SyncedIssue {
  number: number;
  title: string;
  body: string;
  labels: string[];
  state: "open" | "closed";
  url: string;
  taskId: string | null;
  syncedAt: string;
}

export interface IssueSyncLedger {
  version: 1;
  source: IssueSourceConfig;
  issues: SyncedIssue[];
  updatedAt: string;
}

export interface WebhookChannelConfig {
  type: "webhook";
  url: string;
  events: string[];
}

export interface DesktopChannelConfig {
  type: "desktop";
  events: string[];
}

export type NotificationChannelConfig = WebhookChannelConfig | DesktopChannelConfig;

export interface AgentUsage {
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  costUsd?: number;
}

export interface AgentRunResult {
  exitCode: number;
  stdout: string;
  stderr: string;
  usage?: AgentUsage;
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
    type: "pi" | "claude" | "aider" | "codex" | "opencode" | "ka" | "omp" | "custom";
    command: string | null;
    extraArgs?: string[];
  };
  runtime: {
    useWorktree: boolean;
    branchPrefix: string;
    prCommand?: string | null;
    worktreeSetupCommand?: string | null;
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
  issueSource?: IssueSourceConfig | null;
  hooks?: LifecycleHookConfig[];
  schedule?: ProjectSchedule[];
  /** schedule id -> ISO minute of last fire (daemon-maintained). */
  scheduleState?: Record<string, string>;
  review?: {
    enabled: boolean;
  };
  verification?: {
    enabled: boolean;
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
  lastDigestDate?: string | null;
}

export type RunStoppedBy = "none" | "timeout" | "budget" | "pause" | "no-progress";

export type WorkerRole = "sdd-planner" | "implementer" | "repo-improver" | "ci-healer";

export interface ProjectSchedule {
  id: string;
  cron: string;
  title: string;
  prompt?: string;
}

export interface TaskSource {
  type: "human" | "issue" | "discovery" | "ci" | "spec" | "schedule";
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
  status: "proposed" | "planned" | "ready" | "awaiting-approval" | "in_progress" | "blocked" | "done" | "failed" | "cancelled" | "promoted";
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
  /** Task ids that must be promoted/done/cancelled first; `alias:taskId`
   *  cross-project refs are reserved (never satisfied in-project). */
  dependsOn?: string[];
  estimatedCostUsd?: number;
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

export type ReviewFindingSeverity = "block" | "warn" | "info";

export interface ReviewFinding {
  rule: string;
  severity: ReviewFindingSeverity;
  message: string;
  file?: string | null;
}

export interface ReviewResult {
  findings: ReviewFinding[];
  hasBlocking: boolean;
  /** True when the LLM reviewer wrote a file that cannot be parsed. */
  malformed?: boolean;
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
  reviewFindings?: ReviewFinding[];
  runSummaryPath?: string | null;
  costUsd: number | null;
  costSource: "measured" | "estimated" | null;
  approvalPacketPath?: string | null;
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
  approvalPacketPath?: string | null;
  status: PromotionArtifactStatus;
  processedAt: string | null;
  note: string | null;
}

export interface PromotionCheckRollupEntry {
  name: string | null;
  status: string | null;
  conclusion: string | null;
}

export interface PromotionResultArtifact {
  version: 1;
  createdAt: string;
  projectAlias: string;
  taskId: string;
  sourcePromotionArtifactPath: string;
  sourcePromotionAction: SchedulerResult["promotionAction"];
  sourcePromotionDecision: SchedulerResult["promotionDecision"];
  result: "applied" | "rejected" | "refreshed";
  branch: string | null;
  baseBranch: string | null;
  note: string | null;
  prUrl?: string | null;
  /** Present on `refreshed` results: live PR state from `gh pr view`. */
  state?: string | null;
  /** Present on `refreshed` results: live check rollup from `gh pr view`. */
  checks?: PromotionCheckRollupEntry[];
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