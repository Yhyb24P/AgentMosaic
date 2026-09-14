//! `am agent add` and `am agent list`.

use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry};

use crate::{output, project};

pub struct AgentAdd {
    pub id: String,
    pub role: String,
    pub adapter: String,
    pub name: Option<String>,
    pub concurrency: i64,
    pub tags: Vec<String>,
    pub artifacts: Vec<String>,
    pub launch: Vec<String>,
}

pub fn add(spec: AgentAdd) -> Result<String, String> {
    if !matches!(spec.role.as_str(), "reasoner" | "worker" | "utility") {
        return Err("--role must be reasoner, worker, or utility".into());
    }
    if !matches!(spec.adapter.as_str(), "acp" | "codex-app-server") {
        return Err("--adapter must be acp or codex-app-server".into());
    }
    if spec.concurrency <= 0 {
        return Err("max-concurrency must be greater than zero".into());
    }
    let (program, args) = spec
        .launch
        .split_first()
        .ok_or("agent add requires a launch command after --")?;
    let config = if spec.artifacts.is_empty() {
        None
    } else {
        Some(serde_json::json!({"artifact_paths": spec.artifacts}).to_string())
    };
    let (_, database) = project::project_database()?;
    SqliteAgentRegistry::open(&database)
        .map_err(|e| e.to_string())?
        .upsert_agent(&AgentRegistryRecord {
            id: spec.id.clone(),
            name: spec.name.unwrap_or_else(|| spec.id.clone()),
            tier: spec.role,
            driver_kind: Some(spec.adapter),
            executable: Some(program.clone()),
            driver_args_json: Some(serde_json::to_string(args).map_err(|e| e.to_string())?),
            max_concurrency: Some(spec.concurrency),
            tags_json: Some(serde_json::to_string(&spec.tags).map_err(|e| e.to_string())?),
            runtime_version: None,
            driver_config_json: config,
        })
        .map_err(|e| format!("agent add: {e}"))?;
    Ok(format!("added agent={}", spec.id))
}

pub fn list() -> Result<String, String> {
    let (_, database) = project::project_database()?;
    output::registry_list(
        database.to_str().ok_or("project state path is not UTF-8")?,
        None,
    )
}
