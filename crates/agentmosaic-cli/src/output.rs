//! Presentation helpers: the bounded, human-readable renderings the CLI
//! prints. No command logic lives here.

use agentmosaic_runtime::TeamRunOutcome;
use agentmosaic_storage::{SqliteAgentRegistry, SqliteTaskBoard};
use agentmosaic_team::TaskBoard;

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
