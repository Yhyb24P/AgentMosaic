//! The product run path: one objective for the team, rendered as a live run.

use std::path::PathBuf;
use std::sync::Arc;

use agentmosaic_runtime::{LaunchSpec, TeamRunOptions, TeamRunner};
use agentmosaic_team::RunEventSink;

use crate::project;
use crate::render::{self, HumanRunEventSink, RunMode};
use crate::target::{self, TuiTarget};

/// `am run "<objective...>"`: discover the project, then hand the objective to
/// the durable team run while reporting the team's lifecycle on stderr.
///
/// This is the product's own run rendering: the compatibility `run-team`
/// spelling keeps its documented, scriptable stdout, and the two never share a
/// payload. Here stdout carries the final answer and nothing else.
pub fn run(objective: &[String], quiet: bool, json: bool) -> Result<String, String> {
    let invocation = Invocation::parse(objective, quiet, json)?;
    let (root, database) = project::project_database()?;
    let sink = Arc::new(HumanRunEventSink::stderr(invocation.mode));
    // The root does not exist yet, so the run id is not known here: the first
    // line is about the objective, and the sink's `RunStarted` line supplies
    // the run. A real runtime takes a visible moment to start, and a command
    // that prints nothing while it starts reads as a hung one.
    sink.starting(&invocation.objective);

    let host = LaunchSpec::new(
        std::env::current_exe().map_err(|e| format!("locate am executable: {e}"))?,
        Vec::new(),
    )?;
    let runner = TeamRunner::new(&database, &root, TeamRunOptions::default())
        .with_bridge_host(host)
        .with_sink(sink.clone() as Arc<dyn RunEventSink>);
    let runtime = super::team_runtime()?;
    let outcome = match runtime.block_on(runner.run(&invocation.objective)) {
        Ok(outcome) => outcome,
        Err(error) => {
            // A failure before the root exists leaves no state to preserve, so
            // the run id is what separates the two failure renderings.
            return Err(render::failure_payload(
                invocation.mode,
                sink.run_id(),
                &error.to_string(),
            ));
        }
    };
    sink.finish(outcome.root_task_id);
    Ok(invocation.mode.payload(outcome.result.answer))
}

/// `am tui [<DATABASE>]`: the live read-only board of this project, or of an
/// explicitly named database for the legacy spelling.
pub fn tui(database: Option<String>) -> Result<String, String> {
    let tokens: Vec<String> = database.into_iter().collect();
    let path = match target::tui_target(&tokens)? {
        TuiTarget::Project => project::ProjectContext::discover()?.database,
        TuiTarget::LegacyDatabase(path) => PathBuf::from(path),
    };
    agentmosaic_tui::run(path.to_str().ok_or("state database path is not UTF-8")?)?;
    Ok(String::new())
}

/// One `am run` invocation: the objective, and how it is presented.
///
/// The objective is free text, and clap's trailing positional keeps every token
/// after the first one, so the presentation flags are honored wherever they
/// appear rather than silently becoming part of the objective.
struct Invocation {
    objective: String,
    mode: RunMode,
}

impl Invocation {
    fn parse(objective: &[String], quiet: bool, json: bool) -> Result<Self, String> {
        let mut quiet = quiet;
        let mut json = json;
        let mut words = Vec::new();
        for token in objective {
            match token.as_str() {
                "--quiet" => quiet = true,
                "--json" => json = true,
                word => words.push(word.to_string()),
            }
        }
        let objective = words.join(" ");
        if objective.trim().is_empty() {
            return Err("am run requires an objective".into());
        }
        let mode = match (json, quiet) {
            (true, _) => RunMode::Machine,
            (false, true) => RunMode::Quiet,
            (false, false) => RunMode::Human,
        };
        Ok(Self { objective, mode })
    }
}
