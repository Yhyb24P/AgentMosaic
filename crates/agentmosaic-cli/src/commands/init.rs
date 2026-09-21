//! `am init`: create the durable project state.
//!
//! `init` is idempotent and teaches the next step. It prints no storage path:
//! the durable file is the product's business, not the operator's.

use std::path::{Path, PathBuf};

use agentmosaic_storage::SqliteAgentRegistry;

use crate::project::{self, PROJECT_DIR};

/// The Lead onboarding command: the product's default reasoning runtime.
const LEAD_ADD: &str = "am agent add lead --role reasoner --adapter codex-exec -- codex";
/// The Worker onboarding command: the product's default ACP runtime.
const WORKER_ADD: &str = "am agent add worker --role worker --adapter acp -- qwen --acp";

pub fn run(path: Option<&str>) -> Result<String, String> {
    let requested = path
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let requested = requested
        .canonicalize()
        .map_err(|e| format!("init path is not available: {e}"))?;
    if !requested.is_dir() {
        return Err("init path must be a directory".into());
    }
    let root = std::process::Command::new("git")
        .arg("-C")
        .arg(&requested)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|result| result.status.success())
        .and_then(|result| String::from_utf8(result.stdout).ok())
        .map(|value| PathBuf::from(value.trim()))
        .unwrap_or(requested);
    let directory = root.join(PROJECT_DIR);
    let database = project::state_path(&root);
    let already_initialized = database.is_file();
    std::fs::create_dir_all(&directory)
        .map_err(|e| format!("create project state directory: {e}"))?;
    project::open(database.to_str().ok_or("project state path is not UTF-8")?)?;
    let ignored = update_gitignore(&root)?;
    let (reasoners, workers) = registered_roles(&database)?;

    let mut lines = vec![
        if already_initialized {
            "already initialized AgentMosaic".to_string()
        } else {
            "initialized AgentMosaic".to_string()
        },
        format!("project  {}", root.display()),
    ];
    if ignored {
        lines.push("updated .gitignore".into());
    }
    lines.push(String::new());
    lines.push(next_steps(reasoners, workers));
    Ok(lines.join("\n"))
}

/// Keep the durable state out of Git exactly once, and never create a
/// `.gitignore` outside a Git repository.
fn update_gitignore(root: &Path) -> Result<bool, String> {
    if !root.join(".git").exists() {
        return Ok(false);
    }
    let ignore = root.join(".gitignore");
    let existing = std::fs::read_to_string(&ignore).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == "/.agentmosaic/") {
        return Ok(false);
    }
    let suffix = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    std::fs::write(&ignore, format!("{existing}{suffix}/.agentmosaic/\n"))
        .map_err(|e| format!("update .gitignore: {e}"))?;
    Ok(true)
}

/// The roles already registered, so the next step is about the team the
/// project actually has.
fn registered_roles(database: &Path) -> Result<(usize, usize), String> {
    let registry = SqliteAgentRegistry::open(database).map_err(|e| format!("init: {e}"))?;
    let agents = registry.list_agents().map_err(|e| format!("init: {e}"))?;
    let reasoners = agents
        .iter()
        .filter(|agent| agent.tier == "reasoner")
        .count();
    let workers = agents.iter().filter(|agent| agent.tier == "worker").count();
    Ok((reasoners, workers))
}

/// What the project needs next, given the roles it already has.
fn next_steps(reasoners: usize, workers: usize) -> String {
    let mut blocks: Vec<String> = Vec::new();
    let headline = if reasoners == 0 {
        blocks.push(format!("Lead\n  {LEAD_ADD}"));
        if workers == 0 {
            blocks.push(format!("Worker\n  {WORKER_ADD}"));
            "Next: add a Lead and a Worker."
        } else {
            "Next: add a Lead."
        }
    } else if workers == 0 {
        blocks.push(format!("Worker\n  {WORKER_ADD}"));
        "Next: add a Worker."
    } else if reasoners > 1 {
        "Next: leave exactly one Lead; remove the extras with `am agent remove <id>`."
    } else {
        "Next: check the registered team is ready."
    };
    let mut text = headline.to_string();
    for block in blocks {
        text.push_str("\n\n");
        text.push_str(&block);
    }
    text.push_str("\n\nThen\n  am doctor");
    text
}
