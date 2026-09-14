//! The product run path: one objective for the team, and the live board.

use crate::project;

/// `am run "<objective...>"`: discover the project, then hand the objective to
/// the same durable team run the `run-team` compatibility spelling uses.
pub fn run(objective: &[String]) -> Result<String, String> {
    let (root, database) = project::project_database()?;
    let objective = objective.join(" ");
    if objective.trim().is_empty() {
        return Err("am run requires an objective".into());
    }
    super::advanced::run_team(
        database.to_str().ok_or("project state path is not UTF-8")?,
        &[root.display().to_string(), objective],
    )
}

pub fn tui(database: &str) -> Result<String, String> {
    agentmosaic_tui::run(database)?;
    Ok(String::new())
}
