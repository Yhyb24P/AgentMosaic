//! `am init`: create the durable project state.

use std::path::PathBuf;

use crate::project::{self, PROJECT_DIR};

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
    std::fs::create_dir_all(&directory)
        .map_err(|e| format!("create project state directory: {e}"))?;
    let database = project::state_path(&root);
    project::open(database.to_str().ok_or("project state path is not UTF-8")?)?;
    let ignore = root.join(".gitignore");
    if root.join(".git").exists() {
        let existing = std::fs::read_to_string(&ignore).unwrap_or_default();
        if !existing.lines().any(|line| line.trim() == "/.agentmosaic/") {
            let suffix = if existing.is_empty() || existing.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            std::fs::write(&ignore, format!("{existing}{suffix}/.agentmosaic/\n"))
                .map_err(|e| format!("update .gitignore: {e}"))?;
        }
    }
    Ok(format!(
        "initialized AgentMosaic\nproject: {}\nstate:   {}\n\nnext:\n  am agent add ...\n  am doctor",
        root.display(),
        database.display()
    ))
}
