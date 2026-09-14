//! Project discovery and durable state access.
//!
//! Nothing here runs until a command has been parsed and dispatched, so
//! argument handling can never create state as a side effect.

use std::path::{Path, PathBuf};

use agentmosaic_storage::SqliteTaskBoard;
use rusqlite::Connection;

pub const PROJECT_DIR: &str = ".agentmosaic";
pub const PROJECT_DB: &str = "state.db";

pub fn state_path(root: &Path) -> PathBuf {
    root.join(PROJECT_DIR).join(PROJECT_DB)
}

pub fn discover_project(start: &Path) -> Result<(PathBuf, PathBuf), String> {
    let start = start
        .canonicalize()
        .map_err(|e| format!("cannot resolve current directory: {e}"))?;
    for directory in start.ancestors() {
        let database = state_path(directory);
        if database.is_file() {
            return Ok((directory.to_path_buf(), database));
        }
    }
    Err("no initialized AgentMosaic project found; run `am init` first".into())
}

pub fn project_database() -> Result<(PathBuf, PathBuf), String> {
    discover_project(&std::env::current_dir().map_err(|e| e.to_string())?)
}

/// The project the current directory belongs to.
///
/// The project root and its durable database always travel together: a command
/// that inspects the project must never name a database path itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContext {
    pub root: PathBuf,
    pub database: PathBuf,
}

impl ProjectContext {
    /// Discover the project from the current directory, walking ancestors.
    pub fn discover() -> Result<Self, String> {
        let (root, database) = project_database()?;
        Ok(Self { root, database })
    }

    /// Open the project's durable board.
    pub fn open_board(&self) -> Result<SqliteTaskBoard, String> {
        open_path(&self.database)
    }
}

pub fn open(path: &str) -> Result<SqliteTaskBoard, String> {
    open_path(Path::new(path))
}

pub fn open_path(path: &Path) -> Result<SqliteTaskBoard, String> {
    SqliteTaskBoard::open(Connection::open(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
