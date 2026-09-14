//! `am doctor`: is the team ready to run?
//!
//! The decision itself lives in [`crate::output::DoctorReport`]; this module
//! only produces the bounded readiness facts it is rendered from. Probing is
//! deliberately cheap and non-invasive: no prompt, no model, no login.

use std::path::Path;
use std::time::Duration;

use agentmosaic_runtime::{
    validate_driver_config, validate_lead_config, AcpWorkerConfig, AcpWorkerDriver, CodexAppServer,
    LaunchSpec,
};
use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry};

use crate::json::{self, DoctorAgentJson, DoctorJson, TeamJson};
use crate::output::{DoctorAgent, DoctorReport, ReadinessStage};
use crate::project;

/// `am doctor`'s answer: the text the surface prints, and whether the team is
/// ready. `main` turns the readiness into the exit code, and the JSON surface
/// keeps its object even when the answer is "no".
pub fn run(verbose: bool, machine: bool) -> Result<(String, bool), String> {
    let (root, database) = project::project_database()?;
    let registry = SqliteAgentRegistry::open(&database).map_err(|e| format!("doctor: {e}"))?;
    let schema_version = registry
        .schema_version()
        .map_err(|e| format!("doctor: {e}"))?;
    let agents = registry.list_agents().map_err(|e| format!("doctor: {e}"))?;
    let lead = resolved_lead(&agents);
    let described = agents
        .iter()
        .map(|agent| describe(agent, &root, lead))
        .collect::<Vec<_>>();
    let report = DoctorReport {
        project_root: root.display().to_string(),
        schema_version,
        agents: described.iter().map(|agent| agent.view.clone()).collect(),
    };
    let ready = report.ready();
    let text = if machine {
        json::encode(&doctor_json(&report, &described))?
    } else {
        report.render(verbose)
    };
    Ok((text, ready))
}

/// One registered Agent, as the report renders it, plus the adapter the machine
/// surface names.
struct DescribedAgent {
    view: DoctorAgent,
    adapter: Option<String>,
}

/// One registered Agent, as the report renders it.
///
/// Configuration comes first: a run reads this Agent's registry row before it
/// touches the board, so a configuration a run would refuse is decided here
/// rather than after a probe that cannot see it. Only a configuration the run
/// itself accepts goes on to be probed.
fn describe(agent: &AgentRegistryRecord, root: &Path, lead: Option<&str>) -> DescribedAgent {
    let configuration = configuration_problem(agent, root, lead);
    let (stage, stages, detail) = match configuration {
        Some(detail) => (
            ReadinessStage::ConfigInvalid,
            "CONFIG_INVALID".to_string(),
            Some(detail),
        ),
        None => {
            let probe = probe_agent(agent, root);
            (probe.stage, probe.stages, None)
        }
    };
    DescribedAgent {
        view: DoctorAgent {
            id: agent.id.clone(),
            role: agent.tier.clone(),
            program: agent.executable.clone().unwrap_or_else(|| "-".into()),
            launch: crate::output::bounded_launch(agent),
            stage,
            stages,
            detail,
        },
        adapter: agent.driver_kind.clone(),
    }
}

/// The id of the run's Lead: the single registered reasoner, exactly as a run
/// resolves it. Zero or several reasoners is a composition problem the report
/// already carries, so no Agent is validated as the Lead then.
fn resolved_lead(agents: &[AgentRegistryRecord]) -> Option<&str> {
    let mut reasoners = agents.iter().filter(|agent| agent.tier == "reasoner");
    let only = reasoners.next()?;
    if reasoners.next().is_some() {
        return None;
    }
    Some(only.id.as_str())
}

/// The configuration problem a run would hit for this Agent, as the validator
/// itself states it, or None when a run's own configuration checks accept it.
///
/// The Lead's effective configuration is validated the way the run builds it,
/// and every Agent's driver config is validated the way the run parses it. Both
/// validators construct nothing and start no process.
fn configuration_problem(
    agent: &AgentRegistryRecord,
    root: &Path,
    lead: Option<&str>,
) -> Option<String> {
    // A run builds every Agent's driver before it builds the Lead's brain, so
    // the driver rules are read first: an operator sees the same failure the
    // run would report, in the run's own order.
    if let Err(detail) = validate_driver_config(agent) {
        return Some(detail);
    }
    if lead == Some(agent.id.as_str()) {
        return validate_lead_config(agent, root).err();
    }
    None
}

/// The decision as one typed object: what is registered, whether each runtime
/// answered, and — when the team is not ready — the reason and the fix.
fn doctor_json(report: &DoctorReport, described: &[DescribedAgent]) -> DoctorJson {
    let tiers = report.tiers();
    let decision = report.decision();
    DoctorJson {
        ready: report.ready(),
        project: report.project_root.clone(),
        schema_version: report.schema_version,
        agents: described
            .iter()
            .map(|agent| DoctorAgentJson {
                id: agent.view.id.clone(),
                role: agent.view.role.clone(),
                adapter: agent.adapter.clone(),
                ready: agent.view.stage.is_ready(),
                stage: agent.view.stage.as_str().to_string(),
            })
            .collect(),
        team: TeamJson {
            lead: tiers[0],
            worker: tiers[1],
            utility: tiers[2],
        },
        reason: decision.as_ref().map(|(reason, _)| reason.clone()),
        fix: decision.map(|(_, fix)| fix).unwrap_or_default(),
    }
}

/// A readiness classification and the bounded stage codes that produced it.
struct AgentProbe {
    stage: ReadinessStage,
    stages: String,
}

impl AgentProbe {
    fn new(stage: ReadinessStage, stages: impl Into<String>) -> Self {
        Self {
            stage,
            stages: stages.into(),
        }
    }
}

/// Classify one registered Agent without prompting it.
fn probe_agent(agent: &AgentRegistryRecord, root: &Path) -> AgentProbe {
    let Some(program) = agent.executable.as_deref() else {
        return AgentProbe::new(ReadinessStage::ProgramMissing, "PROGRAM_NOT_FOUND");
    };
    if !program_on_path(program) {
        return AgentProbe::new(ReadinessStage::ProgramMissing, "PROGRAM_NOT_FOUND");
    }
    let args = match agent.driver_args_json.as_deref() {
        Some(raw) => match serde_json::from_str::<Vec<String>>(raw) {
            Ok(args) => args,
            Err(_) => {
                return AgentProbe::new(
                    ReadinessStage::LaunchSpecInvalid,
                    "PROGRAM_FOUND LAUNCHSPEC_INVALID",
                )
            }
        },
        None => Vec::new(),
    };
    let launch = match LaunchSpec::new(program, args.clone()) {
        Ok(launch) => launch,
        Err(_) => {
            return AgentProbe::new(
                ReadinessStage::LaunchSpecInvalid,
                "PROGRAM_FOUND LAUNCHSPEC_INVALID",
            )
        }
    };
    let probe = match agent.driver_kind.as_deref() {
        Some("acp") => probe_acp(launch, args, root),
        Some("codex-app-server") => probe_codex(launch, root),
        _ => AgentProbe::new(ReadinessStage::ProtocolUnavailable, "PROTOCOL_UNAVAILABLE"),
    };
    AgentProbe::new(
        probe.stage,
        format!("PROGRAM_FOUND LAUNCHSPEC_VALID {}", probe.stages),
    )
}

/// Probe the configured adapter without a prompt, model selection, or login.
/// The adapter owns the protocol details; the CLI reports only a bounded
/// readiness classification and never exposes protocol transcripts.
fn probe_acp(launch: LaunchSpec, args: Vec<String>, root: &Path) -> AgentProbe {
    let driver = match AcpWorkerDriver::new(AcpWorkerConfig {
        runtime_kind: "acp".into(),
        command: launch.program,
        args,
        auth_method: None,
        working_directory: root.to_path_buf(),
        timeout: Duration::from_secs(5),
        max_prompt_bytes: 1,
        max_result_bytes: 1,
        artifact_paths: Vec::new(),
    }) {
        Ok(driver) => driver,
        Err(_) => return AgentProbe::new(ReadinessStage::SpawnFailed, "SPAWN_FAILED"),
    };
    match super::team_runtime().and_then(|runtime| {
        runtime
            .block_on(driver.probe_readiness())
            .map_err(|error| error.to_string())
    }) {
        Ok(()) => AgentProbe::new(
            ReadinessStage::Ready,
            "SPAWN_OK PROTOCOL_OK SESSION_OK READY",
        ),
        Err(error) if error.to_ascii_lowercase().contains("auth") => AgentProbe::new(
            ReadinessStage::RuntimePreparationRequired,
            "RUNTIME_PREPARATION_REQUIRED",
        ),
        Err(error) if error.contains("timed out") => {
            AgentProbe::new(ReadinessStage::ProtocolUnavailable, "PROTOCOL_UNAVAILABLE")
        }
        Err(error) if error.contains("No such file") => {
            AgentProbe::new(ReadinessStage::SpawnFailed, "SPAWN_FAILED")
        }
        Err(_) => AgentProbe::new(ReadinessStage::ProtocolUnavailable, "PROTOCOL_UNAVAILABLE"),
    }
}

fn probe_codex(launch: LaunchSpec, root: &Path) -> AgentProbe {
    match CodexAppServer::spawn_launch(launch, &[]) {
        Err(_) => AgentProbe::new(ReadinessStage::SpawnFailed, "SPAWN_FAILED"),
        Ok(mut server) => match server
            .initialize("agentmosaic-doctor", "0.2")
            .and_then(|_| {
                server.start_thread_with_options(
                    &root.display().to_string(),
                    None,
                    "read-only",
                    "never",
                )
            }) {
            Ok(_) => AgentProbe::new(
                ReadinessStage::Ready,
                "SPAWN_OK PROTOCOL_OK SESSION_OK READY",
            ),
            Err(error) if error.to_string().to_ascii_lowercase().contains("auth") => {
                AgentProbe::new(
                    ReadinessStage::RuntimePreparationRequired,
                    "RUNTIME_PREPARATION_REQUIRED",
                )
            }
            Err(_) => AgentProbe::new(ReadinessStage::ProtocolUnavailable, "PROTOCOL_UNAVAILABLE"),
        },
    }
}

/// A doctor program check is intentionally non-invasive: it never starts an
/// external Agent or attempts authentication. Protocol readiness is reported
/// only by adapters that can perform a safe handshake.
fn program_on_path(program: &str) -> bool {
    let candidate = Path::new(program);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        return candidate.is_file();
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(program).is_file())
    })
}
