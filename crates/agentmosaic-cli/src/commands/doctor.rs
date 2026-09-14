//! `am doctor`: is the team ready to run?

use std::path::Path;
use std::time::Duration;

use agentmosaic_runtime::{AcpWorkerConfig, AcpWorkerDriver, CodexAppServer, LaunchSpec};
use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry};

use crate::project;

pub fn run() -> Result<String, String> {
    let (root, database) = project::project_database()?;
    let registry = SqliteAgentRegistry::open(&database).map_err(|e| format!("doctor: {e}"))?;
    let agents = registry.list_agents().map_err(|e| format!("doctor: {e}"))?;
    let mut lines = vec![
        format!("project   READY {}", root.display()),
        "state     READY schema=11".into(),
    ];
    let mut tiers = [0usize; 3];
    let mut agents_ready = true;
    for agent in agents {
        match agent.tier.as_str() {
            "reasoner" => tiers[0] += 1,
            "worker" => tiers[1] += 1,
            "utility" => tiers[2] += 1,
            _ => {}
        }
        let ready = agent.executable.as_deref().is_some_and(program_on_path);
        let launch = agent
            .executable
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            && agent
                .driver_args_json
                .as_deref()
                .map(|raw| serde_json::from_str::<Vec<String>>(raw).is_ok())
                .unwrap_or(true);
        let stages = if !ready {
            "PROGRAM_NOT_FOUND".to_string()
        } else if !launch {
            "PROGRAM_FOUND LAUNCHSPEC_INVALID".to_string()
        } else {
            format!(
                "PROGRAM_FOUND LAUNCHSPEC_VALID {}",
                doctor_probe(&agent, &root)
            )
        };
        if !stages.ends_with(" READY") {
            agents_ready = false;
        }
        lines.push(format!(
            "{}      {}    {}",
            agent.id,
            agent.driver_kind.as_deref().unwrap_or("-"),
            stages
        ));
    }
    let team_ready = tiers[0] == 1 && tiers[1] >= 1;
    lines.push(format!(
        "team      {} reasoner={} worker={} utility={}",
        if team_ready { "READY" } else { "NOT_READY" },
        tiers[0],
        tiers[1],
        tiers[2]
    ));
    if tiers[0] != 1 {
        lines.push("lead      LEAD_SELECTION_AMBIGUOUS_OR_MISSING".into());
    }
    if tiers[1] == 0 {
        lines.push("team      MISSING_WORKER".into());
    }
    let report = lines.join("\n");
    if team_ready && agents_ready {
        Ok(report)
    } else {
        Err(report)
    }
}

/// Probe the configured adapter without a prompt, model selection, or login.
/// The adapter owns the protocol details; the CLI reports only a bounded
/// readiness classification and never exposes protocol transcripts.
fn doctor_probe(agent: &AgentRegistryRecord, root: &Path) -> String {
    let Some(program) = agent.executable.as_deref() else {
        return "LAUNCHSPEC_INVALID".into();
    };
    let args = match agent.driver_args_json.as_deref() {
        Some(raw) => match serde_json::from_str::<Vec<String>>(raw) {
            Ok(args) => args,
            Err(_) => return "LAUNCHSPEC_INVALID".into(),
        },
        None => Vec::new(),
    };
    let launch = match LaunchSpec::new(program, args.clone()) {
        Ok(launch) => launch,
        Err(_) => return "LAUNCHSPEC_INVALID".into(),
    };
    match agent.driver_kind.as_deref() {
        Some("acp") => {
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
                Err(_) => return "SPAWN_FAILED".into(),
            };
            match super::team_runtime().and_then(|runtime| {
                runtime
                    .block_on(driver.probe_readiness())
                    .map_err(|error| error.to_string())
            }) {
                Ok(()) => "SPAWN_OK PROTOCOL_OK SESSION_OK READY".into(),
                Err(error) if error.to_ascii_lowercase().contains("auth") => {
                    "RUNTIME_PREPARATION_REQUIRED".into()
                }
                Err(error) if error.contains("timed out") => "PROTOCOL_UNAVAILABLE".into(),
                Err(error) if error.contains("No such file") => "SPAWN_FAILED".into(),
                Err(_) => "PROTOCOL_UNAVAILABLE".into(),
            }
        }
        Some("codex-app-server") => match CodexAppServer::spawn_launch(launch, &[]) {
            Err(_) => "SPAWN_FAILED".into(),
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
                Ok(_) => "SPAWN_OK PROTOCOL_OK SESSION_OK READY".into(),
                Err(error) if error.to_string().to_ascii_lowercase().contains("auth") => {
                    "RUNTIME_PREPARATION_REQUIRED".into()
                }
                Err(_) => "PROTOCOL_UNAVAILABLE".into(),
            },
        },
        _ => "PROTOCOL_UNAVAILABLE".into(),
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
