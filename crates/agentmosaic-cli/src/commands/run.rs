//! The product run path: one objective for the team, rendered as a live run.

use std::path::PathBuf;
use std::sync::Arc;

use agentmosaic_runtime::{TeamRunOptions, TeamRunner};
use agentmosaic_team::{RunEventSink, TaskBoard, TaskKind, TaskStatus};

use crate::json::{ArtifactJson, RunJson};
use crate::project;
use crate::render::{self, HumanRunEventSink, RunMode};
use crate::target::{self, TuiTarget};

/// `am run "<objective...>"`: discover the project, then hand the objective to
/// the durable team run while reporting the team's lifecycle on stderr.
///
/// Stdout carries the final answer or the typed JSON run object. Recovery only
/// settles state; resuming goes through the same durable TeamRunner as a new run.
pub fn run(
    objective: &[String],
    quiet: bool,
    json: bool,
    resume: Option<u64>,
    recover: Option<u64>,
) -> Result<String, String> {
    let invocation = Invocation::parse(
        objective,
        quiet,
        json,
        resume.is_some() || recover.is_some(),
    )?;
    let (root, database) = project::project_database()?;
    let sink = Arc::new(HumanRunEventSink::stderr(invocation.mode));
    if let Some(id) = recover {
        let mut board = project::open(
            database
                .to_str()
                .ok_or("state database path is not UTF-8")?,
        )?;
        let task = board
            .task(id)
            .map_err(|error| format!("recover: {error:?}"))?
            .ok_or_else(|| format!("no run #{id} in this project"))?;
        if task.kind != TaskKind::Reasoning || task.parent_task.is_some() {
            return Err(format!(
                "task #{id} is not a run: recovery requires a root reasoning task"
            ));
        }
        let attempt = board
            .recover_interrupted_attempt(id)
            .map_err(|error| format!("recover: {error:?}"))?;
        return if invocation.mode == RunMode::Machine {
            serde_json::to_string(&serde_json::json!({
                "run_id": id,
                "recovered_attempt": attempt.as_ref().map(|attempt| attempt.attempt),
                "status": board.task(id).map_err(|error| format!("recover: {error:?}"))?
                    .ok_or("run disappeared during recovery")?.status.as_str(),
            }))
            .map_err(|error| error.to_string())
        } else {
            Ok(match attempt {
                Some(attempt) => format!("recovered run #{id} interrupted attempt {}; continue with `am run --resume {id}`", attempt.attempt),
                None => format!("run #{id} has no interrupted running attempt"),
            })
        };
    }
    if resume.is_none() {
        sink.starting(&invocation.objective);
    }
    let runner = TeamRunner::new(&database, &root, TeamRunOptions::default())
        .with_sink(sink.clone() as Arc<dyn RunEventSink>);
    let runtime = super::team_runtime()?;
    let outcome = match runtime.block_on(async {
        match resume {
            Some(id) => runner.resume(id).await,
            None => runner.run(&invocation.objective).await,
        }
    }) {
        Ok(outcome) => outcome,
        Err(error) => {
            // A failure before the root exists leaves no state to preserve, so
            // the run id is what separates the two failure renderings.
            return Err(render::failure_payload(
                invocation.mode,
                sink.run_id().or(resume),
                &error.to_string(),
            ));
        }
    };
    let run = RunJson {
        run_id: outcome.root_task_id,
        lead_agent: outcome.lead_agent.clone(),
        status: TaskStatus::Succeeded.as_str().to_string(),
        answer: outcome.result.answer.clone(),
        task_refs: outcome.result.task_refs.clone(),
        artifact_refs: outcome
            .result
            .artifact_refs
            .iter()
            .map(|selected| ArtifactJson {
                task_id: selected.task_id,
                path: selected.artifact.path.clone(),
                sha256: selected.artifact.sha256.clone(),
            })
            .collect(),
    };
    sink.finish(outcome.root_task_id);
    invocation.mode.payload(&run)
}

/// `am tui [<DATABASE>]`: the live read-only board of this project, or of an
/// explicitly named current-generation database.
pub fn tui(database: Option<String>) -> Result<String, String> {
    let tokens: Vec<String> = database.into_iter().collect();
    let path = match target::tui_target(&tokens)? {
        TuiTarget::Project => project::ProjectContext::discover()?.database,
        TuiTarget::Database(path) => PathBuf::from(path),
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
    fn parse(
        objective: &[String],
        quiet: bool,
        json: bool,
        existing_run: bool,
    ) -> Result<Self, String> {
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
        if objective.trim().is_empty() && !existing_run {
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
