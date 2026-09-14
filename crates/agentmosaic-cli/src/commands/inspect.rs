//! Read-only inspection of the durable board: `status`, `final`, `artifact`.
//!
//! Each command has two spellings. The project-aware spelling discovers the
//! project from the current directory and speaks in runs and tasks; the legacy
//! spelling names a state database and keeps its historical output exactly.
//! [`crate::target`] decides which spelling a bare positional asks for, and
//! never guesses. A user-visible run is a root reasoning task: a failed or
//! incomplete newest run is reported as it is and never skipped for an older
//! successful one.

use agentmosaic_storage::SqliteTaskBoard;
use agentmosaic_team::{TaskBoard, TaskKind, TaskRecord, TaskStatus};

use crate::project::ProjectContext;
use crate::target::{self, ArtifactTarget, FinalTarget, StatusTarget};
use crate::{output, project};

/// What the project-aware spellings print when the project has no run yet.
const NO_RUNS: &str = "no runs in this project yet; start one with `am run \"<objective>\"`";

pub fn status(first: Option<String>, all: bool) -> Result<String, String> {
    let tokens: Vec<String> = first.into_iter().collect();
    match target::status_target(&tokens, all)? {
        // The legacy whole-board listing keeps its exact historical shape.
        StatusTarget::LegacyBoard(database) => output::render_status(&project::open(&database)?),
        StatusTarget::LatestRun => {
            let board = open_project()?;
            let Some(run) = board.latest_root_task().map_err(|e| format!("{e:?}"))? else {
                return Ok(NO_RUNS.to_string());
            };
            output::render_run_status(&board, &run)
        }
        StatusTarget::AllRuns => {
            let board = open_project()?;
            let listing = output::render_run_list(&board)?;
            Ok(if listing.is_empty() {
                NO_RUNS.to_string()
            } else {
                listing
            })
        }
        StatusTarget::Run(id) => {
            let board = open_project()?;
            let run = require_run(&board, id)?;
            output::render_run_status(&board, &run)
        }
    }
}

/// The durable final result of one run. Never replayed and never substituted:
/// the persisted successful attempt is read straight from the board, and a run
/// without one is reported as it is.
pub fn final_result(first: Option<String>, second: Option<String>) -> Result<String, String> {
    let tokens: Vec<String> = first.into_iter().chain(second).collect();
    match target::final_target(&tokens)? {
        FinalTarget::Legacy { database, root } => legacy_final(&database, root),
        FinalTarget::LatestRun => {
            let board = open_project()?;
            let run = latest_run(&board)?;
            run_final(&board, &run)
        }
        FinalTarget::Run(id) => {
            let board = open_project()?;
            let run = require_run(&board, id)?;
            run_final(&board, &run)
        }
    }
}

pub fn artifact(first: Option<String>, second: Option<String>) -> Result<String, String> {
    let tokens: Vec<String> = first.into_iter().chain(second).collect();
    match target::artifact_target(&tokens)? {
        ArtifactTarget::Legacy { database, task } => legacy_artifact(&database, task),
        ArtifactTarget::LatestRun => {
            let board = open_project()?;
            let run = latest_run(&board)?;
            let mut lines = Vec::new();
            for id in subtree(&board, &run)? {
                lines.extend(artifact_lines(&board, id)?);
            }
            Ok(lines.join("\n"))
        }
        ArtifactTarget::Task(id) => {
            let board = open_project()?;
            if board.task(id).map_err(|e| format!("{e:?}"))?.is_none() {
                return Err(format!("no task #{id} in this project"));
            }
            Ok(artifact_lines(&board, id)?.join("\n"))
        }
    }
}

fn open_project() -> Result<SqliteTaskBoard, String> {
    ProjectContext::discover()?.open_board()
}

fn latest_run(board: &SqliteTaskBoard) -> Result<TaskRecord, String> {
    board
        .latest_root_task()
        .map_err(|e| format!("{e:?}"))?
        .ok_or_else(|| NO_RUNS.to_string())
}

/// The run identity rule, enforced rather than assumed: only a root reasoning
/// task is a run, and anything else is refused by name.
fn require_run(board: &SqliteTaskBoard, id: u64) -> Result<TaskRecord, String> {
    let task = board
        .task(id)
        .map_err(|e| format!("{e:?}"))?
        .ok_or_else(|| format!("no task #{id} in this project"))?;
    if task.parent_task.is_none() && task.kind == TaskKind::Reasoning {
        return Ok(task);
    }
    Err(format!(
        "task #{id} is not a run: a run is a root reasoning task, and #{id} is a {} task with parent {}",
        task.kind.as_str(),
        task.parent_task
            .map(|parent| format!("#{parent}"))
            .unwrap_or_else(|| "-".into())
    ))
}

/// The run's root plus its descendants, in id order.
fn subtree(board: &SqliteTaskBoard, run: &TaskRecord) -> Result<Vec<u64>, String> {
    let mut ids = vec![run.id];
    ids.extend(board.descendants_of(run.id).map_err(|e| format!("{e:?}"))?);
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

fn artifact_lines(board: &SqliteTaskBoard, task: u64) -> Result<Vec<String>, String> {
    Ok(board
        .artifacts(task)
        .map_err(|e| format!("artifact: {e:?}"))?
        .into_iter()
        .map(|item| format!("task={task} path={} sha256={}", item.path, item.sha256))
        .collect())
}

fn run_final(board: &SqliteTaskBoard, run: &TaskRecord) -> Result<String, String> {
    board
        .attempts(run.id)
        .map_err(|e| format!("final: {e:?}"))?
        .into_iter()
        .rev()
        .find(|attempt| attempt.status == TaskStatus::Succeeded)
        .and_then(|attempt| attempt.result)
        .ok_or_else(|| {
            format!(
                "run #{} is {} and has no final answer; `am status` shows its state",
                run.id,
                run.status.as_str()
            )
        })
}

fn legacy_final(database: &str, root: u64) -> Result<String, String> {
    let board = project::open(database)?;
    board
        .attempts(root)
        .map_err(|e| format!("final: {e:?}"))?
        .into_iter()
        .rev()
        .find(|attempt| attempt.status == TaskStatus::Succeeded)
        .and_then(|attempt| attempt.result)
        .ok_or_else(|| "no successful result for task".to_string())
}

fn legacy_artifact(database: &str, task: u64) -> Result<String, String> {
    Ok(artifact_lines(&project::open(database)?, task)?.join("\n"))
}
