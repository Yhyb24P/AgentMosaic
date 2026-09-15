//! The `am` command model.
//!
//! clap is the single authoritative parser for the top-level surface. The 13
//! compatibility commands are declared here so they keep their current
//! top-level spellings, but they take a raw trailing vector and keep their
//! established field-level grammar (see `commands::advanced`).

use clap::{CommandFactory, Parser, Subcommand};

const ABOUT: &str = "AgentMosaic — run heterogeneous coding agents as one durable team.";
const AFTER_HELP: &str =
    "Documentation: https://am.yhshyp.xyz\nRepository:    https://github.com/Yhyb24P/AgentMosaic";

#[derive(Debug, Parser)]
#[command(
    name = "am",
    version,
    about = ABOUT,
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP,
    subcommand_required = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// The rendered top-level help. `am` with no arguments reports this on stderr.
pub fn help_text() -> String {
    Cli::command().render_help().to_string()
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize AgentMosaic in a project
    Init {
        /// Project directory (defaults to the current directory)
        #[arg(value_name = "PATH")]
        path: Option<String>,
    },
    /// Add, list, or remove Agents
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Check whether the team is ready
    Doctor {
        /// Also print the bounded per-Agent diagnostic stages
        #[arg(long)]
        verbose: bool,
        /// Print the decision as one JSON object instead of human text
        #[arg(long)]
        json: bool,
    },
    /// Give one objective to the team
    Run {
        /// Do not print routine progress or the next-step footer
        #[arg(long)]
        quiet: bool,
        /// Print one JSON object on stdout, with no human progress on any stream
        #[arg(long)]
        json: bool,
        /// The objective, as one or more words
        #[arg(value_name = "OBJECTIVE", trailing_var_arg = true)]
        objective: Vec<String>,
    },
    /// Show run status for this project
    Status {
        /// Run id, or a state database path for the legacy whole-board listing
        #[arg(value_name = "RUN_OR_DATABASE")]
        target: Option<String>,
        /// List every run of this project, newest first
        #[arg(long)]
        all: bool,
        /// Print the runs as one JSON object instead of human text
        #[arg(long)]
        json: bool,
    },
    /// Show normalized runtime events for a task or run
    Events {
        /// Task or run id (defaults to the latest run)
        #[arg(value_name = "TASK_OR_RUN")]
        target: Option<String>,
        /// Print a stable machine-readable event projection
        #[arg(long)]
        json: bool,
        /// Keep polling for new observations; Ctrl-C stops only this reader
        #[arg(long)]
        follow: bool,
    },
    /// Show a durable final result
    Final {
        /// Run id, or a state database path for the legacy `<database> <root>`
        #[arg(value_name = "RUN_OR_DATABASE")]
        target: Option<String>,
        /// Root task, for the legacy `<database> <root>` form
        #[arg(value_name = "ROOT")]
        root: Option<String>,
        /// Print the result as one JSON object instead of human text
        #[arg(long)]
        json: bool,
    },
    /// Show recorded artifacts
    Artifact {
        /// Task id, or a state database path for the legacy `<database> <task>`
        #[arg(value_name = "TASK_OR_DATABASE")]
        target: Option<String>,
        /// Task, for the legacy `<database> <task>` form
        #[arg(value_name = "TASK")]
        task: Option<String>,
        /// Print the artifacts as one JSON object instead of human text
        #[arg(long)]
        json: bool,
    },
    /// Open the live read-only team board
    Tui {
        /// State database path (defaults to this project's database)
        #[arg(value_name = "DATABASE")]
        database: Option<String>,
    },
    /// Show compatibility/low-level commands
    Advanced,

    // Compatibility/diagnostic surface. Each of these keeps its exact current
    // top-level spelling and its own field-level grammar: clap consumes the
    // database positional, and the remaining fields are handed to the existing
    // parser for that command. They are hidden from every help listing.
    /// Register an Agent directly
    #[command(hide = true)]
    Register {
        /// Project state database
        database: String,
        /// Agent fields, verbatim
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },
    /// List registered Agents
    #[command(hide = true)]
    Registry {
        /// Project state database
        database: String,
        /// Maximum number of Agents to print
        #[arg(value_name = "LIMIT")]
        limit: Option<String>,
    },
    /// Run one task through a registered ACP Agent
    #[command(hide = true)]
    RunAcp {
        /// Project state database
        database: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },
    /// Continue a task through a foreign ACP session
    #[command(hide = true)]
    ContinueAcp {
        /// Project state database
        database: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },
    /// Run one objective through the team
    #[command(hide = true)]
    RunTeam {
        /// Project state database
        database: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },
    /// Resume an existing team run
    #[command(hide = true)]
    ResumeTeam {
        /// Project state database
        database: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },
    /// Submit a task to the board
    #[command(hide = true)]
    Submit {
        /// Project state database
        database: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },
    /// Cancel a task
    #[command(hide = true)]
    Cancel {
        /// Project state database
        database: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },
    /// Assign a task to an Agent
    #[command(hide = true)]
    Override {
        /// Project state database
        database: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },
    /// Recover one interrupted attempt
    #[command(hide = true)]
    Recover {
        /// Project state database
        database: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },
    /// Recover every interrupted attempt
    #[command(hide = true)]
    RecoverAll {
        /// Project state database
        database: String,
    },
    /// Resend a failed or cancelled task
    #[command(hide = true)]
    Resume {
        /// Project state database
        database: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },
    /// Show the recorded external runtime binding
    #[command(hide = true)]
    Binding {
        /// Project state database
        database: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        fields: Vec<String>,
    },

    /// Internal product bridge; not part of the public surface
    #[command(name = "__internal", hide = true)]
    Internal {
        #[command(subcommand)]
        command: InternalCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Add an Agent to this project
    Add {
        /// Agent id
        id: String,
        /// Agent role: reasoner, worker, or utility
        #[arg(long, value_name = "ROLE")]
        role: String,
        /// Adapter kind: acp or codex-app-server
        #[arg(long, value_name = "KIND")]
        adapter: String,
        /// Display name (defaults to the id)
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// Maximum concurrent tasks
        #[arg(long, value_name = "N", default_value_t = 1)]
        concurrency: i64,
        /// Tag the Agent (repeatable)
        #[arg(long = "tag", value_name = "TAG")]
        tags: Vec<String>,
        /// Artifact path relative to the workspace (repeatable)
        #[arg(long = "artifact", value_name = "RELPATH")]
        artifacts: Vec<String>,
        /// Maximum lifecycle events per Codex turn (advanced tuning; codex-app-server only)
        #[arg(long, value_name = "N")]
        max_events: Option<u64>,
        /// Launch command and its argv, after `--`
        #[arg(last = true, value_name = "PROGRAM")]
        launch: Vec<String>,
    },
    /// List the Agents of this project
    List {
        /// Print the registry as one JSON object instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Remove an Agent from this project
    Remove {
        /// Agent id
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum InternalCommand {
    #[command(name = "codex-mcp")]
    CodexMcp,
}
