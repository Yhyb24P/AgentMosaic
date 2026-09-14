//! Presentation helpers: the bounded, human-readable renderings the CLI
//! prints. No command logic lives here.

use agentmosaic_runtime::TeamRunOutcome;
use agentmosaic_storage::{SqliteAgentRegistry, SqliteTaskBoard};
use agentmosaic_team::{TaskBoard, TaskRecord};

/// The longest objective a run rendering prints; longer text is cut on a
/// character boundary and marked.
const MAX_OBJECTIVE_BYTES: usize = 72;

/// The bounded, human-readable summary of one team run.
pub fn render_team_outcome(outcome: &TeamRunOutcome) -> String {
    let task_refs = if outcome.result.task_refs.is_empty() {
        "-".to_string()
    } else {
        outcome
            .result
            .task_refs
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut lines = vec![
        format!("root={} lead={}", outcome.root_task_id, outcome.lead_agent),
        format!("answer: {}", outcome.result.answer),
        format!("task_refs: {task_refs}"),
    ];
    if outcome.result.artifact_refs.is_empty() {
        lines.push("artifact_refs: -".into());
    } else {
        for selected in &outcome.result.artifact_refs {
            lines.push(format!(
                "artifact_refs: task={} path={} sha256={}",
                selected.task_id, selected.artifact.path, selected.artifact.sha256
            ));
        }
    }
    lines.join("\n")
}

/// One line per task: the durable board as the operator sees it.
///
/// This is the legacy `status <database>` renderer, kept byte-for-byte: every
/// line is a `task=...` record and nothing else is printed.
pub fn render_status(board: &SqliteTaskBoard) -> Result<String, String> {
    board
        .task_ids()
        .map_err(|e| format!("status: {e:?}"))?
        .into_iter()
        .map(|id| {
            let task = board
                .task(id)
                .map_err(|e| format!("status: {e:?}"))?
                .ok_or_else(|| format!("status: missing task {id}"))?;
            let attempts = board
                .attempts(task.id)
                .map_err(|e| format!("status: {e:?}"))?;
            Ok(format!(
                "task={} status={} assignee={} attempts={} parent={} objective={}",
                task.id,
                task.status.as_str(),
                task.assignee.as_deref().unwrap_or("-"),
                attempts.len(),
                task.parent_task
                    .map(|parent| parent.to_string())
                    .unwrap_or_else(|| "-".into()),
                task.objective
            ))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|lines| lines.join("\n"))
}

/// One run, as the operator sees it: the root task, the tasks below it, and
/// the artifacts recorded anywhere in that subtree.
///
/// Only board data is printed, never a storage path.
pub fn render_run_status(board: &SqliteTaskBoard, run: &TaskRecord) -> Result<String, String> {
    let ids = subtree_ids(board, run)?;
    let mut lines = vec![
        format!("run #{}  {}", run.id, run.status.as_str()),
        format!("objective  {}", bounded_objective(&run.objective)),
        format!("lead       {}", run.assignee.as_deref().unwrap_or("-")),
        String::new(),
        "tasks".to_string(),
    ];
    for id in &ids {
        let task = task_at(board, *id)?;
        lines.push(format!(
            "  #{}  {}  {}  {}",
            task.id,
            task.assignee.as_deref().unwrap_or("-"),
            task.status.as_str(),
            bounded_objective(&task.objective)
        ));
    }
    lines.push(String::new());
    lines.push("artifacts".to_string());
    for id in &ids {
        for artifact in board.artifacts(*id).map_err(|e| format!("{e:?}"))? {
            lines.push(format!("  #{}  {}", id, artifact.path));
        }
    }
    Ok(lines.join("\n"))
}

/// One concise line per run, newest first.
pub fn render_run_list(board: &SqliteTaskBoard) -> Result<String, String> {
    let mut runs = board.root_tasks().map_err(|e| format!("{e:?}"))?;
    runs.reverse();
    Ok(runs
        .iter()
        .map(|run| {
            format!(
                "run #{}  {}  {}",
                run.id,
                run.status.as_str(),
                bounded_objective(&run.objective)
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// The root task and every task below it, in id order.
fn subtree_ids(board: &SqliteTaskBoard, run: &TaskRecord) -> Result<Vec<u64>, String> {
    let mut ids = vec![run.id];
    ids.extend(board.descendants_of(run.id).map_err(|e| format!("{e:?}"))?);
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

fn task_at(board: &SqliteTaskBoard, id: u64) -> Result<TaskRecord, String> {
    board
        .task(id)
        .map_err(|e| format!("{e:?}"))?
        .ok_or_else(|| format!("missing task {id}"))
}

/// A single-line, bounded objective.
fn bounded_objective(objective: &str) -> String {
    let one_line = objective.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.len() <= MAX_OBJECTIVE_BYTES {
        return one_line;
    }
    let mut end = MAX_OBJECTIVE_BYTES;
    while end > 0 && !one_line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &one_line[..end])
}

pub fn registry_list(database: &str, limit: Option<&str>) -> Result<String, String> {
    let cap = limit
        .map(|value| {
            value
                .parse()
                .map_err(|_| "invalid registry limit".to_string())
        })
        .transpose()?
        .unwrap_or(usize::MAX);
    let registry = SqliteAgentRegistry::open(database).map_err(|e| format!("registry: {e}"))?;
    let lines =
        registry
            .list_agents()
            .map_err(|e| format!("registry: {e}"))?
            .into_iter()
            .take(cap)
            .map(|agent| {
                format!(
                "id={} name={} tier={} driver_kind={} executable={} version={} args={} concurrency={} tags={} driver_config={}",
                agent.id,
                agent.name,
                agent.tier,
                agent.driver_kind.as_deref().unwrap_or("-"),
                agent.executable.as_deref().unwrap_or("-"),
                agent.runtime_version.as_deref().unwrap_or("-"),
                agent.driver_args_json.as_deref().unwrap_or("-"),
                agent.max_concurrency.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                agent.tags_json.as_deref().unwrap_or("-"),
                agent
                    .driver_config_json
                    .as_deref()
                    .map(bounded_config_note)
                    .unwrap_or_else(|| "-".into()),
            )
            })
            .collect::<Vec<_>>()
            .join("\n");
    Ok(lines)
}

/// A short rendering of a driver config for the list surface. A long body is
/// never printed whole; only its bounded head and its size are shown.
pub fn bounded_config_note(raw: &str) -> String {
    const MAX: usize = 80;
    if raw.len() <= MAX {
        return raw.to_string();
    }
    let mut end = MAX;
    while end > 0 && !raw.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... ({} bytes)", &raw[..end], raw.len())
}
