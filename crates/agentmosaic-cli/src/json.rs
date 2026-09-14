//! The machine-readable (`--json`) surface.
//!
//! Every `--json` invocation prints exactly one typed object, and every type
//! lives here, so the stdout contract is one reviewable place. No command
//! assembles ad-hoc `serde_json::json!` for output.
//!
//! Two rules hold for every type:
//!
//! - Values are whole. The human renderers may abbreviate a digest or cut an
//!   objective; the machine form never does.
//! - Nothing a terminal would act on reaches the payload. `serde_json` escapes
//!   every control character, and no field carries raw argv, a credential, a
//!   native session/thread id or hidden model text.

use serde::{Deserialize, Serialize};

/// One recorded artifact: the task that produced it, its workspace-relative
/// path, and its whole digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactJson {
    pub task_id: u64,
    pub path: String,
    pub sha256: String,
}

/// One finished `am run`.
///
/// `run_id` is the durable root reasoning task, `status` its terminal state on
/// this surface, and `task_refs`/`artifact_refs` the completed work and exact
/// artifacts the Lead grounded the answer in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunJson {
    pub run_id: u64,
    pub lead_agent: String,
    pub status: String,
    pub answer: String,
    pub task_refs: Vec<u64>,
    pub artifact_refs: Vec<ArtifactJson>,
}

/// One task of a run's subtree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskJson {
    pub id: u64,
    pub assignee: Option<String>,
    pub status: String,
    pub objective: String,
}

/// One run with its subtree and every artifact recorded below it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusJson {
    pub run_id: u64,
    pub status: String,
    pub objective: String,
    pub lead: Option<String>,
    pub tasks: Vec<TaskJson>,
    pub artifacts: Vec<ArtifactJson>,
}

/// One run in the project listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummaryJson {
    pub run_id: u64,
    pub status: String,
    pub objective: String,
}

/// Every run of the project, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunListJson {
    pub runs: Vec<RunSummaryJson>,
}

/// The durable final result of one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalJson {
    pub run_id: u64,
    pub answer: String,
}

/// Every artifact of one task, or of one run's whole subtree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactListJson {
    pub artifacts: Vec<ArtifactJson>,
}

/// One registered Agent.
///
/// `launch` is the same bounded, credential-redacted rendering the human
/// surface prints: raw argv is never echoed, and the registry's driver config
/// is not a machine field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentJson {
    pub id: String,
    pub name: String,
    pub role: String,
    pub adapter: Option<String>,
    pub launch: String,
    pub concurrency: Option<i64>,
    pub tags: Vec<String>,
    pub version: Option<String>,
}

/// Every registered Agent of the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentListJson {
    pub agents: Vec<AgentJson>,
}

/// The registered Agents per team role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamJson {
    pub lead: usize,
    pub worker: usize,
    pub utility: usize,
}

/// One Agent in the doctor decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorAgentJson {
    pub id: String,
    pub role: String,
    pub adapter: Option<String>,
    pub ready: bool,
    /// The bounded readiness class, as one stable token.
    pub stage: String,
}

/// The `am doctor` decision: whether this project can run, and — when it
/// cannot — the reason and the fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorJson {
    pub ready: bool,
    pub project: String,
    pub schema_version: i32,
    pub agents: Vec<DoctorAgentJson>,
    pub team: TeamJson,
    pub reason: Option<String>,
    pub fix: Vec<String>,
}

/// One typed payload as the single line stdout carries.
///
/// The compact encoding is deliberate: one object, one line, and no trailing
/// text, so `am ... --json | jq` never sees framing the parser did not ask for.
pub fn encode<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| format!("json: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> RunJson {
        RunJson {
            run_id: 4,
            lead_agent: "lead".into(),
            status: "succeeded".into(),
            answer: "the whole answer".into(),
            task_refs: vec![5],
            artifact_refs: vec![ArtifactJson {
                task_id: 5,
                path: "result.txt".into(),
                sha256: "4297addc00112233445566778899aabbccddeeff00112233445566778899aabb".into(),
            }],
        }
    }

    /// The exact object the contract names, with its keys in order.
    #[test]
    fn a_finished_run_encodes_as_the_contract_object() {
        let encoded = encode(&run()).unwrap();
        assert_eq!(
            encoded,
            "{\"run_id\":4,\"lead_agent\":\"lead\",\"status\":\"succeeded\",\
             \"answer\":\"the whole answer\",\"task_refs\":[5],\
             \"artifact_refs\":[{\"task_id\":5,\"path\":\"result.txt\",\
             \"sha256\":\"4297addc00112233445566778899aabbccddeeff00112233445566778899aabb\"}]}"
        );
        assert!(!encoded.contains('\n'), "{encoded}");
    }

    /// The typed payload round-trips: what a consumer parses is what was sent.
    #[test]
    fn every_payload_round_trips() {
        let payloads = [
            encode(&run()).unwrap(),
            encode(&StatusJson {
                run_id: 4,
                status: "succeeded".into(),
                objective: "deliver the objective".into(),
                lead: Some("lead".into()),
                tasks: vec![
                    TaskJson {
                        id: 4,
                        assignee: Some("lead".into()),
                        status: "succeeded".into(),
                        objective: "deliver the objective".into(),
                    },
                    TaskJson {
                        id: 5,
                        assignee: None,
                        status: "pending".into(),
                        objective: "produce the worker result".into(),
                    },
                ],
                artifacts: vec![ArtifactJson {
                    task_id: 5,
                    path: "result.txt".into(),
                    sha256: "a".repeat(64),
                }],
            })
            .unwrap(),
            encode(&RunListJson {
                runs: vec![RunSummaryJson {
                    run_id: 4,
                    status: "succeeded".into(),
                    objective: "deliver the objective".into(),
                }],
            })
            .unwrap(),
            encode(&FinalJson {
                run_id: 4,
                answer: "the whole answer".into(),
            })
            .unwrap(),
            encode(&ArtifactListJson { artifacts: vec![] }).unwrap(),
            encode(&AgentListJson {
                agents: vec![AgentJson {
                    id: "lead".into(),
                    name: "lead".into(),
                    role: "reasoner".into(),
                    adapter: Some("codex-app-server".into()),
                    launch: "codex".into(),
                    concurrency: Some(1),
                    tags: vec!["local".into()],
                    version: None,
                }],
            })
            .unwrap(),
            encode(&DoctorJson {
                ready: false,
                project: "/tmp/project".into(),
                schema_version: 11,
                agents: vec![DoctorAgentJson {
                    id: "worker".into(),
                    role: "worker".into(),
                    adapter: Some("acp".into()),
                    ready: false,
                    stage: "program_missing".into(),
                }],
                team: TeamJson {
                    lead: 1,
                    worker: 1,
                    utility: 0,
                },
                reason: Some("program `qwen` was not found on PATH".into()),
                fix: vec!["install the worker runtime".into(), "am doctor".into()],
            })
            .unwrap(),
        ];
        for payload in payloads {
            let decoded: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert!(decoded.is_object(), "{payload}");
            assert_eq!(
                serde_json::to_string(&decoded).unwrap(),
                payload,
                "the payload did not round-trip"
            );
        }
    }

    /// A digest is never abbreviated and a long text is never cut: the machine
    /// form carries the value the human renderer is free to shorten.
    #[test]
    fn values_are_never_abbreviated_or_cut() {
        let digest = "b".repeat(64);
        let answer = "line one\nline two  ".to_string() + &"x".repeat(500);
        let encoded = encode(&RunJson {
            artifact_refs: vec![ArtifactJson {
                task_id: 5,
                path: "deep/nested/result.txt".into(),
                sha256: digest.clone(),
            }],
            answer: answer.clone(),
            ..run()
        })
        .unwrap();
        let decoded: RunJson = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.artifact_refs[0].sha256, digest);
        assert_eq!(decoded.artifact_refs[0].path, "deep/nested/result.txt");
        assert_eq!(decoded.answer, answer);
        assert!(!encoded.contains('…'), "{encoded}");
    }

    /// No payload can carry a terminal control sequence: `serde_json` escapes
    /// every control character, so an ESC in a durable field is inert text.
    #[test]
    fn no_payload_carries_a_control_sequence() {
        let escaped = "\u{1b}[2J\u{1b}]0;title\u{7}";
        let encoded = encode(&FinalJson {
            run_id: 1,
            answer: format!("answer {escaped} end"),
        })
        .unwrap();
        assert!(!encoded.as_bytes().contains(&0x1b), "{encoded}");
        assert!(!encoded.as_bytes().contains(&0x07), "{encoded}");
        assert!(!encoded.contains('\u{1b}'), "{encoded}");
        assert!(encoded.contains("\\u001b[2J"), "{encoded}");
        let decoded: FinalJson = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.answer, format!("answer {escaped} end"));
    }
}
