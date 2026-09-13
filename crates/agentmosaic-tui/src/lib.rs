//! Read-only interactive terminal dashboard for the authoritative team board.

use std::collections::BTreeMap;
use std::io;

use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry, SqliteTaskBoard};
use agentmosaic_team::{TaskBoard, TaskStatus};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::Paragraph, Terminal};
use rusqlite::Connection;

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
    let mut task_lines = Vec::new();
    let mut final_lines = Vec::new();
    let mut active_by_agent = BTreeMap::<String, usize>::new();
    let mut runtime_state_by_agent = BTreeMap::<String, String>::new();
    for id in board.task_ids().map_err(|e| format!("tasks: {e:?}"))? {
        let task = board
            .task(id)
            .map_err(|e| format!("task: {e:?}"))?
            .ok_or_else(|| format!("task {id} disappeared"))?;
        let attempts = board.attempts(id).map_err(|e| format!("attempts: {e:?}"))?;
        let artifacts = board
            .artifacts(id)
            .map_err(|e| format!("artifacts: {e:?}"))?;
        task_lines.push(format!(
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
            task_lines.push(format!(
                "  artifact task={id} path={} sha256={}",
                artifact.path, artifact.sha256
            ));
        }
        for attempt in &attempts {
            if attempt.status == TaskStatus::Running {
                *active_by_agent.entry(attempt.agent_id.clone()).or_default() += 1;
            }
            if let Some(binding) = board
                .external_binding(id, attempt.attempt)
                .map_err(|e| format!("runtime binding: {e}"))?
            {
                runtime_state_by_agent.insert(binding.agent_id, binding.lifecycle_state);
            }
        }
        let (task_refs, artifact_refs) = board
            .final_refs(id)
            .map_err(|e| format!("final refs: {e:?}"))?;
        if !task_refs.is_empty() || !artifact_refs.is_empty() {
            final_lines.push(format!(
                "root_task={id} task_refs={task_refs:?} artifact_refs={}",
                artifact_refs
                    .iter()
                    .map(|reference| format!(
                        "{}:{}#{}",
                        reference.task_id, reference.artifact.path, reference.artifact.sha256
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
    }
    let mut lines = vec!["AgentMosaic — Team Board".to_string(), "\nTeam".to_string()];
    if agents.is_empty() {
        lines.push("no configured agents".into());
    } else {
        for agent in agents {
            let active = active_by_agent.get(&agent.id).copied().unwrap_or_default();
            let capacity = agent
                .max_concurrency
                .map_or_else(|| "-".into(), |value| value.to_string());
            let runtime_state = runtime_state_by_agent
                .get(&agent.id)
                .map(String::as_str)
                .unwrap_or("idle");
            lines.push(format!(
                "agent={} name={} tier={} driver={} version={} active={active}/{capacity} runtime_state={} tags={}",
                agent.id,
                agent.name,
                agent.tier,
                agent.driver_kind.as_deref().unwrap_or("-"),
                agent.runtime_version.as_deref().unwrap_or("-"),
                runtime_state,
                agent.tags_json.as_deref().unwrap_or("[]"),
            ));
        }
    }
    lines.push("\nTasks".into());
    lines.extend(task_lines);
    lines.push("\nActivity (durable directed summaries)".into());
    for message in board.messages().map_err(|e| format!("messages: {e:?}"))? {
        lines.push(format!(
            "{} -> {}: {}",
            message.from_agent, message.to_agent, message.body
        ));
    }
    lines.push("\nFinal result".into());
    if final_lines.is_empty() {
        lines.push("no selected final references".into());
    } else {
        lines.extend(final_lines);
    }
    lines
        .push("Press q to exit. Controls: am submit/cancel/override/resume/artifact/final.".into());
    Ok(lines.join("\n"))
}

/// Run the read-only interactive board on the current terminal until the user
/// presses `q`. This is the `am tui <database>` entry point.
pub fn run(database: &str) -> Result<(), String> {
    let board = SqliteTaskBoard::open(Connection::open(database).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let agents = SqliteAgentRegistry::open(database)
        .map_err(|e| e.to_string())?
        .list_agents()
        .map_err(|e| e.to_string())?;
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
    let result = run_loop(&mut terminal, &board, &agents);
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    board: &SqliteTaskBoard,
    agents: &[AgentRegistryRecord],
) -> Result<(), String> {
    loop {
        let text = dashboard_text_with_agents(board, agents)?;
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new(text), frame.area()))
            .map_err(|e| e.to_string())?;
        if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
            if key.code == KeyCode::Char('q') {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use agentmosaic_storage::{AgentRegistryRecord, ExternalRuntimeBinding, SqliteTaskBoard};
    use agentmosaic_team::{
        ArtifactMeta, SelectedArtifactRef, TaskAttempt, TaskBoard, TaskKind, TaskStatus,
    };

    use super::{dashboard_text, dashboard_text_with_agents};

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

    #[test]
    fn dashboard_projects_persisted_final_selection() {
        let mut board = SqliteTaskBoard::in_memory().unwrap();
        let root = board
            .create_task("select final result", None, TaskKind::Reasoning, None)
            .unwrap();
        board
            .record_final_refs(
                root,
                &[root],
                &[SelectedArtifactRef {
                    task_id: root,
                    artifact: ArtifactMeta {
                        path: "final.txt".into(),
                        sha256: "abc123".into(),
                    },
                }],
            )
            .unwrap();

        let text = dashboard_text(&board).unwrap();
        assert!(text.contains("Final result"));
        assert!(text.contains("root_task=1 task_refs=[1] artifact_refs=1:final.txt#abc123"));
    }

    #[test]
    fn dashboard_projects_authoritative_agent_occupancy_and_runtime_state() {
        let mut board = SqliteTaskBoard::in_memory().unwrap();
        let task = board
            .create_task("running external task", None, TaskKind::Bulk, None)
            .unwrap();
        board.assign(task, "qwen-worker").unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: task,
                attempt: 1,
                agent_id: "qwen-worker".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(task, TaskStatus::Running).unwrap();
        board
            .upsert_external_binding(&ExternalRuntimeBinding {
                team_task_id: task,
                attempt: 1,
                agent_id: "qwen-worker".into(),
                runtime_kind: "acp".into(),
                native_thread_id: Some("opaque-session".into()),
                native_turn_id: None,
                lifecycle_state: "running".into(),
            })
            .unwrap();
        let agents = [AgentRegistryRecord {
            id: "qwen-worker".into(),
            name: "Qwen worker".into(),
            tier: "worker".into(),
            driver_kind: Some("acp".into()),
            executable: Some("qwen".into()),
            runtime_version: Some("0.23.3".into()),
            driver_args_json: Some(r#"["-qw","--acp"]"#.into()),
            max_concurrency: Some(2),
            tags_json: Some(r#"["qwen"]"#.into()),
            driver_config_json: None,
        }];

        let text = dashboard_text_with_agents(&board, &agents).unwrap();
        assert!(text.contains("agent=qwen-worker"));
        assert!(text.contains("active=1/2 runtime_state=running"));
        assert!(!text.contains("opaque-session"));
    }
}
