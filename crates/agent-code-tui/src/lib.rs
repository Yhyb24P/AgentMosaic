//! Read-only interactive terminal dashboard for the authoritative team board.

use agent_code_storage::{AgentRegistryRecord, SqliteTaskBoard};
use agent_code_team::TaskBoard;

/// Build the compact board view displayed by the R7 terminal UI.  It reads no
/// runtime-local cache: every line is reconstructed from SQLite task records,
/// attempts and artifact references.
pub fn dashboard_text(board: &SqliteTaskBoard) -> Result<String, String> {
    dashboard_text_with_agents(board, &[])
}

/// Same authoritative board projection with the persisted runtime registry.
pub fn dashboard_text_with_agents(
    board: &SqliteTaskBoard,
    agents: &[AgentRegistryRecord],
) -> Result<String, String> {
    let mut lines = vec![
        "Research Agent System — Team Board".to_string(),
        "\nTeam".to_string(),
        "\nTasks".to_string(),
    ];
    if agents.is_empty() {
        lines.insert(2, "no configured agents".into());
    } else {
        for agent in agents.iter().rev() {
            lines.insert(
                2,
                format!(
                    "agent={} name={} tier={} driver={} concurrency={} tags={}",
                    agent.id,
                    agent.name,
                    agent.tier,
                    agent.driver_kind.as_deref().unwrap_or("-"),
                    agent
                        .max_concurrency
                        .map_or_else(|| "-".into(), |value| value.to_string()),
                    agent.tags_json.as_deref().unwrap_or("[]"),
                ),
            );
        }
    }
    for id in board.task_ids().map_err(|e| format!("tasks: {e:?}"))? {
        let task = board
            .task(id)
            .map_err(|e| format!("task: {e:?}"))?
            .ok_or_else(|| format!("task {id} disappeared"))?;
        let attempts = board.attempts(id).map_err(|e| format!("attempts: {e:?}"))?;
        let artifacts = board
            .artifacts(id)
            .map_err(|e| format!("artifacts: {e:?}"))?;
        lines.push(format!(
            "#{id} parent={} kind={} state={} agent={} attempts={} artifacts={} — {}",
            task.parent_task
                .map_or_else(|| "-".into(), |parent| parent.to_string()),
            task.kind.as_str(),
            task.status.as_str(),
            task.assignee.unwrap_or_else(|| "-".into()),
            attempts.len(),
            artifacts.len(),
            task.objective
        ));
        for artifact in artifacts {
            lines.push(format!(
                "  artifact task={id} path={} sha256={}",
                artifact.path, artifact.sha256
            ));
        }
    }
    lines.push("\nActivity (durable directed summaries)".into());
    for message in board.messages().map_err(|e| format!("messages: {e:?}"))? {
        lines.push(format!(
            "{} -> {}: {}",
            message.from_agent, message.to_agent, message.body
        ));
    }
    lines.push("\nFinal result: use agent-code-cli final <database> <task-id>.".into());
    lines.push(
        "Press q to exit. Controls: agent-code-cli submit/cancel/override/resume/artifact/final."
            .into(),
    );
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use agent_code_storage::SqliteTaskBoard;
    use agent_code_team::{TaskBoard, TaskKind};

    use super::dashboard_text;

    #[test]
    fn dashboard_reads_authoritative_board() {
        let mut board = SqliteTaskBoard::in_memory().unwrap();
        board
            .create_task("inspect dashboard", None, TaskKind::Reasoning, None)
            .unwrap();
        let text = dashboard_text(&board).unwrap();
        assert!(text.contains("#1 parent=- kind=reasoning state=pending"));
        assert!(text.contains("inspect dashboard"));
    }
}
