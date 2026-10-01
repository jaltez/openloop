use clap::{Args, Subcommand};
mod dispatch;
mod dispatch2;
mod promotion_refresh;
pub use dispatch::dispatch;



#[derive(Debug, clap::Parser)]
#[command(name = "openloop", version, about = "Global CLI and service for unattended Pi-driven development across linked repositories.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

use crate::core as c;

pub fn stdout_is_tty() -> bool {
    c::table::stdout_is_tty()
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage linked projects
    Project(ProjectCommand),
    /// Inspect or resolve promotion artifacts
    Promotion(PromotionCommand),
    /// Manage global and project configuration
    Config(ConfigCommand),
    /// Manage the global daemon
    Service(ServiceCommand),
    /// View daemon or project run logs
    Logs(LogsArgs),
    /// Run Pi against a linked project
    Run(RunArgs),
    /// Run one scheduler iteration for a linked project
    RunOnce(RunOnceArgs),
    /// [Deprecated] Create a proposed task from a project reference
    Enqueue(EnqueueArgs),
    /// Manage project tasks
    Task(TaskCommand),
    /// Inspect human-approval packets for pending promotions
    Approval(ApprovalCommand),
    /// Show multi-project dashboard
    Status(StatusArgs),
    /// Query the append-only event audit log
    Events(EventsArgs),
    /// Live terminal dashboard (refreshes every second)
    Watch,
    /// Check system prerequisites and configuration health
    Doctor(DoctorArgs),
    /// Generate a human-readable summary of recent daemon activity
    Report(ReportArgs),
    /// Fleet confidence digest: activity, spend split, and pending review queue
    Digest(DigestArgs),
    /// Interactive first-run setup wizard
    Setup,
    /// GitHub/GitLab issue sync
    Issue(IssueCommand),
    /// Web dashboard for monitoring
    Dashboard(DashboardCommand),
    /// Run the openloop MCP server on stdio (JSON-RPC 2.0, newline-delimited)
    Mcp,
    #[command(hide = true)]
    /// Internal daemon control commands
    Daemon(DaemonCommand),
}

#[derive(Debug, Args)]
pub struct ProjectCommand {
    #[command(subcommand)]
    pub command: ProjectSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ProjectSubcommand {
    /// Register a linked project
    Add {
        alias: String,
        project_path: String,
        /// Initialize templates immediately
        #[arg(long)]
        init: bool,
        /// Update path if alias already exists
        #[arg(long)]
        force: bool,
    },
    /// List linked projects
    List {
        #[arg(long)]
        format: Option<String>,
    },
    /// Set the active project preference for the daemon
    Activate { alias: String },
    /// Show a project
    Show { alias: String },
    /// Initialize a linked project with control-plane templates
    Init {
        alias: String,
        /// Overwrite existing template files (backs up .openloop/ first)
        #[arg(long)]
        force: bool,
    },


    /// Unlink a project from the registry
    Remove { alias: String },
}

#[derive(Debug, Args)]
pub struct PromotionCommand {
    #[command(subcommand)]
    pub command: PromotionSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum PromotionSubcommand {
    /// List promotion artifacts for a linked project
    List {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        format: Option<String>,
    },
    /// Show promotion artifact details for a task
    Show {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: String,
    },
    /// Show promotion artifact history for a task
    History {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: String,
    },
    /// Apply a pending promotion locally when supported
    Apply {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        note: Option<String>,
        /// Preview what would happen without executing
        #[arg(long)]
        dry_run: bool,
    },
    /// Mark a pending promotion as rejected
    Reject {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        note: Option<String>,
    },
    /// Refresh promotion status from the PR (requires gh)
    Refresh {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: String,
    },
}

#[derive(Debug, Args)]
pub struct ConfigCommand {
    #[command(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Show global config
    Show,
    /// Set a global config value by dot-notation key
    Set { key: String, value: String },
    /// Set the global default model
    SetModel { model: String },
    /// Show project-local config
    ProjectShow { project: String },
    /// Set the model for a linked project
    ProjectSetModel { project: String, model: String },
    /// Set the global default agent provider
    SetProvider { provider: String },
    /// List all available agent providers
    ListProviders,
    /// Set the agent provider for a linked project
    ProjectSetAgent {
        project: String,
        provider: String,
        /// Custom command (required when provider is 'custom')
        #[arg(long)]
        command: Option<String>,
    },
    /// Add a global lifecycle hook
    AddHook {
        #[arg(long)]
        hook_type: String, // command | webhook
        #[arg(long, default_value = "*")]
        events: String,
        #[arg(long)]
        command: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value = "10")]
        timeout_seconds: u64,
    },
    /// Remove a global lifecycle hook by index
    RemoveHook { index: usize },
    /// List configured global lifecycle hooks
    ListHooks,
    /// Add a project-local lifecycle hook
    ProjectAddHook {
        project: String,
        #[arg(long)]
        hook_type: String,
        #[arg(long, default_value = "*")]
        events: String,
        #[arg(long)]
        command: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value = "10")]
        timeout_seconds: u64,
    },
    /// Remove a project-local lifecycle hook by index
    ProjectRemoveHook { project: String, index: usize },
    /// List configured project-local lifecycle hooks
    ProjectListHooks { project: String },
    /// Add a notification channel (webhook or desktop)
    AddChannel {
        #[arg(long)]
        channel_type: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value = "*")]
        events: String,
    },
    /// Remove a notification channel by index (0-based)
    RemoveChannel { index: usize },
    /// List configured notification channels
    ListChannels,
}

#[derive(Debug, Args)]
pub struct ServiceCommand {
    #[command(subcommand)]
    pub command: ServiceSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ServiceSubcommand {
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Show daemon status
    Status,
    /// Restart the daemon
    Restart,
    /// Pause new daemon runs
    Pause,
    /// Resume daemon scheduling
    Resume,
    /// Run the daemon in the foreground (for debugging or process managers)
    Run,
}

#[derive(Debug, Args)]
pub struct LogsArgs {
    /// Show run summaries for a specific project
    #[arg(short = 'p', long)]
    pub project: Option<String>,
    /// Number of lines to show
    #[arg(long, default_value = "50")]
    pub lines: usize,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(short = 'p', long)]
    pub project: String,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub prompt: String,
    /// Print prompt to stderr before execution
    #[arg(long, default_value = "false")]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct RunOnceArgs {
    #[arg(short = 'p', long)]
    pub project: String,
    /// Override the Pi model for this run
    #[arg(long)]
    pub model: Option<String>,
    /// Print the Pi prompt to stderr before execution
    #[arg(long, default_value = "false")]
    pub verbose: bool,
    /// Select task and build prompt but do not invoke Pi
    #[arg(long, default_value = "false")]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct EnqueueArgs {
    #[arg(short = 'p', long)]
    pub project: String,
    pub ref_: String,
}

#[derive(Debug, Args)]
pub struct TaskCommand {
    #[command(subcommand)]
    pub command: TaskSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum TaskSubcommand {
    /// Add a task to a linked project
    Add {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        title: Option<String>,
        /// Create task from an issue/PR reference (sets title and source)
        #[arg(long = "from-ref")]
        from_ref: Option<String>,
        #[arg(long, default_value = "feature")]
        kind: String,
        #[arg(long, default_value = "medium-risk")]
        risk: String,
        /// Relative scope paths for policy enforcement
        #[arg(long)]
        scope: Vec<String>,
        /// Task ids (or alias:taskId) this task depends on
        #[arg(long = "depends-on")]
        depends_on: Vec<String>,
    },
    /// List tasks from a linked project
    List {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        risk: Option<String>,
        #[arg(long)]
        format: Option<String>,
    },
    /// Show a task from a linked project
    Show {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: String,
    },
    /// Update a task in a linked project
    Update {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        risk: Option<String>,
        #[arg(long)]
        kind: Option<String>,
    },
    /// Remove a task from a linked project
    Remove {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: String,
    },
    /// Reset in_progress tasks back to ready for a linked project
    Recover {
        #[arg(short = 'p', long)]
        project: String,
    },
    /// Approve an awaiting-approval task so it can be implemented
    Approve {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: String,
    },
}

#[derive(Debug, Args)]
pub struct ApprovalCommand {
    #[command(subcommand)]
    pub command: ApprovalSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ApprovalSubcommand {
    /// List approval packets for a linked project
    List {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        format: Option<String>,
    },
    /// Show the approval packet for a task
    Show {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        task: String,
    },
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(long)]
    pub format: Option<String>,
}

#[derive(Debug, Args)]
pub struct EventsArgs {
    /// Filter by project alias
    #[arg(short = 'p', long)]
    pub project: Option<String>,
    /// Time window: e.g. 30m, 2h, 1d
    #[arg(long, default_value = "1h")]
    pub since: String,
    /// Max events to show
    #[arg(long, default_value = "50")]
    pub limit: usize,
    /// Output as JSON array
    #[arg(long, default_value = "false")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(long)]
    pub format: Option<String>,
}

#[derive(Debug, Args)]
pub struct ReportArgs {
    /// Report period in hours (default: 24)
    #[arg(long, default_value = "24")]
    pub hours: f64,
    #[arg(long)]
    pub format: Option<String>,
}

#[derive(Debug, Args)]
pub struct DigestArgs {
    /// Digest window in hours (default: 24)
    #[arg(long = "since-hours", default_value = "24")]
    pub since_hours: f64,
    #[arg(long)]
    pub format: Option<String>,
}

#[derive(Debug, Args)]
pub struct IssueCommand {
    #[command(subcommand)]
    pub command: IssueSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum IssueSubcommand {
    /// Configure issue source for a project
    SetSource {
        #[arg(short = 'p', long)]
        project: String,
        /// GitHub repo (owner/name)
        #[arg(long, conflicts_with = "gitlab")]
        github: Option<String>,
        /// GitLab project path (group/name)
        #[arg(long, conflicts_with = "github")]
        gitlab: Option<String>,
        #[arg(long, default_value = "openloop")]
        label: String,
        /// API token (GitHub PAT or GitLab token)
        #[arg(long)]
        token: Option<String>,
        /// Auto-sync during daemon ticks
        #[arg(long, default_value = "false")]
        auto_sync: bool,
        /// Post status updates as issue comments
        #[arg(long, default_value = "true")]
        post_comments: bool,
    },
    /// Sync issues from the configured source into tasks
    Sync {
        #[arg(short = 'p', long)]
        project: String,
        /// Override API token for this sync
        #[arg(long)]
        token: Option<String>,
    },
    /// List synced issues for a project
    List {
        #[arg(short = 'p', long)]
        project: String,
        #[arg(long)]
        format: Option<String>,
    },
    /// Remove the issue source configuration from a project
    RemoveSource {
        #[arg(short = 'p', long)]
        project: String,
    },
}

#[derive(Debug, Args)]
pub struct DashboardCommand {
    #[command(subcommand)]
    pub command: DashboardSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum DashboardSubcommand {
    /// Start the dashboard web server
    Start {
        /// Port to listen on (default: from config or 7399)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Enable dashboard auto-start with daemon
    Enable {
        #[arg(long)]
        port: Option<u16>,
    },
    /// Disable dashboard auto-start with daemon
    Disable,
    /// Show dashboard configuration
    Status,
}

#[derive(Debug, Args)]
pub struct DaemonCommand {
    #[command(subcommand)]
    pub command: DaemonSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum DaemonSubcommand {
    /// Internal: run the daemon worker loop
    Worker,
}
