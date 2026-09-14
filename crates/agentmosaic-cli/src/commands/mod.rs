//! Command dispatch. Every arm returns the payload `main` prints and the exit
//! code the invocation carries, except the internal bridge, which owns stdout
//! itself.

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

/// What `main` does with one dispatched command: the payload it prints, and the
/// exit code the invocation carries.
///
/// The exit code is part of dispatch rather than of rendering because one
/// command — `am doctor --json` — prints its object on stdout and still reports
/// that the answer is "no". Everything else keeps the historical 0 / 2 split of
/// `Ok` payload / `Err` message.
pub struct Dispatch {
    pub payload: Option<String>,
    pub exit: i32,
}

impl Dispatch {
    fn stdout(payload: impl Into<String>) -> Self {
        Self {
            payload: Some(payload.into()),
            exit: 0,
        }
    }

    fn silent() -> Self {
        Self {
            payload: None,
            exit: 0,
        }
    }

    /// A decision surface that printed its answer and refused to call it a
    /// success: the payload is on stdout, and the invocation exits non-zero.
    fn refused(payload: impl Into<String>) -> Self {
        Self {
            payload: Some(payload.into()),
            exit: 2,
        }
    }
}

/// Dispatch, discarding the exit code. This is the payload-only view the
/// parser tests drive; the binary itself uses [`dispatch_status`].
#[cfg(test)]
pub fn dispatch(command: Command) -> Result<Option<String>, String> {
    dispatch_status(command).map(|dispatch| dispatch.payload)
}

pub fn dispatch_status(command: Command) -> Result<Dispatch, String> {
    let output = match command {
        Command::Init { path } => Dispatch::stdout(init::run(path.as_deref())?),
        Command::Agent { command } => match command {
            AgentCommand::Add {
                id,
                role,
                adapter,
                name,
                concurrency,
                tags,
                artifacts,
                max_events,
                launch,
            } => Dispatch::stdout(agent::add(agent::AgentAdd {
                id,
                role,
                adapter,
                name,
                concurrency,
                tags,
                artifacts,
                max_events,
                launch,
            })?),
            AgentCommand::List { json } => Dispatch::stdout(agent::list(json)?),
            AgentCommand::Remove { id } => Dispatch::stdout(agent::remove(&id)?),
        },
        Command::Doctor { verbose, json } => {
            let (text, ready) = doctor::run(verbose, json)?;
            match (json, ready) {
                (_, true) => Dispatch::stdout(text),
                (true, false) => Dispatch::refused(text),
                (false, false) => return Err(text),
            }
        }
        Command::Run {
            quiet,
            json,
            objective,
        } => Dispatch::stdout(run::run(&objective, quiet, json)?),
        Command::Status { target, all, json } => {
            Dispatch::stdout(inspect::status(target, all, json)?)
        }
        Command::Final { target, root, json } => {
            Dispatch::stdout(inspect::final_result(target, root, json)?)
        }
        Command::Artifact { target, task, json } => {
            Dispatch::stdout(inspect::artifact(target, task, json)?)
        }
        Command::Tui { database } => Dispatch::stdout(run::tui(database)?),
        Command::Advanced => Dispatch::stdout(advanced::text()),

        // The compatibility commands keep their established field grammar.
        Command::Register { database, fields } => {
            Dispatch::stdout(advanced::register(&database, &fields)?)
        }
        Command::Registry { database, limit } => {
            Dispatch::stdout(advanced::registry(&database, limit.as_deref())?)
        }
        Command::RunAcp { database, fields } => {
            Dispatch::stdout(advanced::run_acp(&database, &fields)?)
        }
        Command::ContinueAcp { database, fields } => {
            Dispatch::stdout(advanced::continue_acp(&database, &fields)?)
        }
        Command::RunTeam { database, fields } => {
            Dispatch::stdout(advanced::run_team(&database, &fields)?)
        }
        Command::ResumeTeam { database, fields } => {
            Dispatch::stdout(advanced::resume_team(&database, &fields)?)
        }
        Command::Submit { database, fields } => {
            Dispatch::stdout(advanced::submit(&database, &fields)?)
        }
        Command::Cancel { database, fields } => {
            Dispatch::stdout(advanced::cancel(&database, &fields)?)
        }
        Command::Override { database, fields } => {
            Dispatch::stdout(advanced::override_task(&database, &fields)?)
        }
        Command::Recover { database, fields } => {
            Dispatch::stdout(advanced::recover(&database, &fields)?)
        }
        Command::RecoverAll { database } => Dispatch::stdout(advanced::recover_all(&database)?),
        Command::Resume { database, fields } => {
            Dispatch::stdout(advanced::resume(&database, &fields)?)
        }
        Command::Binding { database, fields } => {
            Dispatch::stdout(advanced::binding(&database, &fields)?)
        }

        // The Codex MCP bridge is the product's own driver entrypoint; it writes
        // its protocol to stdout, so `main` must print nothing for it.
        Command::Internal { command } => match command {
            InternalCommand::CodexMcp => {
                run_codex_mcp_bridge();
                return Ok(Dispatch::silent());
            }
        },
    };
    Ok(output)
}
