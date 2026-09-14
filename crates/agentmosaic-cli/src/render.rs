//! The `am run` rendering: the team's lifecycle on stderr, the answer on
//! stdout.
//!
//! `am run` is the one command that is a *run* rather than an inspection, so it
//! reports what the team is doing while it happens and keeps stdout composable:
//! `am run "..." > answer.txt` captures the final answer and nothing else.
//!
//! Every line here is derived from a [`RunEvent`] or from the run's own
//! outcome. The durable board stays the only truth: the rendering observes, it
//! never decides. A write failure is swallowed, because a presentation failure
//! must not disturb a run.
//!
//! The compatibility `run-team` spelling does not come through here at all: it
//! keeps its scriptable one-line-per-field payload.

use std::io::{self, Write};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use agentmosaic_team::{bounded_event_text, LeadPhase, RunEvent, RunEventSink};

use crate::json::{self, RunJson};

/// The widest human text one progress line carries, once the whitespace of a
/// multi-line objective or error is collapsed to single spaces.
const MAX_LINE_TEXT_BYTES: usize = 72;

/// The actor column and the status column of a progress line.
const ACTOR_WIDTH: usize = 8;
const STATUS_WIDTH: usize = 10;

/// How `am run` presents one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Progress, artifacts and next steps on stderr; the answer on stdout.
    Human,
    /// The answer on stdout and errors on stderr; no routine progress anywhere.
    Quiet,
    /// The answer on stdout and no routine progress anywhere, so the surface is
    /// machine-readable. Failures still report on stderr. The machine payload
    /// has exactly one replacement point: [`RunMode::payload`].
    Machine,
}

impl RunMode {
    /// Whether routine progress is written at all.
    pub fn progress(self) -> bool {
        matches!(self, RunMode::Human)
    }

    /// The stdout payload of a finished run.
    ///
    /// The human and quiet surfaces print the answer the Lead persisted. The
    /// machine surface prints the typed run result — the whole answer, the
    /// completed tasks and the exact artifact digests, never an abbreviation.
    /// This is the one place the machine stdout contract changes.
    pub fn payload(self, run: &RunJson) -> Result<String, String> {
        match self {
            RunMode::Machine => json::encode(run),
            RunMode::Human | RunMode::Quiet => Ok(run.answer.clone()),
        }
    }
}

/// The human rendering of one run's lifecycle.
///
/// One instance is shared by the whole run, so `emit` is called concurrently
/// from parallel scheduler tasks. Every line is written whole under the sink's
/// lock, so two tasks can never interleave bytes inside a line.
pub struct HumanRunEventSink {
    mode: RunMode,
    started_at: Instant,
    state: Mutex<State>,
}

struct State {
    writer: Box<dyn Write + Send>,
    /// The root task id, once the durable root exists. A failure before this is
    /// known has no state to preserve, which is what the failure rendering
    /// keys on.
    run_id: Option<u64>,
    /// Every artifact the run recorded, in lifecycle order.
    artifacts: Vec<ArtifactNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtifactNotice {
    task_id: u64,
    path: String,
    sha256: String,
}

impl HumanRunEventSink {
    /// A sink that renders on stderr.
    pub fn stderr(mode: RunMode) -> Self {
        Self::new(mode, Box::new(io::stderr()))
    }

    /// A sink over an arbitrary writer, so the line serialization can be
    /// exercised without a terminal.
    pub fn new(mode: RunMode, writer: Box<dyn Write + Send>) -> Self {
        Self {
            mode,
            started_at: Instant::now(),
            state: Mutex::new(State {
                writer,
                run_id: None,
                artifacts: Vec::new(),
            }),
        }
    }

    /// The root task id, once the sink has seen the run start.
    pub fn run_id(&self) -> Option<u64> {
        self.locked().run_id
    }

    /// The first line of a run, written before the runner can know the root.
    ///
    /// Resolving the Lead and building the brain happen before the durable root
    /// exists, and a real external runtime is not instantaneous; the operator
    /// sees the objective immediately instead of a silent command.
    pub fn starting(&self, objective: &str) {
        if !self.mode.progress() {
            return;
        }
        self.locked()
            .write(&line("run", "starting", &bounded_line(objective)));
    }

    /// The end of a successful run: the artifacts it recorded, then the
    /// commands that look the run up again.
    pub fn finish(&self, run_id: u64) {
        if !self.mode.progress() {
            return;
        }
        let mut state = self.locked();
        let mut lines: Vec<String> = Vec::new();
        if !state.artifacts.is_empty() {
            lines.push(String::new());
            lines.push("artifacts".into());
            lines.extend(state.artifacts.iter().map(artifact_line));
        }
        lines.push(String::new());
        lines.push("next".into());
        lines.push(format!("  am status {run_id}"));
        lines.push(format!("  am final {run_id}"));
        lines.push("  am tui".into());
        for line in &lines {
            state.write(line);
        }
    }

    /// A poisoned lock is not a reason to lose the rest of a run's rendering:
    /// the presentation layer never panics and never aborts a run.
    fn locked(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl RunEventSink for HumanRunEventSink {
    fn emit(&self, event: &RunEvent) {
        let mut state = self.locked();
        state.observe(event);
        if !self.mode.progress() {
            return;
        }
        state.write(&progress_line(event, self.started_at.elapsed()));
    }
}

impl State {
    /// The durable facts the rendering keeps: the run id, and the artifacts the
    /// run recorded.
    fn observe(&mut self, event: &RunEvent) {
        match event {
            RunEvent::RunStarted { root_task_id, .. }
            | RunEvent::LeadRoundStarted { root_task_id, .. }
            | RunEvent::RunCompleted { root_task_id, .. }
            | RunEvent::RunFailed { root_task_id, .. }
            | RunEvent::RunResumed { root_task_id } => self.run_id = Some(*root_task_id),
            RunEvent::ArtifactRecorded {
                task_id,
                path,
                sha256,
            } => {
                let notice = ArtifactNotice {
                    task_id: *task_id,
                    path: path.clone(),
                    sha256: sha256.clone(),
                };
                if !self.artifacts.contains(&notice) {
                    self.artifacts.push(notice);
                }
            }
            _ => {}
        }
    }

    /// One complete line, under the sink's lock. Write errors are ignored: the
    /// terminal is not the run's business.
    fn write(&mut self, text: &str) {
        let _ = self.writer.write_all(text.as_bytes());
        let _ = self.writer.write_all(b"\n");
        let _ = self.writer.flush();
    }
}

/// The stderr text for a failed run.
///
/// In [`RunMode::Human`] the sink has already announced `run #N  failed`, so
/// only the diagnosis follows; the quieter surfaces print the headline
/// themselves. A failure that never reached a root left no durable state, so it
/// points at the readiness check rather than at the board.
pub fn failure_payload(mode: RunMode, run_id: Option<u64>, error: &str) -> String {
    let Some(run_id) = run_id else {
        return "Run could not start.\n  am doctor".to_string();
    };
    let headline = match mode {
        RunMode::Human => String::new(),
        RunMode::Quiet | RunMode::Machine => format!("run #{run_id}  failed\n\n"),
    };
    format!(
        "{headline}Reason\n  {reason}\n\nState was preserved.\n  am status {run_id}",
        reason = single_line(error)
    )
}

/// One progress line for one lifecycle event.
fn progress_line(event: &RunEvent, elapsed: Duration) -> String {
    match event {
        RunEvent::RunStarted {
            root_task_id,
            lead_agent,
        } => run_line(*root_task_id, "started", &format!("lead={lead_agent}")),
        RunEvent::LeadRoundStarted { round, phase, .. } => {
            line("lead", phase_text(*phase), &format!("round={}", round + 1))
        }
        RunEvent::TaskDelegated {
            task_id,
            kind,
            requested_target,
            ..
        } => line(
            &format!("task #{task_id}"),
            "delegated",
            &format!(
                "{} -> {}",
                kind.as_str(),
                requested_target.as_deref().unwrap_or("auto")
            ),
        ),
        RunEvent::AttemptStarted {
            task_id,
            attempt,
            agent_id,
        } => line(
            agent_id,
            "running",
            &format!("task #{task_id} \u{b7} attempt {attempt}"),
        ),
        RunEvent::AttemptFailed {
            task_id,
            attempt,
            agent_id,
            error,
        } => line(
            agent_id,
            "failed",
            &format!(
                "task #{task_id} \u{b7} attempt {attempt} \u{b7} {}",
                bounded_line(error)
            ),
        ),
        RunEvent::TaskSucceeded {
            task_id, agent_id, ..
        } => line(agent_id, "completed", &format!("task #{task_id}")),
        RunEvent::ArtifactRecorded { path, .. } => line("", "artifact", &bounded_line(path)),
        RunEvent::TaskFailed { task_id, error } => {
            line(&format!("task #{task_id}"), "failed", &bounded_line(error))
        }
        RunEvent::RunCompleted { root_task_id, .. } => {
            run_line(*root_task_id, "complete", &duration_text(elapsed))
        }
        // The diagnosis follows this line on stderr, so the event ends the
        // block it heads.
        RunEvent::RunFailed { root_task_id, .. } => {
            format!("{}\n", run_line(*root_task_id, "failed", ""))
        }
        RunEvent::RunResumed { root_task_id } => run_line(*root_task_id, "resumed", ""),
    }
}

fn run_line(root_task_id: u64, status: &str, detail: &str) -> String {
    line(&format!("run #{root_task_id}"), status, detail)
}

fn phase_text(phase: LeadPhase) -> &'static str {
    match phase {
        LeadPhase::Planning => "planning",
        LeadPhase::Reviewing => "reviewing",
    }
}

/// One progress line: the actor, the status, and the detail, in fixed columns.
fn line(actor: &str, status: &str, detail: &str) -> String {
    format!(
        "{actor:<actor_width$}{status:<status_width$}{detail}",
        actor_width = ACTOR_WIDTH,
        status_width = STATUS_WIDTH
    )
    .trim_end()
    .to_string()
}

/// One recorded artifact in the closing section: what it is, which task
/// produced it, and a digest short enough to compare at a glance.
fn artifact_line(notice: &ArtifactNotice) -> String {
    format!(
        "  {}  task #{}  sha256 {}",
        bounded_line(&notice.path),
        notice.task_id,
        abbreviated_digest(&notice.sha256)
    )
}

/// A digest as a human reads it: the first eight hex characters, marked as an
/// abbreviation. A digest too short to abbreviate is shown whole.
fn abbreviated_digest(sha256: &str) -> String {
    let head: String = sha256.chars().take(8).collect();
    if head.chars().count() < sha256.chars().count() {
        format!("{head}\u{2026}")
    } else {
        head
    }
}

/// A duration reduced to the chrome: sub-minute runs read in tenths of a
/// second, longer runs in minutes.
fn duration_text(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{}m{:02}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
    }
}

/// Event text as one line: the event contract's byte bound, then the whitespace
/// of a multi-line objective or error collapsed to single spaces, so no event
/// can ever break the line discipline of the progress stream.
fn single_line(text: &str) -> String {
    bounded_event_text(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// [`single_line`] cut to the width one progress line carries.
fn bounded_line(text: &str) -> String {
    cut(&single_line(text), MAX_LINE_TEXT_BYTES)
}

/// Cut to `max` bytes on a character boundary and mark the cut.
fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use agentmosaic_team::{RunEvent, RunEventSink, TaskKind};

    use super::{
        abbreviated_digest, failure_payload, HumanRunEventSink, RunMode, ACTOR_WIDTH, STATUS_WIDTH,
    };

    /// A writer every test can read back, and that the sink can own.
    #[derive(Clone, Default)]
    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl SharedBuffer {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for SharedBuffer {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn sink(mode: RunMode, buffer: &SharedBuffer) -> Arc<HumanRunEventSink> {
        Arc::new(HumanRunEventSink::new(mode, Box::new(buffer.clone())))
    }

    fn run_started() -> RunEvent {
        RunEvent::RunStarted {
            root_task_id: 4,
            lead_agent: "lead".into(),
        }
    }

    fn artifact() -> RunEvent {
        RunEvent::ArtifactRecorded {
            task_id: 5,
            path: "result.txt".into(),
            sha256: "4297addc00112233445566778899aabbccddeeff00112233445566778899aabb".into(),
        }
    }

    /// The three lifecycle layers a run reports: the run chrome, the Lead's
    /// rounds, and the worker's task.
    #[test]
    fn the_lifecycle_renders_as_the_three_layers() {
        let buffer = SharedBuffer::default();
        let sink = sink(RunMode::Human, &buffer);
        sink.starting("deliver the objective");
        sink.emit(&run_started());
        sink.emit(&RunEvent::LeadRoundStarted {
            root_task_id: 4,
            round: 0,
            phase: agentmosaic_team::LeadPhase::Planning,
        });
        sink.emit(&RunEvent::TaskDelegated {
            root_task_id: 4,
            task_id: 5,
            kind: TaskKind::Bulk,
            requested_target: Some("worker".into()),
            objective: "produce the worker result".into(),
        });
        sink.emit(&RunEvent::AttemptStarted {
            task_id: 5,
            attempt: 1,
            agent_id: "worker".into(),
        });
        sink.emit(&artifact());
        sink.emit(&RunEvent::TaskSucceeded {
            task_id: 5,
            agent_id: "worker".into(),
            artifact_count: 1,
        });
        sink.emit(&RunEvent::LeadRoundStarted {
            root_task_id: 4,
            round: 1,
            phase: agentmosaic_team::LeadPhase::Reviewing,
        });
        sink.emit(&RunEvent::RunCompleted {
            root_task_id: 4,
            selected_task_ids: vec![5],
            artifact_count: 1,
        });
        sink.finish(4);

        let text = buffer.text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "run     starting  deliver the objective");
        assert_eq!(lines[1], "run #4  started   lead=lead");
        assert_eq!(lines[2], "lead    planning  round=1");
        assert_eq!(lines[3], "task #5 delegated bulk -> worker");
        assert_eq!(lines[4], "worker  running   task #5 \u{b7} attempt 1");
        assert_eq!(lines[5], "        artifact  result.txt");
        assert_eq!(lines[6], "worker  completed task #5");
        assert_eq!(lines[7], "lead    reviewing round=2");
        assert!(lines[8].starts_with("run #4  complete  "), "{text}");
        assert!(lines[8].ends_with('s'), "{text}");
        assert!(text.contains("\nartifacts\n  result.txt  task #5  sha256 4297addc\u{2026}\n"));
        assert!(text.contains("\nnext\n  am status 4\n  am final 4\n  am tui\n"));
    }

    // A long objective is bounded, and a multi-line one still occupies one
    // line: the progress stream is line-oriented.
    #[test]
    fn event_text_is_bounded_and_kept_on_one_line() {
        let buffer = SharedBuffer::default();
        let sink = sink(RunMode::Human, &buffer);
        sink.starting(&format!("first\nsecond {}", "x".repeat(500)));
        sink.emit(&RunEvent::TaskFailed {
            task_id: 2,
            error: "a\nb".into(),
        });

        let text = buffer.text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(
            lines[0].starts_with("run     starting  first second "),
            "{text}"
        );
        assert!(lines[0].ends_with("..."), "{text}");
        assert!(lines[0].len() < 120, "{text}");
        assert_eq!(lines[1], "task #2 failed    a b");
    }

    // The quieter surfaces write no routine progress at all; the answer is the
    // caller's payload, not a line here.
    #[test]
    fn quiet_and_machine_modes_write_no_progress() {
        for mode in [RunMode::Quiet, RunMode::Machine] {
            let buffer = SharedBuffer::default();
            let sink = sink(mode, &buffer);
            sink.starting("deliver the objective");
            sink.emit(&run_started());
            sink.emit(&artifact());
            sink.emit(&RunEvent::RunCompleted {
                root_task_id: 4,
                selected_task_ids: vec![5],
                artifact_count: 1,
            });
            sink.finish(4);
            assert_eq!(buffer.text(), "", "{mode:?} wrote progress");
            assert_eq!(sink.run_id(), Some(4));
        }
    }

    // The sink remembers the run id from the root-carrying events, so a failure
    // after the root exists can point at the durable state.
    #[test]
    fn the_run_id_is_remembered_from_the_root() {
        let buffer = SharedBuffer::default();
        let quiet = sink(RunMode::Machine, &buffer);
        assert_eq!(quiet.run_id(), None);
        quiet.emit(&run_started());
        assert_eq!(quiet.run_id(), Some(4));
    }

    #[test]
    fn the_digest_is_abbreviated_not_shortened_silently() {
        assert_eq!(
            abbreviated_digest("4297addc00112233445566778899aabbccddeeff00112233445566778899aabb"),
            "4297addc\u{2026}"
        );
        assert_eq!(abbreviated_digest("abc"), "abc");
    }

    // A failure after the root exists names the run, the human reason, and the
    // command that reads the preserved state back. The human surface has
    // already printed the headline line.
    #[test]
    fn a_post_root_failure_names_the_run_and_the_next_command() {
        let human = failure_payload(
            RunMode::Human,
            Some(4),
            "the lead run failed: a real reason",
        );
        assert_eq!(
            human,
            "Reason\n  the lead run failed: a real reason\n\nState was preserved.\n  am status 4"
        );
        assert!(!human.starts_with("run #4"), "{human}");
        let quiet = failure_payload(
            RunMode::Quiet,
            Some(4),
            "the lead run failed: a real reason",
        );
        assert!(quiet.starts_with("run #4  failed\n\nReason\n"), "{quiet}");
        assert!(quiet.ends_with("  am status 4"), "{quiet}");
        let machine = failure_payload(RunMode::Machine, Some(4), "the lead run failed");
        assert!(
            machine.starts_with("run #4  failed\n\nReason\n"),
            "{machine}"
        );
    }

    // A failure with no durable root has no state to preserve: it points at the
    // one command that says what is not ready.
    #[test]
    fn a_pre_root_failure_points_at_the_readiness_check() {
        for mode in [RunMode::Human, RunMode::Quiet, RunMode::Machine] {
            assert_eq!(
                failure_payload(mode, None, "no reasoner agent is registered"),
                "Run could not start.\n  am doctor"
            );
        }
    }

    // A multi-line reason stays one indented line, and a very long one is
    // carried whole up to the document bound rather than dropped.
    #[test]
    fn the_reason_is_one_bounded_line() {
        let reason = failure_payload(RunMode::Human, Some(4), "first\n  second");
        assert!(reason.contains("  first second\n"), "{reason}");
        let long = failure_payload(RunMode::Human, Some(4), &"e".repeat(2_000));
        let line = long
            .lines()
            .find(|line| line.starts_with("  e"))
            .expect("a reason line");
        assert_eq!(line.len(), 2 + agentmosaic_team::MAX_EVENT_TEXT_BYTES);
    }

    // Presentation never panics: a writer that always fails is a silent sink,
    // not a failed run.
    #[test]
    fn a_failing_writer_never_panics_and_never_panics_the_run() {
        struct Failing;

        impl std::io::Write for Failing {
            fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("no terminal"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("no terminal"))
            }
        }

        let sink = HumanRunEventSink::new(RunMode::Human, Box::new(Failing));
        sink.starting("deliver the objective");
        sink.emit(&run_started());
        sink.emit(&RunEvent::RunFailed {
            root_task_id: 4,
            error: "a reason".into(),
        });
        sink.finish(4);
        assert_eq!(sink.run_id(), Some(4));
    }

    // Parallel scheduler tasks share one sink. Every line must be complete and
    // well formed: no partial line, and no two events interleaved inside one
    // line.
    #[test]
    fn concurrent_events_never_interleave_inside_a_line() {
        const THREADS: usize = 8;
        const EVENTS_PER_THREAD: usize = 250;

        let buffer = SharedBuffer::default();
        let sink = sink(RunMode::Human, &buffer);
        let mut threads = Vec::new();
        for thread in 0..THREADS {
            let sink = Arc::clone(&sink);
            threads.push(std::thread::spawn(move || {
                for index in 0..EVENTS_PER_THREAD {
                    sink.emit(&event(thread * EVENTS_PER_THREAD + index));
                }
            }));
        }
        for thread in threads {
            thread.join().expect("a rendering thread");
        }

        let text = buffer.text();
        assert!(text.ends_with('\n'), "the last line is complete");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), THREADS * EVENTS_PER_THREAD, "{text}");
        for line in &lines {
            assert!(well_formed(line), "a torn or interleaved line: {line:?}");
        }
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("artifact-"))
                .count(),
            (0..THREADS * EVENTS_PER_THREAD)
                .filter(|index| matches!(event(*index), RunEvent::ArtifactRecorded { .. }))
                .count(),
            "every recorded artifact has exactly one line"
        );
    }

    /// A deterministic event per index, across the parallel and the
    /// root-carrying variants.
    fn event(index: usize) -> RunEvent {
        let agent = format!("worker-{}", index % 5);
        match index % 4 {
            0 => RunEvent::AttemptStarted {
                task_id: index as u64,
                attempt: 1,
                agent_id: agent,
            },
            1 => RunEvent::TaskSucceeded {
                task_id: index as u64,
                agent_id: agent,
                artifact_count: 0,
            },
            2 => RunEvent::ArtifactRecorded {
                task_id: index as u64,
                path: format!("artifact-{}.txt", index % 3),
                sha256: "a".repeat(64),
            },
            _ => RunEvent::LeadRoundStarted {
                root_task_id: 1,
                round: index as u32,
                phase: agentmosaic_team::LeadPhase::Reviewing,
            },
        }
    }

    /// The exact shape one complete progress line has: the actor column, the
    /// status column, and a detail that follows from that status.
    fn well_formed(line: &str) -> bool {
        let Some(actor) = line.get(..ACTOR_WIDTH) else {
            return false;
        };
        let Some(rest) = line.get(ACTOR_WIDTH..) else {
            return false;
        };
        let Some(status) = rest.get(..STATUS_WIDTH) else {
            return false;
        };
        let detail = &rest[STATUS_WIDTH..];
        match (actor.trim_end(), status.trim_end()) {
            ("lead", "reviewing") => detail.starts_with("round="),
            (actor, "running") => {
                actor.starts_with("worker-")
                    && detail.starts_with("task #")
                    && detail.ends_with("\u{b7} attempt 1")
            }
            (actor, "completed") => actor.starts_with("worker-") && detail.starts_with("task #"),
            (actor, "artifact") => {
                actor.is_empty() && detail.starts_with("artifact-") && detail.ends_with(".txt")
            }
            _ => false,
        }
    }
}
