//! Read-only inspection of the durable board: `status`, `final`, `artifact`.
//!
//! Each command has two spellings. The project-aware spelling discovers the
//! project from the current directory and speaks in runs and tasks; the legacy
//! spelling names a state database and keeps its historical output exactly.
//! [`crate::target`] decides which spelling a bare positional asks for, and
//! never guesses. A user-visible run is a root reasoning task: a failed or
//! incomplete newest run is reported as it is and never skipped for an older
//! successful one.
//!
//! `--json` is additive: it replaces the human rendering of the project-aware
//! spellings with one typed object and leaves every other surface alone. The
//! legacy `<database>` spellings keep their historical text and have no JSON
//! form, so asking for one is refused by name rather than guessed at.

use agentmosaic_storage::SqliteTaskBoard;
use agentmosaic_team::{TaskBoard, TaskKind, TaskRecord, TaskStatus};

use crate::json::{
    self, ArtifactJson, ArtifactListJson, FinalJson, RunListJson, RunSummaryJson, StatusJson,
    TaskJson,
};
use crate::project::ProjectContext;
use crate::target::{self, ArtifactTarget, FinalTarget, StatusTarget};
use crate::{output, project};

/// What the project-aware spellings print when the project has no run yet.
const NO_RUNS: &str = "no runs in this project yet; start one with `am run \"<objective>\"`";

pub fn status(first: Option<String>, all: bool, machine: bool) -> Result<String, String> {
    let tokens: Vec<String> = first.into_iter().collect();
    match target::status_target(&tokens, all)? {
        // The legacy whole-board listing keeps its exact historical shape.
        StatusTarget::LegacyBoard(database) => {
            legacy_refuses_json("status", machine)?;
            output::render_status(&project::open(&database)?)
        }
        StatusTarget::LatestRun => {
            let board = open_project()?;
            let Some(run) = board.latest_root_task().map_err(|e| format!("{e:?}"))? else {
                // There is no run to describe; a machine consumer gets the same
                // refusal a human does, not an object with invented fields.
                return if machine {
                    Err(NO_RUNS.to_string())
                } else {
                    Ok(NO_RUNS.to_string())
                };
            };
            run_status(&board, &run, machine)
        }
        StatusTarget::AllRuns => {
            let board = open_project()?;
            if machine {
                return json::encode(&run_list_json(&board)?);
            }
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
            run_status(&board, &run, machine)
        }
    }
}

/// The durable final result of one run. Never replayed and never substituted:
/// the persisted successful attempt is read straight from the board, and a run
/// without one is reported as it is.
pub fn final_result(
    first: Option<String>,
    second: Option<String>,
    machine: bool,
) -> Result<String, String> {
    let tokens: Vec<String> = first.into_iter().chain(second).collect();
    match target::final_target(&tokens)? {
        FinalTarget::Legacy { database, root } => {
            legacy_refuses_json("final", machine)?;
            legacy_final(&database, root)
        }
        FinalTarget::LatestRun => {
            let board = open_project()?;
            let run = latest_run(&board)?;
            run_final(&board, &run, machine)
        }
        FinalTarget::Run(id) => {
            let board = open_project()?;
            let run = require_run(&board, id)?;
            run_final(&board, &run, machine)
        }
    }
}

pub fn artifact(
    first: Option<String>,
    second: Option<String>,
    machine: bool,
) -> Result<String, String> {
    let tokens: Vec<String> = first.into_iter().chain(second).collect();
    match target::artifact_target(&tokens)? {
        ArtifactTarget::Legacy { database, task } => {
            legacy_refuses_json("artifact", machine)?;
            legacy_artifact(&database, task)
        }
        ArtifactTarget::LatestRun => {
            let board = open_project()?;
            let run = latest_run(&board)?;
            let mut items = Vec::new();
            for id in subtree(&board, &run)? {
                items.extend(artifacts_of(&board, id)?);
            }
            artifact_payload(items, machine)
        }
        ArtifactTarget::Task(id) => {
            let board = open_project()?;
            if board.task(id).map_err(|e| format!("{e:?}"))?.is_none() {
                return Err(format!("no task #{id} in this project"));
            }
            artifact_payload(artifacts_of(&board, id)?, machine)
        }
    }
}

/// The legacy spellings name a database, not a project: they keep their exact
/// historical text, and no JSON shape was ever defined for them.
fn legacy_refuses_json(command: &str, machine: bool) -> Result<(), String> {
    if machine {
        return Err(format!(
            "`{command} --json` describes this project's runs, and the legacy \
             `<database>` form has no JSON output"
        ));
    }
    Ok(())
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

/// Every artifact of one task, with its whole path and whole digest.
fn artifacts_of(board: &SqliteTaskBoard, task: u64) -> Result<Vec<ArtifactJson>, String> {
    Ok(board
        .artifacts(task)
        .map_err(|e| format!("artifact: {e:?}"))?
        .into_iter()
        .map(|item| ArtifactJson {
            task_id: task,
            path: item.path,
            sha256: item.sha256,
        })
        .collect())
}

fn artifact_payload(items: Vec<ArtifactJson>, machine: bool) -> Result<String, String> {
    if machine {
        return json::encode(&ArtifactListJson { artifacts: items });
    }
    Ok(items
        .into_iter()
        .map(|item| {
            format!(
                "task={} path={} sha256={}",
                item.task_id, item.path, item.sha256
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// One run with its subtree and every artifact recorded below it.
fn run_status(board: &SqliteTaskBoard, run: &TaskRecord, machine: bool) -> Result<String, String> {
    if !machine {
        return output::render_run_status(board, run);
    }
    let mut tasks = Vec::new();
    let mut artifacts = Vec::new();
    for id in subtree(board, run)? {
        let task = board
            .task(id)
            .map_err(|e| format!("{e:?}"))?
            .ok_or_else(|| format!("missing task {id}"))?;
        tasks.push(TaskJson {
            id: task.id,
            assignee: task.assignee.clone(),
            status: task.status.as_str().to_string(),
            objective: task.objective.clone(),
        });
        artifacts.extend(artifacts_of(board, id)?);
    }
    json::encode(&StatusJson {
        run_id: run.id,
        status: run.status.as_str().to_string(),
        objective: run.objective.clone(),
        lead: run.assignee.clone(),
        tasks,
        artifacts,
    })
}

/// Every run of the project, newest first.
fn run_list_json(board: &SqliteTaskBoard) -> Result<RunListJson, String> {
    let mut runs = board.root_tasks().map_err(|e| format!("{e:?}"))?;
    runs.reverse();
    Ok(RunListJson {
        runs: runs
            .into_iter()
            .map(|run| RunSummaryJson {
                run_id: run.id,
                status: run.status.as_str().to_string(),
                objective: run.objective,
            })
            .collect(),
    })
}

fn run_final(board: &SqliteTaskBoard, run: &TaskRecord, machine: bool) -> Result<String, String> {
    let answer = board
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
        })?;
    if machine {
        return json::encode(&FinalJson {
            run_id: run.id,
            answer,
        });
    }
    Ok(answer)
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
    artifact_payload(artifacts_of(&project::open(database)?, task)?, false)
}
