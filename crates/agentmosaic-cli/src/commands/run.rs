//! The product run path: one objective for the team, and the live board.

use std::path::PathBuf;

use crate::project;
use crate::target::{self, TuiTarget};

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

/// `am tui [<DATABASE>]`: the live read-only board of this project, or of an
/// explicitly named database for the legacy spelling.
pub fn tui(database: Option<String>) -> Result<String, String> {
    let tokens: Vec<String> = database.into_iter().collect();
    let path = match target::tui_target(&tokens)? {
        TuiTarget::Project => project::ProjectContext::discover()?.database,
        TuiTarget::LegacyDatabase(path) => PathBuf::from(path),
    };
    agentmosaic_tui::run(path.to_str().ok_or("state database path is not UTF-8")?)?;
    Ok(String::new())
}
