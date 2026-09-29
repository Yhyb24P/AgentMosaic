//! The project command model. Parsing performs no database writes.

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
    /// Import a released or baseline database into this project
    Import {
        /// Source database, opened read-only
        source: String,
    },
    /// Give one objective to the team
    Run {
        /// Do not print routine progress or the next-step footer
        #[arg(long)]
        quiet: bool,
        /// Print one JSON object on stdout, with no human progress on any stream
        #[arg(long)]
        json: bool,
        /// Continue a durable run without replaying completed work
        #[arg(long, value_name = "RUN", conflicts_with_all = ["recover", "objective"])]
        resume: Option<u64>,
        /// Settle an interrupted running root after its owning process has stopped
        #[arg(long, value_name = "RUN", conflicts_with_all = ["resume", "objective"])]
        recover: Option<u64>,
        /// The objective, as one or more words
        #[arg(value_name = "OBJECTIVE", trailing_var_arg = true)]
        objective: Vec<String>,
    },
    /// Show run status for this project
    Status {
        /// Run id (defaults to the latest run)
        #[arg(value_name = "RUN")]
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
        /// Run id (defaults to the latest run)
        #[arg(value_name = "RUN")]
        target: Option<String>,
        /// Print the result as one JSON object instead of human text
        #[arg(long)]
        json: bool,
    },
    /// Show recorded artifacts
    Artifact {
        /// Task id (defaults to all artifacts of the latest run)
        #[arg(value_name = "TASK")]
        target: Option<String>,
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
        /// Adapter kind: acp, codex-exec, or claude-cli
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
