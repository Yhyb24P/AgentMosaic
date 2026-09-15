//! Read-only interactive terminal dashboard for the authoritative team board.
//!
//! The board is a live projection of durable state rather than a cache: every
//! refresh reopens the database and the agent registry, so work happening in
//! another process appears here within one poll interval and without a
//! keypress. Nothing in this crate mutates a task; the only control is `q`.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use agentmosaic_storage::{
    AgentRegistryRecord, SqliteAgentRegistry, SqliteTaskBoard, MAX_RUNTIME_EVENT_QUERY,
};
use agentmosaic_team::{AgentMessage, RuntimeEvent, SelectedArtifactRef, TaskBoard, TaskStatus};
use crossterm::{
    cursor::Show,
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    widgets::Paragraph,
    Terminal,
};
use rusqlite::Connection;

/// How long the board waits for a key before it redraws.
///
/// Waiting is bounded rather than indefinite, and the wait is long enough to
/// actually sleep: an external writer becomes visible on the next tick, the
/// loop never spins on a zero timeout, and no thread is needed to poll.
pub const POLL_TIMEOUT: Duration = Duration::from_millis(400);

/// How many durable directed messages the activity summary keeps.
pub const MESSAGE_SUMMARY_LIMIT: usize = 5;
/// How many normalized runtime observations the dashboard retains.
pub const RUNTIME_EVENT_SUMMARY_LIMIT: usize = 8;

/// The latest root run, as the board header renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunView {
    pub id: u64,
    pub status: TaskStatus,
    pub objective: String,
    /// The agent the run was assigned to, if any.
    pub lead: Option<String>,
}

/// One registered agent and its live occupancy on the current run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamMember {
    pub id: String,
    pub tier: String,
    /// Running attempts this agent holds on the run's subtree.
    pub running: usize,
    /// The registry's concurrency limit; `None` or `0` means unlimited.
    pub max_concurrency: Option<i64>,
}

impl TeamMember {
    /// Whether this agent currently holds work.
    fn state(&self) -> &'static str {
        if self.running > 0 {
            "running"
        } else {
            "idle"
        }
    }

    /// `running/limit`, or `None` when the agent has no finite limit.
    fn occupancy(&self) -> Option<String> {
        match self.max_concurrency {
            Some(limit) if limit > 0 => Some(format!("{}/{}", self.running, limit)),
            _ => None,
        }
    }
}

/// One task of the run's subtree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskView {
    pub id: u64,
    pub status: TaskStatus,
    pub assignee: Option<String>,
    pub objective: String,
}

/// One artifact recorded against a task of the run's subtree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactView {
    pub task_id: u64,
    pub path: String,
    pub sha256: String,
}

/// One privacy-filtered runtime observation in the dashboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEventView {
    pub task_id: u64,
    pub attempt: u32,
    pub agent: String,
    pub runtime: Option<String>,
    pub kind: String,
    pub summary: String,
}

/// The durable final selection for one run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FinalRefs {
    pub task_refs: Vec<u64>,
    pub artifact_refs: Vec<SelectedArtifactRef>,
}

impl FinalRefs {
    /// Whether the run has selected nothing yet.
    pub fn is_pending(&self) -> bool {
        self.task_refs.is_empty() && self.artifact_refs.is_empty()
    }
}

/// One complete, self-consistent read of the project's durable state.
///
/// A snapshot is built from the database on every refresh, so holding one is
/// never a claim about the board's current content.
#[derive(Debug, Clone)]
pub struct BoardSnapshot {
    /// The latest root run, or `None` when the project has none yet.
    pub run: Option<RunView>,
    /// Registered agents with their live occupancy.
    pub team: Vec<TeamMember>,
    /// The run's subtree: its root task and every descendant.
    pub tasks: Vec<TaskView>,
    /// Artifacts recorded on that subtree.
    pub artifacts: Vec<ArtifactView>,
    /// The durable final selection for the run.
    pub final_refs: FinalRefs,
    /// The newest durable directed messages, bounded by [`MESSAGE_SUMMARY_LIMIT`].
    pub messages: Vec<AgentMessage>,
    /// Recent normalized runtime observations; never task authority or a raw transcript.
    pub runtime_events: Vec<RuntimeEventView>,
}

/// Read one complete board snapshot from `database`.
///
/// Both handles are opened fresh on every call. That is deliberate: it costs
/// one small SQLite open per refresh and buys cross-process visibility, so a
/// run driven from another process appears here, and the agent registry is
/// live instead of captured once at start-up.
pub fn load_snapshot(database: &Path) -> Result<BoardSnapshot, String> {
    let board = SqliteTaskBoard::open(
        Connection::open(database).map_err(|e| format!("open database: {e}"))?,
    )
    .map_err(|e| format!("open task board: {e}"))?;
    let agents = SqliteAgentRegistry::open(database)
        .map_err(|e| format!("open agent registry: {e}"))?
        .list_agents()
        .map_err(|e| format!("list agents: {e}"))?;
    snapshot_from(&board, &agents)
}

/// Project one board connection plus one registry read into a snapshot.
fn snapshot_from(
    board: &SqliteTaskBoard,
    agents: &[AgentRegistryRecord],
) -> Result<BoardSnapshot, String> {
    let run = board
        .latest_root_task()
        .map_err(|e| format!("read runs: {e}"))?;
    let Some(run) = run else {
        // No run is a normal state for a fresh project, not a failure.
        return Ok(BoardSnapshot {
            run: None,
            team: team_view(agents, &BTreeMap::new()),
            tasks: Vec::new(),
            artifacts: Vec::new(),
            final_refs: FinalRefs::default(),
            messages: message_summary(board)?,
            runtime_events: Vec::new(),
        });
    };

    let mut subtree = vec![run.id];
    subtree.extend(
        board
            .descendants_of(run.id)
            .map_err(|e| format!("read run tasks: {e}"))?,
    );

    let mut tasks = Vec::new();
    let mut artifacts = Vec::new();
    let mut running = BTreeMap::<String, usize>::new();
    let mut runtime_events = Vec::new();
    for id in subtree {
        let record = board
            .task(id)
            .map_err(|e| format!("read task {id}: {e}"))?
            .ok_or_else(|| format!("task {id} disappeared while reading the board"))?;
        for attempt in board
            .attempts(id)
            .map_err(|e| format!("read task {id} attempts: {e}"))?
        {
            if attempt.status == TaskStatus::Running {
                *running.entry(attempt.agent_id).or_default() += 1;
            }
        }
        for artifact in board
            .artifacts(id)
            .map_err(|e| format!("read task {id} artifacts: {e}"))?
        {
            artifacts.push(ArtifactView {
                task_id: id,
                path: artifact.path,
                sha256: artifact.sha256,
            });
        }
        tasks.push(TaskView {
            id: record.id,
            status: record.status,
            assignee: record.assignee,
            objective: record.objective,
        });
        runtime_events.extend(
            board
                .latest_runtime_events(id, MAX_RUNTIME_EVENT_QUERY)
                .map_err(|e| format!("read runtime events for task {id}: {e}"))?
                .into_iter()
                .map(|stored| RuntimeEventView {
                    task_id: id,
                    attempt: stored.record.attempt,
                    agent: stored.record.agent_id,
                    runtime: stored.record.runtime_name,
                    kind: stored.record.event.kind().into(),
                    summary: runtime_summary(&stored.record.event),
                }),
        );
    }

    let (task_refs, artifact_refs) = board
        .final_refs(run.id)
        .map_err(|e| format!("read final refs: {e}"))?;
    Ok(BoardSnapshot {
        run: Some(RunView {
            id: run.id,
            status: run.status,
            objective: run.objective,
            lead: run.assignee,
        }),
        team: team_view(agents, &running),
        tasks,
        artifacts,
        final_refs: FinalRefs {
            task_refs,
            artifact_refs,
        },
        messages: message_summary(board)?,
        runtime_events: {
            runtime_events.truncate(RUNTIME_EVENT_SUMMARY_LIMIT);
            runtime_events
        },
    })
}

/// The registry rows with occupancy, plus any agent holding a running attempt
/// that has no registration left (its work is real even if its row is gone).
fn team_view(agents: &[AgentRegistryRecord], running: &BTreeMap<String, usize>) -> Vec<TeamMember> {
    let mut team: Vec<TeamMember> = agents
        .iter()
        .map(|agent| TeamMember {
            id: agent.id.clone(),
            tier: agent.tier.clone(),
            running: running.get(&agent.id).copied().unwrap_or_default(),
            max_concurrency: agent.max_concurrency,
        })
        .collect();
    for (id, count) in running {
        if !team.iter().any(|member| &member.id == id) {
            team.push(TeamMember {
                id: id.clone(),
                tier: "unregistered".into(),
                running: *count,
                max_concurrency: None,
            });
        }
    }
    team.sort_by(|a, b| a.id.cmp(&b.id));
    team
}

/// The newest directed messages, bounded so a long run cannot push the rest of
/// the board off the screen.
fn message_summary(board: &SqliteTaskBoard) -> Result<Vec<AgentMessage>, String> {
    let mut messages = board
        .messages()
        .map_err(|e| format!("read messages: {e}"))?;
    if messages.len() > MESSAGE_SUMMARY_LIMIT {
        messages.drain(..messages.len() - MESSAGE_SUMMARY_LIMIT);
    }
    Ok(messages)
}

fn runtime_summary(event: &RuntimeEvent) -> String {
    match event {
        RuntimeEvent::AssistantMessageCompleted { text } => truncate(text, 160),
        RuntimeEvent::ToolCallStarted { tool, .. } => format!("tool {tool} started"),
        RuntimeEvent::ToolCallCompleted { tool, ok, .. } => {
            format!("tool {tool} {}", if *ok { "completed" } else { "failed" })
        }
        RuntimeEvent::RuntimeWarning { message, .. }
        | RuntimeEvent::RuntimeError { message, .. } => truncate(message, 160),
        RuntimeEvent::UsageUpdated {
            input_tokens,
            output_tokens,
            ..
        } => format!(
            "usage input={} output={}",
            input_tokens.map_or("-".into(), |value| value.to_string()),
            output_tokens.map_or("-".into(), |value| value.to_string())
        ),
        _ => event.kind().replace('_', " "),
    }
}

/// Render one snapshot as the text the board shows, bounded to the terminal.
///
/// The renderer is pure: it takes the snapshot and the terminal dimensions and
/// returns lines. No terminal is involved, so the information architecture is
/// testable without a TTY.
pub fn render_snapshot(snapshot: &BoardSnapshot, width: u16, height: u16) -> String {
    let width = usize::from(width).max(1);
    let mut lines = vec![
        truncate("AgentMosaic — Team Board", width),
        String::new(),
        truncate("Run", width),
    ];

    match &snapshot.run {
        Some(run) => {
            lines.push(truncate(
                &format!("  #{}  {}", run.id, run.status.as_str()),
                width,
            ));
            lines.push(truncate(&format!("  objective: {}", run.objective), width));
            lines.push(truncate(
                &format!("  lead: {}", run.lead.as_deref().unwrap_or("unassigned")),
                width,
            ));
        }
        None => lines.push(truncate("  no runs recorded", width)),
    }

    lines.push(String::new());
    lines.push(truncate("Team", width));
    if snapshot.team.is_empty() {
        lines.push(truncate("  no configured agents", width));
    } else {
        let id_width = widest(snapshot.team.iter().map(|member| member.id.as_str()));
        let tier_width = widest(snapshot.team.iter().map(|member| member.tier.as_str()));
        for member in &snapshot.team {
            let mut row = format!(
                "  {}  {}  {}",
                column(&member.id, id_width),
                column(&member.tier, tier_width),
                member.state()
            );
            if let Some(occupancy) = member.occupancy() {
                row.push_str(&format!(" {occupancy}"));
            }
            lines.push(truncate(&row, width));
        }
    }

    lines.push(String::new());
    lines.push(truncate("Tasks", width));
    if snapshot.tasks.is_empty() {
        lines.push(truncate("  no tasks in this run", width));
    } else {
        let id_width = widest(snapshot.tasks.iter().map(|task| format!("#{}", task.id)));
        let status_width = widest(snapshot.tasks.iter().map(|task| task.status.as_str()));
        let assignee_width = widest(
            snapshot
                .tasks
                .iter()
                .map(|task| task.assignee.as_deref().unwrap_or("-")),
        );
        for task in &snapshot.tasks {
            let row = format!(
                "  {}  {}  {}  {}",
                column(&format!("#{}", task.id), id_width),
                column(task.status.as_str(), status_width),
                column(task.assignee.as_deref().unwrap_or("-"), assignee_width),
                task.objective
            );
            lines.push(truncate(&row, width));
        }
    }

    lines.push(String::new());
    lines.push(truncate("Artifacts", width));
    if snapshot.artifacts.is_empty() {
        lines.push(truncate("  none recorded", width));
    } else {
        for artifact in &snapshot.artifacts {
            lines.push(truncate(
                &format!("  #{}  {}", artifact.task_id, artifact.path),
                width,
            ));
        }
    }

    lines.push(String::new());
    lines.push(truncate("Runtime", width));
    if snapshot.runtime_events.is_empty() {
        lines.push(truncate("  no runtime events", width));
    } else {
        for event in &snapshot.runtime_events {
            lines.push(truncate(
                &format!(
                    "  #{} a{} {} {} {}",
                    event.task_id,
                    event.attempt,
                    event.agent,
                    event.runtime.as_deref().unwrap_or("-"),
                    event.summary
                ),
                width,
            ));
        }
    }

    lines.push(String::new());
    lines.push(truncate("Activity", width));
    if snapshot.messages.is_empty() {
        lines.push(truncate("  no directed messages", width));
    } else {
        for message in &snapshot.messages {
            lines.push(truncate(
                &format!(
                    "  {} -> {}: {}",
                    message.from_agent, message.to_agent, message.body
                ),
                width,
            ));
        }
    }

    lines.push(String::new());
    lines.push(truncate("Final", width));
    if snapshot.final_refs.is_pending() {
        lines.push(truncate("  pending", width));
    } else {
        if !snapshot.final_refs.task_refs.is_empty() {
            let refs = snapshot
                .final_refs
                .task_refs
                .iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(truncate(&format!("  tasks: {refs}"), width));
        }
        for reference in &snapshot.final_refs.artifact_refs {
            lines.push(truncate(
                &format!("  #{}  {}", reference.task_id, reference.artifact.path),
                width,
            ));
        }
    }

    lines.push(String::new());
    lines.push(truncate("q quit", width));

    lines.truncate(usize::from(height).max(1));
    lines.join("\n")
}

/// The widest of `values` in characters, or zero.
fn widest(values: impl Iterator<Item = impl AsRef<str>>) -> usize {
    values
        .map(|value| value.as_ref().chars().count())
        .max()
        .unwrap_or_default()
}

/// What one keyboard event means to the board loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopAction {
    /// Redraw and keep polling.
    Continue,
    /// Leave the board.
    Quit,
}

/// Decide what an event means, without touching a terminal.
///
/// The decision is separated from the event source so that `q` quitting the
/// board is a unit test rather than a keypress.
pub fn event_action(event: &Event) -> LoopAction {
    match event {
        Event::Key(key) if key.code == KeyCode::Char('q') => LoopAction::Quit,
        _ => LoopAction::Continue,
    }
}

/// The terminal state a board session must put back on the way out.
///
/// Every action is injectable so that restore-on-every-path can be tested
/// without a real terminal: production runs the crossterm calls, and a test
/// substitutes a recorder that only remembers what it was asked to do.
pub trait TerminalRestore {
    /// Stop raw input: echo and line buffering come back.
    fn disable_raw_mode(&mut self);
    /// Leave the alternate screen, revealing the user's own output again.
    fn leave_alternate_screen(&mut self);
    /// Bring the cursor back.
    fn show_cursor(&mut self);

    /// All three, in the order a terminal needs them.
    fn restore(&mut self) {
        self.disable_raw_mode();
        self.leave_alternate_screen();
        self.show_cursor();
    }
}

/// The production restore: crossterm escapes on an output handle.
pub struct CrosstermRestore<W: Write> {
    out: W,
}

impl<W: Write> CrosstermRestore<W> {
    pub fn new(out: W) -> Self {
        Self { out }
    }
}

impl<W: Write> TerminalRestore for CrosstermRestore<W> {
    fn disable_raw_mode(&mut self) {
        let _ = disable_raw_mode();
    }

    fn leave_alternate_screen(&mut self) {
        let _ = execute!(self.out, LeaveAlternateScreen);
    }

    fn show_cursor(&mut self) {
        let _ = execute!(self.out, Show);
    }
}

/// Restores the terminal on every exit path, including a panic.
///
/// `Drop` runs on an ordinary `Ok`/`Err` return and during unwinding, so the
/// board can never leave the user in raw mode on the alternate screen. The
/// guard is armed before the alternate screen is entered, which is what covers
/// the failure path in between.
pub struct TerminalGuard<'a, R: TerminalRestore + ?Sized> {
    restore: &'a mut R,
    armed: bool,
}

impl<'a, R: TerminalRestore + ?Sized> TerminalGuard<'a, R> {
    pub fn new(restore: &'a mut R) -> Self {
        Self {
            restore,
            armed: true,
        }
    }

    /// Restore now, at most once.
    pub fn restore_now(&mut self) {
        if self.armed {
            self.armed = false;
            self.restore.restore();
        }
    }
}

impl<R: TerminalRestore + ?Sized> Drop for TerminalGuard<'_, R> {
    fn drop(&mut self) {
        self.restore_now();
    }
}

/// Run the read-only interactive board on the current terminal until the user
/// presses `q`. This is the `am tui <database>` entry point.
pub fn run(database: &str) -> Result<(), String> {
    let path = Path::new(database);
    let mut restore = CrosstermRestore::new(io::stdout());
    enable_raw_mode().map_err(|e| e.to_string())?;
    // Armed before the alternate screen is entered: a failure between raw mode
    // and here has to hand the terminal back too.
    let _guard = TerminalGuard::new(&mut restore);
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
    run_loop(&mut terminal, path)
}

/// Draw a fresh snapshot, then wait for input before doing it again.
fn run_loop<B: Backend>(terminal: &mut Terminal<B>, database: &Path) -> Result<(), String> {
    loop {
        let snapshot = load_snapshot(database)?;
        draw(terminal, &snapshot)?;
        if poll_action(POLL_TIMEOUT)? == LoopAction::Quit {
            return Ok(());
        }
    }
}

/// Draw one snapshot, bounded to the terminal's current size.
fn draw<B: Backend>(terminal: &mut Terminal<B>, snapshot: &BoardSnapshot) -> Result<(), String> {
    let size = terminal.size().map_err(|e| e.to_string())?;
    let text = render_snapshot(snapshot, size.width, size.height);
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new(text), frame.area()))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Wait up to `timeout` for a key, then report what to do next.
///
/// A timed-out poll is the redraw trigger, so an external writer is picked up
/// without any input at all.
fn poll_action(timeout: Duration) -> Result<LoopAction, String> {
    if !event::poll(timeout).map_err(|e| e.to_string())? {
        return Ok(LoopAction::Continue);
    }
    let event = event::read().map_err(|e| e.to_string())?;
    Ok(event_action(&event))
}

/// Clip `text` to at most `limit` characters, on a character boundary.
fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(limit.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}

/// Clip `text` to `width` characters and pad it with spaces to exactly that.
fn column(text: &str, width: usize) -> String {
    let clipped = truncate(text, width);
    let padding = width.saturating_sub(clipped.chars().count());
    format!("{clipped}{}", " ".repeat(padding))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use agentmosaic_storage::{
        AgentRegistryRecord, ExternalRuntimeBinding, SqliteAgentRegistry, SqliteTaskBoard,
    };
    use agentmosaic_team::{
        AgentMessage, ArtifactMeta, SelectedArtifactRef, TaskAttempt, TaskBoard, TaskKind,
        TaskStatus,
    };
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use rusqlite::Connection;

    use super::{
        event_action, load_snapshot, render_snapshot, LoopAction, TerminalGuard, TerminalRestore,
        MESSAGE_SUMMARY_LIMIT, POLL_TIMEOUT,
    };

    /// A database path owned by one test, so parallel tests never collide.
    fn temp_database(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "agentmosaic_tui_task06_{name}_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// A board connection onto `path`, as a writer (or another process) would
    /// have.
    fn board_on(path: &Path) -> SqliteTaskBoard {
        SqliteTaskBoard::open(Connection::open(path).expect("open database")).expect("open board")
    }

    fn worker(id: &str, capacity: i64) -> AgentRegistryRecord {
        AgentRegistryRecord {
            id: id.into(),
            name: format!("{id} name"),
            tier: "worker".into(),
            driver_kind: Some("acp".into()),
            executable: Some("qwen".into()),
            runtime_version: Some("0.23.3".into()),
            driver_args_json: Some(r#"["-qw","--acp"]"#.into()),
            max_concurrency: Some(capacity),
            tags_json: Some(r#"["qwen"]"#.into()),
            driver_config_json: None,
        }
    }

    /// What the board shows for `path` at a terminal size that fits it all.
    fn board_text(path: &Path) -> String {
        let snapshot = load_snapshot(path).expect("snapshot");
        render_snapshot(&snapshot, 100, 40)
    }

    /// The indented body lines of one `Header` section.
    fn section<'a>(text: &'a str, header: &str) -> Vec<&'a str> {
        let lines: Vec<&str> = text.lines().collect();
        let Some(start) = lines.iter().position(|line| *line == header) else {
            return Vec::new();
        };
        lines[start + 1..]
            .iter()
            .copied()
            .take_while(|line| line.starts_with("  "))
            .collect()
    }

    /// A line with its column padding collapsed, so assertions ignore layout.
    fn compact(line: &str) -> String {
        line.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn compact_lines(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| compact(line)).collect()
    }

    /// The authoritative board is read from durable storage: the run, its
    /// objective and its subtree come from the task rows themselves.
    #[test]
    fn dashboard_reads_authoritative_board() {
        let path = temp_database("authoritative");
        let mut board = board_on(&path);
        board
            .create_task("inspect dashboard", None, TaskKind::Reasoning, None)
            .unwrap();

        let text = board_text(&path);
        assert!(text.starts_with("AgentMosaic — Team Board"));
        assert_eq!(
            compact_lines(&section(&text, "Run")),
            vec![
                "#1 pending",
                "objective: inspect dashboard",
                "lead: unassigned"
            ]
        );
        assert_eq!(
            compact_lines(&section(&text, "Tasks")),
            vec!["#1 pending - inspect dashboard"]
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The persisted final selection is projected as it was recorded, and the
    /// artifacts the subtree produced are listed with the task that made them.
    #[test]
    fn dashboard_projects_persisted_final_selection() {
        let path = temp_database("final_selection");
        let mut board = board_on(&path);
        let root = board
            .create_task("select final result", None, TaskKind::Reasoning, None)
            .unwrap();
        let worker_task = board
            .create_task("produce the report", Some(root), TaskKind::Bulk, None)
            .unwrap();
        let selected = ArtifactMeta {
            path: "tests/report.txt".into(),
            sha256: "abc123".into(),
        };
        board.record_artifact(worker_task, &selected).unwrap();
        board
            .record_final_refs(
                root,
                &[worker_task],
                &[SelectedArtifactRef {
                    task_id: worker_task,
                    artifact: selected,
                }],
            )
            .unwrap();

        let snapshot = load_snapshot(&path).unwrap();
        assert_eq!(snapshot.final_refs.task_refs, vec![worker_task]);
        assert_eq!(snapshot.artifacts.len(), 1);

        let text = board_text(&path);
        assert_eq!(
            compact_lines(&section(&text, "Artifacts")),
            vec![format!("#{worker_task} tests/report.txt")]
        );
        assert_eq!(
            compact_lines(&section(&text, "Final")),
            vec![
                format!("tasks: #{worker_task}"),
                format!("#{worker_task} tests/report.txt"),
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Occupancy is projected from the durable attempts against the registry's
    /// concurrency limit, and no opaque runtime identifier is ever rendered.
    #[test]
    fn dashboard_projects_authoritative_agent_occupancy_and_runtime_state() {
        let path = temp_database("occupancy");
        let mut board = board_on(&path);
        let root = board
            .create_task("run the team", None, TaskKind::Reasoning, None)
            .unwrap();
        let task = board
            .create_task("running external task", Some(root), TaskKind::Bulk, None)
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
        SqliteAgentRegistry::open(&path)
            .unwrap()
            .upsert_agent(&worker("qwen-worker", 2))
            .unwrap();

        let snapshot = load_snapshot(&path).unwrap();
        assert_eq!(snapshot.team.len(), 1);
        assert_eq!(snapshot.team[0].running, 1);

        let text = board_text(&path);
        assert_eq!(
            compact_lines(&section(&text, "Team")),
            vec!["qwen-worker worker running 1/2"]
        );
        assert!(!text.contains("opaque-session"));
        let _ = std::fs::remove_file(&path);
    }

    /// A run is a root `reasoning` task: the newest one is the one shown, and
    /// an older run's work never leaks into its section.
    #[test]
    fn latest_run_selection_prefers_the_newest_root() {
        let path = temp_database("latest_run");
        let mut board = board_on(&path);
        board
            .create_task("first objective", None, TaskKind::Reasoning, None)
            .unwrap();
        let newest = board
            .create_task("second objective", None, TaskKind::Reasoning, None)
            .unwrap();
        let child = board
            .create_task("second objective work", Some(newest), TaskKind::Bulk, None)
            .unwrap();

        let snapshot = load_snapshot(&path).unwrap();
        assert_eq!(snapshot.run.as_ref().map(|run| run.id), Some(newest));
        assert_eq!(
            snapshot
                .tasks
                .iter()
                .map(|task| task.id)
                .collect::<Vec<_>>(),
            vec![newest, child]
        );

        let text = board_text(&path);
        assert!(text.contains("second objective"));
        assert!(!text.contains("first objective"));
        let _ = std::fs::remove_file(&path);
    }

    /// Every task of the subtree renders with its state, assignee and
    /// objective, root included.
    #[test]
    fn task_states_and_assignees_render_in_the_run_subtree() {
        let path = temp_database("task_states");
        let mut board = board_on(&path);
        let root = board
            .create_task("implement parser UX", None, TaskKind::Reasoning, None)
            .unwrap();
        let running = board
            .create_task("implement parser", Some(root), TaskKind::Bulk, None)
            .unwrap();
        board.assign(running, "worker").unwrap();
        board.set_status(running, TaskStatus::Running).unwrap();
        let done = board
            .create_task("add tests", Some(root), TaskKind::Bulk, None)
            .unwrap();
        board.assign(done, "worker").unwrap();
        board.set_status(done, TaskStatus::Succeeded).unwrap();

        let snapshot = load_snapshot(&path).unwrap();
        assert_eq!(snapshot.tasks.len(), 3);

        let text = board_text(&path);
        let tasks = compact_lines(&section(&text, "Tasks"));
        assert!(tasks.contains(&"#1 pending - implement parser UX".to_string()));
        assert!(tasks.contains(&format!("#{running} running worker implement parser")));
        assert!(tasks.contains(&format!("#{done} succeeded worker add tests")));
        let _ = std::fs::remove_file(&path);
    }

    /// A run that has selected nothing says so instead of showing an empty
    /// section.
    #[test]
    fn run_without_a_selection_renders_a_pending_final() {
        let path = temp_database("pending_final");
        let mut board = board_on(&path);
        board
            .create_task("still running", None, TaskKind::Reasoning, None)
            .unwrap();

        let snapshot = load_snapshot(&path).unwrap();
        assert!(snapshot.final_refs.is_pending());
        assert_eq!(
            compact_lines(&section(&board_text(&path), "Final")),
            vec!["pending"]
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A project with no runs renders an empty state rather than failing.
    #[test]
    fn project_without_runs_renders_an_empty_state() {
        let path = temp_database("no_runs");
        let snapshot = load_snapshot(&path).unwrap();
        assert!(snapshot.run.is_none());
        assert!(snapshot.team.is_empty());

        let text = render_snapshot(&snapshot, 100, 40);
        assert_eq!(
            compact_lines(&section(&text, "Run")),
            vec!["no runs recorded"]
        );
        assert_eq!(
            compact_lines(&section(&text, "Team")),
            vec!["no configured agents"]
        );
        assert_eq!(
            compact_lines(&section(&text, "Tasks")),
            vec!["no tasks in this run"]
        );
        assert_eq!(compact_lines(&section(&text, "Final")), vec!["pending"]);
        let _ = std::fs::remove_file(&path);
    }

    /// Long fields are clipped to the terminal, not left to wrap or overflow.
    #[test]
    fn long_fields_are_clipped_to_the_terminal_size() {
        let path = temp_database("clipping");
        let long_objective = "objective words ".repeat(20);
        let mut board = board_on(&path);
        board
            .create_task(&long_objective, None, TaskKind::Reasoning, None)
            .unwrap();

        let snapshot = load_snapshot(&path).unwrap();
        let text = render_snapshot(&snapshot, 24, 8);
        assert!(text.lines().count() <= 8);
        assert!(text.lines().all(|line| line.chars().count() <= 24));
        assert!(!text.contains(long_objective.trim()));
        let _ = std::fs::remove_file(&path);
    }

    /// The activity summary is a bounded tail of the durable messages.
    #[test]
    fn message_summary_keeps_only_the_newest_bounded_tail() {
        let path = temp_database("messages");
        let mut board = board_on(&path);
        board
            .create_task("run", None, TaskKind::Reasoning, None)
            .unwrap();
        for index in 0..(MESSAGE_SUMMARY_LIMIT + 2) {
            board
                .record_message(&AgentMessage {
                    from_agent: "worker".into(),
                    to_agent: "lead".into(),
                    body: format!("update {index}"),
                })
                .unwrap();
        }

        let snapshot = load_snapshot(&path).unwrap();
        assert_eq!(snapshot.messages.len(), MESSAGE_SUMMARY_LIMIT);
        assert_eq!(snapshot.messages.first().unwrap().body, "update 2");
        assert_eq!(
            snapshot.messages.last().unwrap().body,
            format!("update {}", MESSAGE_SUMMARY_LIMIT + 1)
        );
        assert_eq!(
            compact_lines(&section(&board_text(&path), "Activity")).len(),
            MESSAGE_SUMMARY_LIMIT
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The registry is reloaded, not captured once: a row written by another
    /// connection appears without a keypress, and its removal disappears.
    #[test]
    fn registry_reload_sees_rows_written_by_another_connection() {
        let path = temp_database("registry_reload");
        let mut board = board_on(&path);
        board
            .create_task("live run", None, TaskKind::Reasoning, None)
            .unwrap();
        assert!(load_snapshot(&path).unwrap().team.is_empty());

        let other = SqliteAgentRegistry::open(&path).unwrap();
        other.upsert_agent(&worker("added-worker", 1)).unwrap();
        let added = load_snapshot(&path).unwrap();
        assert_eq!(
            added
                .team
                .iter()
                .map(|member| member.id.as_str())
                .collect::<Vec<_>>(),
            ["added-worker"]
        );

        assert!(other.delete_agent("added-worker").unwrap());
        assert!(load_snapshot(&path).unwrap().team.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    /// Board state written through a different connection is visible in a
    /// later snapshot, which is what makes the board live and cross-process.
    #[test]
    fn board_reload_sees_task_state_written_by_another_connection() {
        let path = temp_database("board_reload");
        let root = {
            let mut writer = board_on(&path);
            writer
                .create_task("watch me", None, TaskKind::Reasoning, None)
                .unwrap()
        };
        let before = load_snapshot(&path).unwrap();
        assert_eq!(before.run.as_ref().unwrap().status, TaskStatus::Pending);

        let mut other = board_on(&path);
        other.assign(root, "worker").unwrap();
        other.set_status(root, TaskStatus::Succeeded).unwrap();

        let after = load_snapshot(&path).unwrap();
        assert_eq!(after.run.as_ref().unwrap().status, TaskStatus::Succeeded);
        assert_eq!(
            compact_lines(&section(&board_text(&path), "Run"))[0],
            "#1 succeeded"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// `q` is the only control, and every other event is ignored.
    #[test]
    fn q_key_quits_and_other_keys_do_not() {
        let quit = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!(event_action(&quit), LoopAction::Quit);
        for event in [
            Event::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Event::Resize(80, 24),
        ] {
            assert_eq!(event_action(&event), LoopAction::Continue);
        }
    }

    /// The board polls on a bounded, non-zero timeout: no busy loop, and no
    /// waiting forever for a keypress.
    #[test]
    fn poll_timeout_is_bounded_polling() {
        assert!(POLL_TIMEOUT > Duration::ZERO);
        assert!(
            POLL_TIMEOUT >= Duration::from_millis(250),
            "a very short timeout would spin"
        );
        assert!(
            POLL_TIMEOUT <= Duration::from_millis(500),
            "a longer wait would read as a stale board"
        );
    }

    /// A recorder standing in for a real terminal.
    #[derive(Default)]
    struct RecordingRestore {
        calls: Vec<&'static str>,
    }

    impl TerminalRestore for RecordingRestore {
        fn disable_raw_mode(&mut self) {
            self.calls.push("disable_raw_mode");
        }

        fn leave_alternate_screen(&mut self) {
            self.calls.push("leave_alternate_screen");
        }

        fn show_cursor(&mut self) {
            self.calls.push("show_cursor");
        }
    }

    const RESTORE_CALLS: [&str; 3] = ["disable_raw_mode", "leave_alternate_screen", "show_cursor"];

    #[test]
    fn terminal_cleanup_runs_on_the_normal_exit_path() {
        let mut restore = RecordingRestore::default();
        {
            let _guard = TerminalGuard::new(&mut restore);
        }
        assert_eq!(restore.calls, RESTORE_CALLS);
    }

    /// The path between leaving raw mode and entering the alternate screen:
    /// entering fails, and the terminal is handed back anyway.
    #[test]
    fn terminal_cleanup_runs_when_the_alternate_screen_fails() {
        fn enter_alternate_screen() -> Result<(), String> {
            Err("enter alternate screen: not a terminal".into())
        }

        let mut restore = RecordingRestore::default();
        let outcome: Result<(), String> = (|| {
            let _guard = TerminalGuard::new(&mut restore);
            enter_alternate_screen()?;
            Ok(())
        })();
        assert!(outcome.is_err());
        assert_eq!(restore.calls, RESTORE_CALLS);
    }

    /// A panic inside the board loop unwinds through the guard, so the
    /// terminal is restored rather than left in raw mode.
    #[test]
    fn terminal_cleanup_runs_when_the_board_loop_panics() {
        let mut restore = RecordingRestore::default();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = TerminalGuard::new(&mut restore);
            panic!("the board loop panicked");
        }));
        assert!(outcome.is_err());
        assert_eq!(restore.calls, RESTORE_CALLS);
    }
}
