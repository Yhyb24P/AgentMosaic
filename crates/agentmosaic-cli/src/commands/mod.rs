//! Command dispatch. Every arm returns the payload `main` prints and the exit
//! code the invocation carries.

pub mod agent;
pub mod doctor;
pub mod events;
pub mod init;
pub mod inspect;
pub mod run;

use crate::args::{AgentCommand, Command};

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
                launch,
            } => Dispatch::stdout(agent::add(agent::AgentAdd {
                id,
                role,
                adapter,
                name,
                concurrency,
                tags,
                artifacts,
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
        Command::Import { source } => {
            let database = crate::project::import_destination()?;
            agentmosaic_storage::import_database(&source, &database)
                .map_err(|error| format!("import: {error}"))?;
            if let Some(root) = database.parent().and_then(std::path::Path::parent) {
                init::update_gitignore(root)?;
            }
            Dispatch::stdout(format!("imported project database {}", database.display()))
        }
        Command::Run {
            quiet,
            json,
            resume,
            recover,
            objective,
        } => Dispatch::stdout(run::run(&objective, quiet, json, resume, recover)?),
        Command::Status { target, all, json } => {
            Dispatch::stdout(inspect::status(target, all, json)?)
        }
        Command::Events {
            target,
            json,
            follow,
        } => {
            debug_assert!(!follow, "main owns streaming event output");
            Dispatch::stdout(events::list(target, json)?)
        }
        Command::Final { target, json } => Dispatch::stdout(inspect::final_result(target, json)?),
        Command::Artifact { target, json } => Dispatch::stdout(inspect::artifact(target, json)?),
        Command::Tui { database } => Dispatch::stdout(run::tui(database)?),
    };
    Ok(output)
}
