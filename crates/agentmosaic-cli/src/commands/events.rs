//! Privacy-filtered read-only projection of durable runtime events.

use agentmosaic_storage::{StoredRuntimeEvent, MAX_RUNTIME_EVENT_QUERY};
use agentmosaic_team::{RuntimeEvent, TaskBoard, TaskKind};
use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

use crate::{
    json::{self, RuntimeEventJson, RuntimeEventListJson},
    project::ProjectContext,
};

/// Render the latest run's subtree or one exact task. Events are observations:
/// this command never writes the board and never launches a runtime.
pub fn list(target: Option<String>, machine: bool) -> Result<String, String> {
    let events = read(target)?;
    if machine {
        return json::encode(&RuntimeEventListJson { events });
    }
    Ok(events
        .into_iter()
        .map(render)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Poll durable observations using bounded per-attempt cursors. This performs
/// no task mutation; the OS default Ctrl-C terminates this reader only.
pub fn follow(target: Option<String>, machine: bool, mut output: impl Write) -> Result<(), String> {
    // Per-attempt monotonic cursors bound memory for a long-lived reader. If
    // the cap is exceeded this observer may replay a line, never task state.
    let mut cursors = BTreeMap::<(u64, u32), u64>::new();
    loop {
        for event in read(target.clone())? {
            let key = (event.task_id, event.attempt);
            if cursors
                .get(&key)
                .is_none_or(|sequence| event.sequence > *sequence)
            {
                let sequence = event.sequence;
                let line = if machine {
                    json::encode(&event)?
                } else {
                    render(event)
                };
                writeln!(output, "{line}").map_err(|error| format!("write events: {error}"))?;
                cursors.insert(key, sequence);
            }
        }
        if cursors.len() > 1024 {
            cursors.clear();
        }
        output
            .flush()
            .map_err(|error| format!("flush events: {error}"))?;
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn read(target: Option<String>) -> Result<Vec<RuntimeEventJson>, String> {
    let board = ProjectContext::discover()?.open_board()?;
    let task_ids = match target {
        Some(value) => vec![value
            .parse::<u64>()
            .map_err(|_| "events requires a numeric task or run id".to_string())?],
        None => {
            let run = board
                .latest_root_task()
                .map_err(|e| format!("{e:?}"))?
                .ok_or_else(|| "no runs in this project yet".to_string())?;
            let mut ids = vec![run.id];
            ids.extend(board.descendants_of(run.id).map_err(|e| format!("{e:?}"))?);
            ids
        }
    };
    let mut events = Vec::new();
    for task_id in task_ids {
        let task = board
            .task(task_id)
            .map_err(|e| format!("{e:?}"))?
            .ok_or_else(|| format!("no task #{task_id} in this project"))?;
        let ids = if task.parent_task.is_none() && task.kind == TaskKind::Reasoning {
            let mut ids = vec![task_id];
            ids.extend(
                board
                    .descendants_of(task_id)
                    .map_err(|e| format!("{e:?}"))?,
            );
            ids
        } else {
            vec![task_id]
        };
        for id in ids {
            events.extend(
                board
                    .latest_runtime_events(id, MAX_RUNTIME_EVENT_QUERY)
                    .map_err(|e| format!("runtime events: {e}"))?,
            );
        }
    }
    events.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.sequence.cmp(&right.sequence))
    });
    Ok(events.into_iter().map(project).collect::<Vec<_>>())
}

fn render(event: RuntimeEventJson) -> String {
    format!(
        "{} task={} attempt={} agent={} runtime={} event={} {}",
        event.timestamp,
        event.task_id,
        event.attempt,
        event.agent,
        event.runtime.unwrap_or_else(|| "-".into()),
        event.event,
        event.summary
    )
}

fn project(stored: StoredRuntimeEvent) -> RuntimeEventJson {
    let record = stored.record;
    RuntimeEventJson {
        task_id: record.task_id,
        attempt: record.attempt,
        sequence: stored.sequence,
        timestamp: stored.created_at,
        agent: record.agent_id,
        runtime: record.runtime_name,
        event: record.event.kind().into(),
        summary: summary(&record.event),
    }
}

fn summary(event: &RuntimeEvent) -> String {
    match event {
        RuntimeEvent::AssistantMessageCompleted { text } => bounded(text),
        RuntimeEvent::ToolCallStarted { tool, .. } => format!("tool {tool} started"),
        RuntimeEvent::ToolCallCompleted { tool, ok, .. } => {
            format!("tool {tool} {}", if *ok { "completed" } else { "failed" })
        }
        RuntimeEvent::FileChanged { path, change } => {
            format!("file {} {:?}", bounded(path), change)
        }
        RuntimeEvent::UsageUpdated {
            input_tokens,
            output_tokens,
            ..
        } => format!(
            "usage input={} output={}",
            input_tokens.map_or("-".into(), |v| v.to_string()),
            output_tokens.map_or("-".into(), |v| v.to_string())
        ),
        RuntimeEvent::RuntimeWarning { message, .. }
        | RuntimeEvent::RuntimeError { message, .. } => bounded(message),
        RuntimeEvent::PermissionRequested { action, .. } => {
            format!("permission requested: {}", bounded(action))
        }
        RuntimeEvent::PermissionResolved { .. } => "permission resolved".into(),
        RuntimeEvent::SubagentStarted { .. } => "subagent started".into(),
        RuntimeEvent::SubagentCompleted { status, .. } => format!("subagent {status}"),
        _ => event.kind().replace('_', " "),
    }
}

fn bounded(value: &str) -> String {
    value.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_exclude_native_session_identifiers_and_tool_payloads() {
        assert_eq!(
            summary(&RuntimeEvent::SessionStarted {
                native_session_id: "private-session".into(),
            }),
            "session started"
        );
        assert_eq!(
            summary(&RuntimeEvent::ToolCallStarted {
                native_call_id: "call-1".into(),
                tool: "Bash".into(),
                input_summary: "rm -rf never-show".into(),
            }),
            "tool Bash started"
        );
    }

    #[test]
    fn machine_projection_is_one_event_without_native_session_data() {
        let event = RuntimeEventJson {
            task_id: 7,
            attempt: 2,
            sequence: 3,
            timestamp: "2026-09-15T00:00:00Z".into(),
            agent: "worker".into(),
            runtime: Some("claude".into()),
            event: "session_started".into(),
            summary: "session started".into(),
        };
        let json = json::encode(&event).unwrap();
        assert!(json.contains("\"sequence\":3"));
        assert!(!json.contains("native_session_id"));
        assert_eq!(render(event), "2026-09-15T00:00:00Z task=7 attempt=2 agent=worker runtime=claude event=session_started session started");
    }
}
