//! Explicit target disambiguation for the four normal inspection commands.
//!
//! `status`, `final`, `artifact` and `tui` each accept a bare positional. That
//! positional is either a run/task id or a state database path, and the two
//! are never guessed at: a token is an id exactly when it parses as a `u64`,
//! and a token that is both an id and an existing file is refused outright.
//! The rules live here, away from the command bodies, so every accepted and
//! rejected spelling is unit-testable without a database.

use std::path::Path;

/// One bare positional token, classified without guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A decimal run or task id.
    Id(u64),
    /// A state database path.
    Path(String),
}

/// Classify one bare positional token against the real filesystem.
pub fn classify(token: &str) -> Result<Token, String> {
    classify_with(token, |path| path.exists())
}

/// Classify one token, probing the filesystem with `exists`.
///
/// The probe is a parameter so the ambiguity rule can be tested without
/// creating files in a shared working directory.
fn classify_with(token: &str, exists: impl Fn(&Path) -> bool) -> Result<Token, String> {
    match token.parse::<u64>() {
        Ok(_) if exists(Path::new(token)) => Err(format!(
            "ambiguous target `{token}`: it is a valid id and an existing file; \
             pass the id alone, or name the database with a path that is not a bare number"
        )),
        Ok(id) => Ok(Token::Id(id)),
        Err(_) => Ok(Token::Path(token.to_string())),
    }
}

/// What `am status` was asked to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusTarget {
    /// The newest run of the discovered project.
    LatestRun,
    /// Every run of the discovered project, newest first.
    AllRuns,
    /// One run of the discovered project.
    Run(u64),
    /// The legacy whole-board listing of this database.
    LegacyBoard(String),
}

/// `am status [<RUN_OR_DATABASE>] [--all]`.
pub fn status_target(tokens: &[String], all: bool) -> Result<StatusTarget, String> {
    match tokens {
        [] if all => Ok(StatusTarget::AllRuns),
        [] => Ok(StatusTarget::LatestRun),
        [token] if all => Err(format!(
            "`status --all` lists every run of this project and takes no target, but `{token}` was given"
        )),
        [token] => match classify(token)? {
            Token::Id(id) => Ok(StatusTarget::Run(id)),
            Token::Path(database) => Ok(StatusTarget::LegacyBoard(database)),
        },
        _ => Err("status takes at most one run id or database path".into()),
    }
}

/// What `am final` was asked to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalTarget {
    /// The newest run of the discovered project.
    LatestRun,
    /// One run of the discovered project.
    Run(u64),
    /// The legacy `<database> <root>` form.
    Legacy { database: String, root: u64 },
}

/// `am final [<RUN_OR_DATABASE> [<ROOT>]]`.
pub fn final_target(tokens: &[String]) -> Result<FinalTarget, String> {
    match tokens {
        [] => Ok(FinalTarget::LatestRun),
        [token] => match classify(token)? {
            Token::Id(id) => Ok(FinalTarget::Run(id)),
            Token::Path(_) => Err(format!(
                "`final` needs a run id, or a database path and a root task; `{token}` is a database path"
            )),
        },
        [database, root] => match classify(database)? {
            Token::Path(database) => Ok(FinalTarget::Legacy {
                database,
                root: parse_task_id(root)?,
            }),
            Token::Id(_) => Err(legacy_pair_error("final", "root task", database)),
        },
        _ => Err("final takes a run id, or a database path and a root task".into()),
    }
}

/// What `am artifact` was asked to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactTarget {
    /// Every artifact of the newest run of the discovered project.
    LatestRun,
    /// The artifacts of one task of the discovered project.
    Task(u64),
    /// The legacy `<database> <task>` form.
    Legacy { database: String, task: u64 },
}

/// `am artifact [<TASK_OR_DATABASE> [<TASK>]]`.
pub fn artifact_target(tokens: &[String]) -> Result<ArtifactTarget, String> {
    match tokens {
        [] => Ok(ArtifactTarget::LatestRun),
        [token] => match classify(token)? {
            Token::Id(id) => Ok(ArtifactTarget::Task(id)),
            Token::Path(_) => Err(format!(
                "`artifact` needs a task id, or a database path and a task; `{token}` is a database path"
            )),
        },
        [database, task] => match classify(database)? {
            Token::Path(database) => Ok(ArtifactTarget::Legacy {
                database,
                task: parse_task_id(task)?,
            }),
            Token::Id(_) => Err(legacy_pair_error("artifact", "task", database)),
        },
        _ => Err("artifact takes a task id, or a database path and a task".into()),
    }
}

/// What `am tui` was asked to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiTarget {
    /// The discovered project's database.
    Project,
    /// An explicitly named database path.
    LegacyDatabase(String),
}

/// `am tui [<DATABASE>]`.
pub fn tui_target(tokens: &[String]) -> Result<TuiTarget, String> {
    match tokens {
        [] => Ok(TuiTarget::Project),
        [token] => match classify(token)? {
            Token::Path(database) => Ok(TuiTarget::LegacyDatabase(database)),
            Token::Id(id) => Err(format!(
                "`tui` opens a state database, and `{id}` is an id; name the database path instead"
            )),
        },
        _ => Err("tui takes at most one database path".into()),
    }
}

fn parse_task_id(token: &str) -> Result<u64, String> {
    token
        .parse()
        .map_err(|_| format!("invalid task id `{token}`"))
}

fn legacy_pair_error(command: &str, second: &str, first: &str) -> String {
    format!(
        "`{command} {first}` is not a database path, so `{command}` cannot take a {second}; \
         pass one run id, or a database path and a {second}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn a_token_is_an_id_only_when_it_parses_as_a_u64() {
        assert_eq!(classify_with("12", |_| false), Ok(Token::Id(12)));
        assert_eq!(classify_with("0", |_| false), Ok(Token::Id(0)));
        assert_eq!(
            classify_with("board.db", |_| false),
            Ok(Token::Path("board.db".into()))
        );
        assert_eq!(
            classify_with("../state.db", |_| false),
            Ok(Token::Path("../state.db".into()))
        );
        assert_eq!(
            classify_with("run-2", |_| false),
            Ok(Token::Path("run-2".into()))
        );
    }

    #[test]
    fn a_token_that_is_both_an_id_and_an_existing_file_is_refused_by_name() {
        let error = classify_with("7", |_| true).expect_err("ambiguous");
        assert!(error.contains("ambiguous"), "{error}");
        assert!(error.contains("`7`"), "{error}");
    }

    #[test]
    fn status_without_a_target_selects_the_latest_run_or_every_run() {
        assert_eq!(status_target(&[], false), Ok(StatusTarget::LatestRun));
        assert_eq!(status_target(&[], true), Ok(StatusTarget::AllRuns));
        assert!(status_target(&tokens(&["7"]), true).is_err());
    }

    #[test]
    fn status_reads_one_run_id_or_the_legacy_database() {
        assert_eq!(
            status_target(&tokens(&["12"]), false),
            Ok(StatusTarget::Run(12))
        );
        assert_eq!(
            status_target(&tokens(&["board.db"]), false),
            Ok(StatusTarget::LegacyBoard("board.db".into()))
        );
        assert!(status_target(&tokens(&["12", "13"]), false).is_err());
    }

    #[test]
    fn final_accepts_no_target_a_run_id_or_the_legacy_pair() {
        assert_eq!(final_target(&[]), Ok(FinalTarget::LatestRun));
        assert_eq!(final_target(&tokens(&["12"])), Ok(FinalTarget::Run(12)));
        assert_eq!(
            final_target(&tokens(&["board.db", "4"])),
            Ok(FinalTarget::Legacy {
                database: "board.db".into(),
                root: 4
            })
        );
        assert!(final_target(&tokens(&["board.db"])).is_err());
        assert!(final_target(&tokens(&["board.db", "root"])).is_err());
        assert!(final_target(&tokens(&["12", "13"])).is_err());
        assert!(final_target(&tokens(&["a", "b", "c"])).is_err());
    }

    #[test]
    fn artifact_accepts_no_target_a_task_id_or_the_legacy_pair() {
        assert_eq!(artifact_target(&[]), Ok(ArtifactTarget::LatestRun));
        assert_eq!(
            artifact_target(&tokens(&["12"])),
            Ok(ArtifactTarget::Task(12))
        );
        assert_eq!(
            artifact_target(&tokens(&["board.db", "4"])),
            Ok(ArtifactTarget::Legacy {
                database: "board.db".into(),
                task: 4
            })
        );
        assert!(artifact_target(&tokens(&["board.db"])).is_err());
        assert!(artifact_target(&tokens(&["board.db", "task"])).is_err());
        assert!(artifact_target(&tokens(&["a", "b", "c"])).is_err());
    }

    #[test]
    fn tui_resolves_the_project_or_one_database_path() {
        assert_eq!(tui_target(&[]), Ok(TuiTarget::Project));
        assert_eq!(
            tui_target(&tokens(&["board.db"])),
            Ok(TuiTarget::LegacyDatabase("board.db".into()))
        );
        assert!(tui_target(&tokens(&["7"])).is_err());
        assert!(tui_target(&tokens(&["a", "b"])).is_err());
    }
}
