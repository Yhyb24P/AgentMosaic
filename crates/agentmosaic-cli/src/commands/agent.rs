//! `am agent add`, `am agent list` and `am agent remove`.

use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry};

use crate::json::{self, AgentJson, AgentListJson};
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
    let record = AgentRegistryRecord {
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
    };
    let (_, database) = project::project_database()?;
    let registry = SqliteAgentRegistry::open(&database).map_err(|e| e.to_string())?;
    // The registry itself answers whether the row already existed: no second
    // pass over the input decides which verb is printed.
    let created = registry
        .get_agent(&record.id)
        .map_err(|e| format!("agent add: {e}"))?
        .is_none();
    registry
        .upsert_agent(&record)
        .map_err(|e| format!("agent add: {e}"))?;
    Ok(output::render_agent_registration(
        &record.id,
        created,
        &record.tier,
        record.driver_kind.as_deref().unwrap_or("-"),
        &output::bounded_launch(&record),
    ))
}

/// List the durable registrations. This surface reads the registry only: it
/// starts no runtime and performs no readiness handshake, so an Agent whose
/// launch program is not installed still lists.
///
/// The machine form carries the registry's own fields, with the same bounded,
/// credential-redacted launch rendering the table prints: raw argv and the
/// driver config never reach a payload.
pub fn list(machine: bool) -> Result<String, String> {
    let (_, database) = project::project_database()?;
    let registry = SqliteAgentRegistry::open(&database).map_err(|e| format!("registry: {e}"))?;
    let agents = registry
        .list_agents()
        .map_err(|e| format!("registry: {e}"))?;
    if machine {
        return json::encode(&AgentListJson {
            agents: agents.iter().map(agent_json).collect(),
        });
    }
    Ok(output::render_agent_table(&agents))
}

fn agent_json(agent: &AgentRegistryRecord) -> AgentJson {
    AgentJson {
        id: agent.id.clone(),
        name: agent.name.clone(),
        role: agent.tier.clone(),
        adapter: agent.driver_kind.clone(),
        launch: output::bounded_launch(agent),
        concurrency: agent.max_concurrency,
        tags: string_list(agent.tags_json.as_deref()),
        version: agent.runtime_version.clone(),
    }
}

/// A registry list field as the strings it holds. The registry writes JSON
/// string arrays; anything else contributes nothing rather than echoing raw
/// stored text.
fn string_list(raw: Option<&str>) -> Vec<String> {
    raw.and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
        .unwrap_or_default()
}

/// Remove one registration. Only the registry entry is deleted: the tasks,
/// attempts, results, artifacts and bindings the Agent produced are history.
pub fn remove(id: &str) -> Result<String, String> {
    let (_, database) = project::project_database()?;
    let registry =
        SqliteAgentRegistry::open(&database).map_err(|e| format!("agent remove: {e}"))?;
    if !registry
        .delete_agent(id)
        .map_err(|e| format!("agent remove: {e}"))?
    {
        return Err(format!("no agent `{id}` is registered"));
    }
    // The team rule is evaluated from the registry as it is *after* the
    // deletion, so the warning is about the team the user now has.
    let agents = registry
        .list_agents()
        .map_err(|e| format!("agent remove: {e}"))?;
    let reasoners = agents
        .iter()
        .filter(|agent| agent.tier == "reasoner")
        .count();
    let workers = agents.iter().filter(|agent| agent.tier == "worker").count();
    Ok(output::render_agent_removed(
        id,
        reasoners == 1 && workers >= 1,
    ))
}
