//! The team lifecycle as a narrow, non-authoritative notification projection.
//!
//! The durable SQLite `TaskBoard` is the only truth. A [`RunEvent`] is a
//! best-effort notification a presentation layer may render: it is never
//! persisted, never read back to decide anything, and never gates a lifecycle
//! transition. Every event that corresponds to durable state is emitted *after*
//! the durable mutation succeeded and *outside* the board lock, so a sink can
//! only ever observe state the board already holds.
//!
//! The contract lives here, not in the CLI, because both the `Scheduler` (task
//! and attempt lifecycle) and the `Lead` (round lifecycle) own transitions.
//!
//! Deliberately absent from every event: hidden chain-of-thought, raw Codex
//! reasoning, the Lead prompt/context JSON, a raw model reply before the
//! decision contract validated it, raw ACP protocol transcripts, auth or token
//! material, environment secrets, native thread/session ids, and the driver
//! argv.

use crate::registry::TaskKind;

/// The byte bound every text an event carries is truncated to, so no event can
/// ship an unbounded string.
pub const MAX_EVENT_TEXT_BYTES: usize = 512;

/// Bound event text to [`MAX_EVENT_TEXT_BYTES`] without splitting a UTF-8
/// character. Every objective and error an event carries goes through this.
pub fn bounded_event_text(text: &str) -> String {
    if text.len() <= MAX_EVENT_TEXT_BYTES {
        return text.to_string();
    }
    let mut end = MAX_EVENT_TEXT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// The phase of a Lead round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadPhase {
    /// The first round: the Lead plans what to delegate.
    Planning,
    /// Every round after the first: the Lead reviews what came back and drives
    /// the run toward its final answer.
    Reviewing,
}

impl LeadPhase {
    /// The phase of `round`, derived from the round number, which is all the
    /// Lead loop genuinely knows before it has asked the brain to decide.
    pub fn for_round(round: u32) -> Self {
        if round == 0 {
            LeadPhase::Planning
        } else {
            LeadPhase::Reviewing
        }
    }
}

/// One step of the team lifecycle, as a presentation layer may render it.
///
/// The board stays canonical: an event carries ids, bounded text, and counts
/// that a rendering layer can look up durably, never a transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    RunStarted {
        root_task_id: u64,
        lead_agent: String,
    },
    LeadRoundStarted {
        root_task_id: u64,
        round: u32,
        phase: LeadPhase,
    },
    TaskDelegated {
        root_task_id: u64,
        task_id: u64,
        kind: TaskKind,
        requested_target: Option<String>,
        objective: String,
    },
    AttemptStarted {
        task_id: u64,
        attempt: u32,
        agent_id: String,
    },
    AttemptFailed {
        task_id: u64,
        attempt: u32,
        agent_id: String,
        error: String,
    },
    TaskSucceeded {
        task_id: u64,
        agent_id: String,
        /// The artifacts committed with this result, as the board now holds
        /// them.
        artifact_count: usize,
    },
    TaskFailed {
        task_id: u64,
        error: String,
    },
    ArtifactRecorded {
        task_id: u64,
        path: String,
        sha256: String,
    },
    RunCompleted {
        root_task_id: u64,
        /// The exact completed tasks the Lead selected to ground the answer.
        selected_task_ids: Vec<u64>,
        /// The artifacts the Lead selected for the final answer.
        artifact_count: usize,
    },
    RunFailed {
        root_task_id: u64,
        error: String,
    },
    RunResumed {
        root_task_id: u64,
    },
}

/// Where a presentation layer receives the lifecycle projection.
///
/// `emit` is infallible by construction: a presentation failure is not a
/// scheduler or Lead error, so it can never corrupt or cancel a run. One sink
/// instance is shared as an `Arc` by every spawned task, so an implementation
/// must tolerate concurrent `emit` calls.
pub trait RunEventSink: Send + Sync {
    fn emit(&self, event: &RunEvent);
}

/// The default sink: no observer, no cost, no behavior change.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopRunEventSink;

impl RunEventSink for NoopRunEventSink {
    fn emit(&self, _event: &RunEvent) {}
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use crate::board::{TaskBoard, TaskStatus};
    use crate::lead::{Lead, LeadBrain, LeadBrainError, LeadContext, LeadDecision, TeamResult};
    use crate::registry::{AgentDriver, AgentTask, AgentTaskResult, TaskKind};
    use crate::scheduler::{Scheduler, TaskSpec};
    use crate::testutil::{err_driver, ok_driver, trio_registry, MemBoard};

    use super::{
        bounded_event_text, LeadPhase, NoopRunEventSink, RunEvent, RunEventSink,
        MAX_EVENT_TEXT_BYTES,
    };

    /// Records every emitted event, in emission order.
    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<Vec<RunEvent>>,
    }

    impl RecordingSink {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        fn events(&self) -> Vec<RunEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    impl RunEventSink for RecordingSink {
        fn emit(&self, event: &RunEvent) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    fn occurrences(events: &[RunEvent], matches: impl Fn(&RunEvent) -> bool) -> usize {
        events.iter().filter(|event| matches(event)).count()
    }

    fn bulk_spec(objective: &str, target: &str) -> TaskSpec {
        TaskSpec {
            objective: objective.into(),
            kind: TaskKind::Bulk,
            target: Some(target.into()),
            parent: None,
            context: Vec::new(),
        }
    }

    /// One delegated task, one follow-up on its result, then completion.
    struct ScriptedBrain;

    #[async_trait]
    impl LeadBrain for ScriptedBrain {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            Ok(match ctx.round {
                0 => LeadDecision::Delegate(vec![bulk_spec("summarize data", "worker-a")]),
                1 => LeadDecision::FollowUp(vec![TaskSpec {
                    objective: "refine the summary".into(),
                    kind: TaskKind::Reasoning,
                    target: Some("reasoner-a".into()),
                    parent: None,
                    context: Vec::new(),
                }]),
                _ => LeadDecision::Complete(TeamResult {
                    answer: "done".into(),
                    task_refs: ctx.results.iter().map(|(id, _)| *id).collect(),
                    artifact_refs: Vec::new(),
                }),
            })
        }
    }

    // The Lead loop's projection: one round event per round, one delegated and
    // one succeeded event per completed task, in lifecycle order.
    #[tokio::test]
    async fn a_delegate_follow_up_and_complete_run_reports_each_round_and_task() {
        let sink = RecordingSink::new();
        let drivers = BTreeMap::from([
            ("worker-a".to_string(), ok_driver("data summary")),
            ("reasoner-a".to_string(), ok_driver("refined insight")),
        ]);
        let scheduler = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1)
            .with_sink(sink.clone());
        let mut lead = Lead::new(Box::new(ScriptedBrain), scheduler, 5, 10, "lead");
        lead.run("analyze the dataset")
            .await
            .expect("run completes");

        assert_eq!(
            sink.events(),
            vec![
                RunEvent::LeadRoundStarted {
                    root_task_id: 1,
                    round: 0,
                    phase: LeadPhase::Planning,
                },
                RunEvent::TaskDelegated {
                    root_task_id: 1,
                    task_id: 2,
                    kind: TaskKind::Bulk,
                    requested_target: Some("worker-a".into()),
                    objective: "summarize data".into(),
                },
                RunEvent::AttemptStarted {
                    task_id: 2,
                    attempt: 1,
                    agent_id: "worker-a".into(),
                },
                RunEvent::TaskSucceeded {
                    task_id: 2,
                    agent_id: "worker-a".into(),
                    artifact_count: 0,
                },
                RunEvent::LeadRoundStarted {
                    root_task_id: 1,
                    round: 1,
                    phase: LeadPhase::Reviewing,
                },
                RunEvent::TaskDelegated {
                    root_task_id: 1,
                    task_id: 3,
                    kind: TaskKind::Reasoning,
                    requested_target: Some("reasoner-a".into()),
                    objective: "refine the summary".into(),
                },
                RunEvent::AttemptStarted {
                    task_id: 3,
                    attempt: 1,
                    agent_id: "reasoner-a".into(),
                },
                RunEvent::TaskSucceeded {
                    task_id: 3,
                    agent_id: "reasoner-a".into(),
                    artifact_count: 0,
                },
                RunEvent::LeadRoundStarted {
                    root_task_id: 1,
                    round: 2,
                    phase: LeadPhase::Reviewing,
                },
            ]
        );
    }

    // A failing task reports the attempt failure first and the task-level
    // failure once every candidate is exhausted. A task scheduled without a
    // parent is its own root.
    #[tokio::test]
    async fn a_failing_task_reports_its_attempt_then_the_task_failure() {
        let sink = RecordingSink::new();
        let drivers = BTreeMap::from([("worker-a".to_string(), err_driver("worker-a is down"))]);
        let mut scheduler = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1)
            .with_sink(sink.clone());
        let results = scheduler
            .schedule(&[bulk_spec("job", "worker-a")])
            .await
            .expect("schedule returns the failure, it does not error");
        assert_eq!(
            results[0].result.as_ref().err().map(String::as_str),
            Some("worker-a is down")
        );

        assert_eq!(
            sink.events(),
            vec![
                RunEvent::TaskDelegated {
                    root_task_id: 1,
                    task_id: 1,
                    kind: TaskKind::Bulk,
                    requested_target: Some("worker-a".into()),
                    objective: "job".into(),
                },
                RunEvent::AttemptStarted {
                    task_id: 1,
                    attempt: 1,
                    agent_id: "worker-a".into(),
                },
                RunEvent::AttemptFailed {
                    task_id: 1,
                    attempt: 1,
                    agent_id: "worker-a".into(),
                    error: "worker-a is down".into(),
                },
                RunEvent::TaskFailed {
                    task_id: 1,
                    error: "worker-a is down".into(),
                },
            ]
        );
    }

    /// Returns only once two tasks have been in flight together, so `peak >= 2`
    /// proves real overlap rather than merely interleaved scheduling.
    struct OverlapDriver {
        in_flight: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl AgentDriver for OverlapDriver {
        async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(current, Ordering::SeqCst);
            for _ in 0..10_000 {
                if self.peak.load(Ordering::SeqCst) >= 2 {
                    break;
                }
                tokio::task::yield_now().await;
            }
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(AgentTaskResult {
                task_id: task.id,
                summary: "ok".into(),
                artifacts: Vec::new(),
                message: None,
            })
        }
    }

    // Several tasks across two agents with `max_concurrency = 2`: every event
    // arrives exactly once, with no loss and no duplication under real overlap.
    #[tokio::test]
    async fn concurrent_tasks_report_every_event_exactly_once() {
        let sink = RecordingSink::new();
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let driver = || {
            Arc::new(OverlapDriver {
                in_flight: in_flight.clone(),
                peak: peak.clone(),
            }) as Arc<dyn AgentDriver>
        };
        let drivers = BTreeMap::from([
            ("worker-a".to_string(), driver()),
            ("worker-b".to_string(), driver()),
        ]);
        let mut scheduler = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1)
            .with_sink(sink.clone());
        let specs = vec![
            bulk_spec("a1", "worker-a"),
            bulk_spec("b1", "worker-b"),
            bulk_spec("a2", "worker-a"),
            bulk_spec("b2", "worker-b"),
        ];
        let results = scheduler.schedule(&specs).await.expect("schedule");
        assert!(results.iter().all(|result| result.result.is_ok()));
        assert!(
            peak.load(Ordering::SeqCst) >= 2,
            "the tasks must actually overlap"
        );

        let events = sink.events();
        assert_eq!(
            occurrences(&events, |event| matches!(
                event,
                RunEvent::TaskDelegated { .. }
            )),
            4,
            "{events:?}"
        );
        assert_eq!(
            occurrences(&events, |event| matches!(
                event,
                RunEvent::AttemptStarted { .. }
            )),
            4,
            "{events:?}"
        );
        assert_eq!(
            occurrences(&events, |event| matches!(
                event,
                RunEvent::TaskSucceeded { .. }
            )),
            4,
            "{events:?}"
        );
        assert_eq!(
            occurrences(&events, |event| matches!(
                event,
                RunEvent::AttemptFailed { .. } | RunEvent::TaskFailed { .. }
            )),
            0,
            "{events:?}"
        );
        for (task_id, agent_id) in [
            (1, "worker-a"),
            (2, "worker-b"),
            (3, "worker-a"),
            (4, "worker-b"),
        ] {
            assert_eq!(
                occurrences(&events, |event| matches!(
                    event,
                    RunEvent::AttemptStarted { task_id: id, attempt: 1, agent_id: agent }
                        if *id == task_id && agent == agent_id
                )),
                1,
                "{events:?}"
            );
            assert_eq!(
                occurrences(&events, |event| matches!(
                    event,
                    RunEvent::TaskSucceeded { task_id: id, agent_id: agent, artifact_count: 0 }
                        if *id == task_id && agent == agent_id
                )),
                1,
                "{events:?}"
            );
            let started = events
                .iter()
                .position(|event| matches!(event, RunEvent::AttemptStarted { task_id: id, .. } if *id == task_id))
                .expect("started");
            let succeeded = events
                .iter()
                .position(|event| matches!(event, RunEvent::TaskSucceeded { task_id: id, .. } if *id == task_id))
                .expect("succeeded");
            assert!(started < succeeded, "{events:?}");
        }
    }

    // The default sink is the no-op, and a run without one behaves exactly as
    // it did before the projection existed.
    #[tokio::test]
    async fn the_default_sink_changes_nothing() {
        let drivers = || BTreeMap::from([("worker-a".to_string(), ok_driver("done"))]);
        let mut plain = Scheduler::new(trio_registry(), drivers(), MemBoard::default(), 1);
        let mut observed = Scheduler::new(trio_registry(), drivers(), MemBoard::default(), 1)
            .with_sink(RecordingSink::new());

        let plain_results = plain
            .schedule(&[bulk_spec("job", "worker-a")])
            .await
            .expect("schedule");
        let observed_results = observed
            .schedule(&[bulk_spec("job", "worker-a")])
            .await
            .expect("schedule");

        assert_eq!(plain_results[0].task_id, observed_results[0].task_id);
        assert_eq!(plain_results[0].attempts[0].status, TaskStatus::Succeeded);
        assert_eq!(
            plain_results[0].attempts[0].status,
            observed_results[0].attempts[0].status
        );
        assert_eq!(
            plain_results[0]
                .result
                .as_ref()
                .ok()
                .map(|r| r.summary.clone()),
            observed_results[0]
                .result
                .as_ref()
                .ok()
                .map(|r| r.summary.clone())
        );
        let board = plain.board().lock().unwrap();
        assert_eq!(
            board.task(1).unwrap().unwrap().status,
            TaskStatus::Succeeded
        );
        drop(board);

        // The default sink is always callable and never panics.
        NoopRunEventSink.emit(&RunEvent::RunResumed { root_task_id: 1 });
        plain.sink().emit(&RunEvent::RunResumed { root_task_id: 1 });
    }

    // An event never carries unbounded text: a long objective and a long driver
    // error are both truncated to the event bound.
    #[tokio::test]
    async fn event_text_is_bounded() {
        let sink = RecordingSink::new();
        let objective = "o".repeat(5_000);
        let error = "e".repeat(5_000);
        let drivers = BTreeMap::from([("worker-a".to_string(), err_driver(&error))]);
        let mut scheduler = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1)
            .with_sink(sink.clone());
        scheduler
            .schedule(&[bulk_spec(&objective, "worker-a")])
            .await
            .expect("schedule");

        let events = sink.events();
        match &events[0] {
            RunEvent::TaskDelegated { objective, .. } => {
                assert_eq!(objective.len(), MAX_EVENT_TEXT_BYTES)
            }
            other => panic!("unexpected first event: {other:?}"),
        }
        match &events[2] {
            RunEvent::AttemptFailed { error, .. } => {
                assert_eq!(error.len(), MAX_EVENT_TEXT_BYTES)
            }
            other => panic!("unexpected third event: {other:?}"),
        }
        match &events[3] {
            RunEvent::TaskFailed { error, .. } => assert_eq!(error.len(), MAX_EVENT_TEXT_BYTES),
            other => panic!("unexpected fourth event: {other:?}"),
        }
    }

    #[test]
    fn bounded_event_text_never_splits_a_character() {
        let bounded = bounded_event_text(&"☃".repeat(1_000));
        assert!(bounded.len() <= MAX_EVENT_TEXT_BYTES);
        assert_eq!(bounded.len(), 510, "510 bytes is the last 3-byte boundary");
        assert!(bounded.chars().all(|c| c == '☃'));
        assert_eq!(bounded_event_text("short"), "short");
    }

    #[test]
    fn the_lead_phase_follows_the_round_number() {
        assert_eq!(LeadPhase::for_round(0), LeadPhase::Planning);
        assert_eq!(LeadPhase::for_round(1), LeadPhase::Reviewing);
        assert_eq!(LeadPhase::for_round(9), LeadPhase::Reviewing);
    }
}
