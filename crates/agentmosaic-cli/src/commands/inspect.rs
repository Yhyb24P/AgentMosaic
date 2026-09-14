//! Read-only inspection of the durable board: `status`, `final`, `artifact`.

use agentmosaic_team::{TaskBoard, TaskStatus};

use crate::{output, project};

pub fn status(database: &str) -> Result<String, String> {
    output::render_status(&project::open(database)?)
}

/// The durable final result of a root task. Never replayed: the persisted
/// successful attempt is read straight from the board.
pub fn final_result(database: &str, root: &str) -> Result<String, String> {
    let task = super::parse_task(root)?;
    let board = project::open(database)?;
    board
        .attempts(task)
        .map_err(|e| format!("final: {e:?}"))?
        .into_iter()
        .rev()
        .find(|attempt| attempt.status == TaskStatus::Succeeded)
        .and_then(|attempt| attempt.result)
        .ok_or_else(|| "no successful result for task".to_string())
}

pub fn artifact(database: &str, task: &str) -> Result<String, String> {
    let task = super::parse_task(task)?;
    let board = project::open(database)?;
    board
        .artifacts(task)
        .map_err(|e| format!("artifact: {e:?}"))
        .map(|items| {
            items
                .into_iter()
                .map(|item| format!("task={task} path={} sha256={}", item.path, item.sha256))
                .collect::<Vec<_>>()
                .join("\n")
        })
}
