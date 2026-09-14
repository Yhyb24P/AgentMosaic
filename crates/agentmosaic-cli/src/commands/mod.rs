//! Command dispatch. Every arm returns the payload `main` prints, except the
//! internal bridge, which owns stdout itself (`Ok(None)`).

pub mod advanced;
pub mod agent;
pub mod doctor;
pub mod init;
pub mod inspect;
pub mod run;

use crate::args::{AgentCommand, Command, InternalCommand};
use agentmosaic_runtime::run_codex_mcp_bridge;

/// Shared field-level helper: a task id is a decimal board id.
pub fn parse_task(value: &str) -> Result<u64, String> {
    value.parse().map_err(|_| "invalid task id".to_string())
}

/// A current-thread runtime for the blocking CLI entrypoints that need timers.
pub fn team_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|error| format!("team run runtime: {error}"))
}

pub fn dispatch(command: Command) -> Result<Option<String>, String> {
    let output = match command {
        Command::Init { path } => init::run(path.as_deref())?,
        Command::Agent { command } => match command {
            AgentCommand::Add {
                id,
                role,
                adapter,
                name,
                concurrency,
                tags,
                artifacts,
                launch,
            } => agent::add(agent::AgentAdd {
                id,
                role,
                adapter,
                name,
                concurrency,
                tags,
                artifacts,
                launch,
            })?,
            AgentCommand::List => agent::list()?,
        },
        Command::Doctor => doctor::run()?,
        Command::Run { objective } => run::run(&objective)?,
        Command::Status { target, all } => inspect::status(target, all)?,
        Command::Final { target, root } => inspect::final_result(target, root)?,
        Command::Artifact { target, task } => inspect::artifact(target, task)?,
        Command::Tui { database } => run::tui(database)?,
        Command::Advanced => advanced::text().to_string(),

        // The compatibility commands keep their established field grammar.
        Command::Register { database, fields } => advanced::register(&database, &fields)?,
        Command::Registry { database, limit } => advanced::registry(&database, limit.as_deref())?,
        Command::RunAcp { database, fields } => advanced::run_acp(&database, &fields)?,
        Command::ContinueAcp { database, fields } => advanced::continue_acp(&database, &fields)?,
        Command::RunTeam { database, fields } => advanced::run_team(&database, &fields)?,
        Command::ResumeTeam { database, fields } => advanced::resume_team(&database, &fields)?,
        Command::Submit { database, fields } => advanced::submit(&database, &fields)?,
        Command::Cancel { database, fields } => advanced::cancel(&database, &fields)?,
        Command::Override { database, fields } => advanced::override_task(&database, &fields)?,
        Command::Recover { database, fields } => advanced::recover(&database, &fields)?,
        Command::RecoverAll { database } => advanced::recover_all(&database)?,
        Command::Resume { database, fields } => advanced::resume(&database, &fields)?,
        Command::Binding { database, fields } => advanced::binding(&database, &fields)?,

        // The Codex MCP bridge is the product's own driver entrypoint; it writes
        // its protocol to stdout, so `main` must print nothing for it.
        Command::Internal { command } => match command {
            InternalCommand::CodexMcp => {
                run_codex_mcp_bridge();
                return Ok(None);
            }
        },
    };
    Ok(Some(output))
}
