//! Vendor-neutral observations emitted by external Agent runtimes.
//!
//! These values are deliberately separate from [`crate::RunEvent`]. A
//! `RuntimeEvent` describes what a foreign runtime reported; it never changes
//! canonical task state and is never copied wholesale into another Agent's
//! context.

use serde::{Deserialize, Serialize};

pub const MAX_RUNTIME_ID_BYTES: usize = 512;
pub const MAX_RUNTIME_SUMMARY_BYTES: usize = 4 * 1024;
pub const MAX_ASSISTANT_MESSAGE_BYTES: usize = 16 * 1024;
pub const MAX_COMMAND_BYTES: usize = 8 * 1024;
pub const MAX_PLAN_ITEM_BYTES: usize = 2 * 1024;
pub const MAX_PLAN_ITEMS: usize = 64;
pub const MAX_PERMISSION_OPTIONS: usize = 64;
pub const MAX_DURABLE_RUNTIME_PAYLOAD_BYTES: usize = 32 * 1024;

const TRUNCATION_MARKER: &str = "...[truncated]";

/// Persistence/visibility policy for normalized runtime observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEventPolicy {
    LiveOnly,
    Durable,
    PrivateDrop,
}

/// Inputs that are intentionally excluded before normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivateRuntimeInput {
    RawWireFrame,
    AuthenticationMaterial,
    EnvironmentDump,
    HiddenChainOfThought,
    AcpAgentThoughtChunk,
    ClaudeThinkingBlock,
}

impl PrivateRuntimeInput {
    pub fn policy(self) -> RuntimeEventPolicy {
        RuntimeEventPolicy::PrivateDrop
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFileChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePlanItem {
    pub text: String,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePermissionOption {
    pub option_id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum RuntimePermissionDecision {
    Denied,
    Allowed { option_id: Option<String> },
    Cancelled,
}

/// A stable, vendor-neutral runtime event. Serde's tagged representation is a
/// durable wire form; variants and their snake-case names require round-trip
/// tests before they change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeEvent {
    SessionStarted {
        native_session_id: String,
    },
    SessionResumed {
        native_session_id: String,
    },
    AssistantMessageDelta {
        text: String,
    },
    AssistantMessageCompleted {
        text: String,
    },
    ReasoningSummary {
        text: String,
    },
    PlanUpdated {
        items: Vec<RuntimePlanItem>,
    },
    ToolCallStarted {
        native_call_id: String,
        tool: String,
        input_summary: String,
    },
    ToolCallUpdated {
        native_call_id: String,
        status: String,
        output_summary: Option<String>,
    },
    ToolCallCompleted {
        native_call_id: String,
        tool: String,
        ok: bool,
        output_summary: String,
    },
    CommandStarted {
        native_call_id: String,
        command: String,
    },
    CommandCompleted {
        native_call_id: String,
        exit_code: Option<i32>,
        output_summary: String,
    },
    FileChanged {
        path: String,
        change: RuntimeFileChangeKind,
    },
    PermissionRequested {
        request_id: String,
        action: String,
        options: Vec<RuntimePermissionOption>,
    },
    PermissionResolved {
        request_id: String,
        decision: RuntimePermissionDecision,
    },
    SubagentStarted {
        native_id: String,
        parent_native_id: Option<String>,
    },
    SubagentCompleted {
        native_id: String,
        status: String,
    },
    UsageUpdated {
        input_tokens: Option<u64>,
        cached_input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        reasoning_tokens: Option<u64>,
        estimated_cost_usd: Option<f64>,
    },
    RuntimeWarning {
        code: Option<String>,
        message: String,
    },
    RuntimeError {
        code: Option<String>,
        message: String,
    },
    CheckpointCreated {
        native_session_id: Option<String>,
    },
}

impl RuntimeEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::SessionStarted { .. } => "session_started",
            Self::SessionResumed { .. } => "session_resumed",
            Self::AssistantMessageDelta { .. } => "assistant_message_delta",
            Self::AssistantMessageCompleted { .. } => "assistant_message_completed",
            Self::ReasoningSummary { .. } => "reasoning_summary",
            Self::PlanUpdated { .. } => "plan_updated",
            Self::ToolCallStarted { .. } => "tool_call_started",
            Self::ToolCallUpdated { .. } => "tool_call_updated",
            Self::ToolCallCompleted { .. } => "tool_call_completed",
            Self::CommandStarted { .. } => "command_started",
            Self::CommandCompleted { .. } => "command_completed",
            Self::FileChanged { .. } => "file_changed",
            Self::PermissionRequested { .. } => "permission_requested",
            Self::PermissionResolved { .. } => "permission_resolved",
            Self::SubagentStarted { .. } => "subagent_started",
            Self::SubagentCompleted { .. } => "subagent_completed",
            Self::UsageUpdated { .. } => "usage_updated",
            Self::RuntimeWarning { .. } => "runtime_warning",
            Self::RuntimeError { .. } => "runtime_error",
            Self::CheckpointCreated { .. } => "checkpoint_created",
        }
    }

    pub fn policy(&self) -> RuntimeEventPolicy {
        match self {
            Self::AssistantMessageDelta { .. }
            | Self::ToolCallUpdated { .. }
            | Self::CommandStarted { .. } => RuntimeEventPolicy::LiveOnly,
            _ => RuntimeEventPolicy::Durable,
        }
    }

    /// Apply deterministic UTF-8-safe field and collection bounds.
    pub fn bounded(self) -> Self {
        let mut event = match self {
            Self::SessionStarted { native_session_id } => Self::SessionStarted {
                native_session_id: truncate(&native_session_id, MAX_RUNTIME_ID_BYTES),
            },
            Self::SessionResumed { native_session_id } => Self::SessionResumed {
                native_session_id: truncate(&native_session_id, MAX_RUNTIME_ID_BYTES),
            },
            Self::AssistantMessageDelta { text } => Self::AssistantMessageDelta {
                text: truncate(&text, MAX_RUNTIME_SUMMARY_BYTES),
            },
            Self::AssistantMessageCompleted { text } => Self::AssistantMessageCompleted {
                text: truncate(&text, MAX_ASSISTANT_MESSAGE_BYTES),
            },
            Self::ReasoningSummary { text } => Self::ReasoningSummary {
                text: truncate(&text, MAX_RUNTIME_SUMMARY_BYTES),
            },
            Self::PlanUpdated { items } => Self::PlanUpdated {
                items: items
                    .into_iter()
                    .take(MAX_PLAN_ITEMS)
                    .map(|item| RuntimePlanItem {
                        text: truncate(&item.text, MAX_PLAN_ITEM_BYTES),
                        status: item
                            .status
                            .map(|status| truncate(&status, MAX_RUNTIME_ID_BYTES)),
                    })
                    .collect(),
            },
            Self::ToolCallStarted {
                native_call_id,
                tool,
                input_summary,
            } => Self::ToolCallStarted {
                native_call_id: truncate(&native_call_id, MAX_RUNTIME_ID_BYTES),
                tool: truncate(&tool, MAX_RUNTIME_ID_BYTES),
                input_summary: truncate(&input_summary, MAX_RUNTIME_SUMMARY_BYTES),
            },
            Self::ToolCallUpdated {
                native_call_id,
                status,
                output_summary,
            } => Self::ToolCallUpdated {
                native_call_id: truncate(&native_call_id, MAX_RUNTIME_ID_BYTES),
                status: truncate(&status, MAX_RUNTIME_ID_BYTES),
                output_summary: output_summary
                    .map(|output| truncate(&output, MAX_RUNTIME_SUMMARY_BYTES)),
            },
            Self::ToolCallCompleted {
                native_call_id,
                tool,
                ok,
                output_summary,
            } => Self::ToolCallCompleted {
                native_call_id: truncate(&native_call_id, MAX_RUNTIME_ID_BYTES),
                tool: truncate(&tool, MAX_RUNTIME_ID_BYTES),
                ok,
                output_summary: truncate(&output_summary, MAX_RUNTIME_SUMMARY_BYTES),
            },
            Self::CommandStarted {
                native_call_id,
                command,
            } => Self::CommandStarted {
                native_call_id: truncate(&native_call_id, MAX_RUNTIME_ID_BYTES),
                command: truncate(&command, MAX_COMMAND_BYTES),
            },
            Self::CommandCompleted {
                native_call_id,
                exit_code,
                output_summary,
            } => Self::CommandCompleted {
                native_call_id: truncate(&native_call_id, MAX_RUNTIME_ID_BYTES),
                exit_code,
                output_summary: truncate(&output_summary, MAX_RUNTIME_SUMMARY_BYTES),
            },
            Self::FileChanged { path, change } => Self::FileChanged {
                path: truncate(&path, MAX_RUNTIME_SUMMARY_BYTES),
                change,
            },
            Self::PermissionRequested {
                request_id,
                action,
                options,
            } => Self::PermissionRequested {
                request_id: truncate(&request_id, MAX_RUNTIME_ID_BYTES),
                action: truncate(&action, MAX_RUNTIME_SUMMARY_BYTES),
                options: options
                    .into_iter()
                    .take(MAX_PERMISSION_OPTIONS)
                    .map(|option| RuntimePermissionOption {
                        option_id: truncate(&option.option_id, MAX_RUNTIME_ID_BYTES),
                        label: truncate(&option.label, MAX_RUNTIME_ID_BYTES),
                    })
                    .collect(),
            },
            Self::PermissionResolved {
                request_id,
                decision,
            } => Self::PermissionResolved {
                request_id: truncate(&request_id, MAX_RUNTIME_ID_BYTES),
                decision: match decision {
                    RuntimePermissionDecision::Allowed { option_id } => {
                        RuntimePermissionDecision::Allowed {
                            option_id: option_id
                                .map(|value| truncate(&value, MAX_RUNTIME_ID_BYTES)),
                        }
                    }
                    other => other,
                },
            },
            Self::SubagentStarted {
                native_id,
                parent_native_id,
            } => Self::SubagentStarted {
                native_id: truncate(&native_id, MAX_RUNTIME_ID_BYTES),
                parent_native_id: parent_native_id
                    .map(|value| truncate(&value, MAX_RUNTIME_ID_BYTES)),
            },
            Self::SubagentCompleted { native_id, status } => Self::SubagentCompleted {
                native_id: truncate(&native_id, MAX_RUNTIME_ID_BYTES),
                status: truncate(&status, MAX_RUNTIME_ID_BYTES),
            },
            Self::UsageUpdated {
                input_tokens,
                cached_input_tokens,
                output_tokens,
                reasoning_tokens,
                estimated_cost_usd,
            } => Self::UsageUpdated {
                input_tokens,
                cached_input_tokens,
                output_tokens,
                reasoning_tokens,
                estimated_cost_usd: estimated_cost_usd.filter(|value| value.is_finite()),
            },
            Self::RuntimeWarning { code, message } => Self::RuntimeWarning {
                code: code.map(|value| truncate(&value, MAX_RUNTIME_ID_BYTES)),
                message: truncate(&message, MAX_RUNTIME_SUMMARY_BYTES),
            },
            Self::RuntimeError { code, message } => Self::RuntimeError {
                code: code.map(|value| truncate(&value, MAX_RUNTIME_ID_BYTES)),
                message: truncate(&message, MAX_RUNTIME_SUMMARY_BYTES),
            },
            Self::CheckpointCreated { native_session_id } => Self::CheckpointCreated {
                native_session_id: native_session_id
                    .map(|value| truncate(&value, MAX_RUNTIME_ID_BYTES)),
            },
        };

        // Collection variants can exceed the aggregate JSON ceiling even
        // after per-item bounds. Drop only the deterministic tail until the
        // normalized payload fits.
        while serde_json::to_vec(&event)
            .map(|json| json.len() > MAX_DURABLE_RUNTIME_PAYLOAD_BYTES)
            .unwrap_or(false)
        {
            let shortened = match &mut event {
                Self::PlanUpdated { items } => items.pop().is_some(),
                Self::PermissionRequested { options, .. } => options.pop().is_some(),
                _ => false,
            };
            if !shortened {
                break;
            }
        }
        event
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEventRecord {
    pub task_id: u64,
    pub attempt: u32,
    pub agent_id: String,
    pub runtime_name: Option<String>,
    pub native_session_id: Option<String>,
    pub event: RuntimeEvent,
}

impl RuntimeEventRecord {
    pub fn bounded(self) -> Self {
        let mut record = Self {
            task_id: self.task_id,
            attempt: self.attempt,
            agent_id: truncate(&self.agent_id, MAX_RUNTIME_ID_BYTES),
            runtime_name: self
                .runtime_name
                .map(|value| truncate(&value, MAX_RUNTIME_ID_BYTES)),
            native_session_id: self
                .native_session_id
                .map(|value| truncate(&value, MAX_RUNTIME_ID_BYTES)),
            event: self.event.bounded(),
        };
        while serde_json::to_vec(&record)
            .map(|json| json.len() > MAX_DURABLE_RUNTIME_PAYLOAD_BYTES)
            .unwrap_or(false)
        {
            let shortened = match &mut record.event {
                RuntimeEvent::PlanUpdated { items } => items.pop().is_some(),
                RuntimeEvent::PermissionRequested { options, .. } => options.pop().is_some(),
                _ => false,
            };
            if !shortened {
                break;
            }
        }
        record
    }
}

fn truncate(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let payload_max = max_bytes.saturating_sub(TRUNCATION_MARKER.len());
    let mut end = payload_max.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &text[..end], TRUNCATION_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variants() -> Vec<RuntimeEvent> {
        vec![
            RuntimeEvent::SessionStarted {
                native_session_id: "s".into(),
            },
            RuntimeEvent::SessionResumed {
                native_session_id: "s".into(),
            },
            RuntimeEvent::AssistantMessageDelta {
                text: "part".into(),
            },
            RuntimeEvent::AssistantMessageCompleted {
                text: "done".into(),
            },
            RuntimeEvent::ReasoningSummary {
                text: "summary".into(),
            },
            RuntimeEvent::PlanUpdated {
                items: vec![RuntimePlanItem {
                    text: "step".into(),
                    status: Some("pending".into()),
                }],
            },
            RuntimeEvent::ToolCallStarted {
                native_call_id: "c".into(),
                tool: "read".into(),
                input_summary: "input".into(),
            },
            RuntimeEvent::ToolCallUpdated {
                native_call_id: "c".into(),
                status: "running".into(),
                output_summary: Some("partial".into()),
            },
            RuntimeEvent::ToolCallCompleted {
                native_call_id: "c".into(),
                tool: "read".into(),
                ok: true,
                output_summary: "output".into(),
            },
            RuntimeEvent::CommandStarted {
                native_call_id: "c".into(),
                command: "cargo test".into(),
            },
            RuntimeEvent::CommandCompleted {
                native_call_id: "c".into(),
                exit_code: Some(0),
                output_summary: "ok".into(),
            },
            RuntimeEvent::FileChanged {
                path: "src/lib.rs".into(),
                change: RuntimeFileChangeKind::Modified,
            },
            RuntimeEvent::PermissionRequested {
                request_id: "p".into(),
                action: "write".into(),
                options: vec![RuntimePermissionOption {
                    option_id: "deny".into(),
                    label: "Deny".into(),
                }],
            },
            RuntimeEvent::PermissionResolved {
                request_id: "p".into(),
                decision: RuntimePermissionDecision::Denied,
            },
            RuntimeEvent::SubagentStarted {
                native_id: "a".into(),
                parent_native_id: None,
            },
            RuntimeEvent::SubagentCompleted {
                native_id: "a".into(),
                status: "completed".into(),
            },
            RuntimeEvent::UsageUpdated {
                input_tokens: Some(1),
                cached_input_tokens: Some(2),
                output_tokens: Some(3),
                reasoning_tokens: Some(4),
                estimated_cost_usd: Some(0.5),
            },
            RuntimeEvent::RuntimeWarning {
                code: Some("retry".into()),
                message: "retrying".into(),
            },
            RuntimeEvent::RuntimeError {
                code: Some("failed".into()),
                message: "failed".into(),
            },
            RuntimeEvent::CheckpointCreated {
                native_session_id: Some("s".into()),
            },
        ]
    }

    #[test]
    fn every_event_variant_round_trips_with_stable_kind() {
        for event in variants() {
            let expected_kind = event.kind();
            let json = serde_json::to_string(&event).expect("serialize event");
            assert!(json.contains(&format!("\"kind\":\"{expected_kind}\"")));
            assert_eq!(serde_json::from_str::<RuntimeEvent>(&json).unwrap(), event);
        }
    }

    #[test]
    fn visibility_and_private_drop_policy_are_centralized() {
        assert_eq!(
            RuntimeEvent::AssistantMessageDelta {
                text: String::new()
            }
            .policy(),
            RuntimeEventPolicy::LiveOnly
        );
        assert_eq!(
            RuntimeEvent::ToolCallUpdated {
                native_call_id: String::new(),
                status: String::new(),
                output_summary: None
            }
            .policy(),
            RuntimeEventPolicy::LiveOnly
        );
        assert_eq!(
            RuntimeEvent::RuntimeError {
                code: None,
                message: String::new()
            }
            .policy(),
            RuntimeEventPolicy::Durable
        );
        for private in [
            PrivateRuntimeInput::RawWireFrame,
            PrivateRuntimeInput::AuthenticationMaterial,
            PrivateRuntimeInput::EnvironmentDump,
            PrivateRuntimeInput::HiddenChainOfThought,
            PrivateRuntimeInput::AcpAgentThoughtChunk,
            PrivateRuntimeInput::ClaudeThinkingBlock,
        ] {
            assert_eq!(private.policy(), RuntimeEventPolicy::PrivateDrop);
        }
    }

    #[test]
    fn bounds_are_utf8_safe_marked_and_under_the_payload_ceiling() {
        let event = RuntimeEvent::PlanUpdated {
            items: (0..100)
                .map(|_| RuntimePlanItem {
                    text: "界".repeat(2_000),
                    status: Some("running".repeat(200)),
                })
                .collect(),
        }
        .bounded();
        let json = serde_json::to_vec(&event).expect("bounded event serializes");
        assert!(json.len() <= MAX_DURABLE_RUNTIME_PAYLOAD_BYTES);
        let RuntimeEvent::PlanUpdated { items } = event else {
            unreachable!()
        };
        assert!(items.len() <= MAX_PLAN_ITEMS);
        assert!(items
            .iter()
            .all(|item| item.text.is_char_boundary(item.text.len())));
        assert!(items
            .iter()
            .all(|item| item.text.contains(TRUNCATION_MARKER)));
    }
}
