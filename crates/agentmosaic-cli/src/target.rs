//! Project inspection targets. Run and task ids are decimal board ids.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusTarget {
    LatestRun,
    AllRuns,
    Run(u64),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalTarget {
    LatestRun,
    Run(u64),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactTarget {
    LatestRun,
    Task(u64),
}

fn id(token: &str) -> Result<u64, String> {
    token
        .parse()
        .map_err(|_| format!("invalid task or run id `{token}`; use this project's decimal id"))
}
pub fn status_target(tokens: &[String], all: bool) -> Result<StatusTarget, String> {
    match tokens {
        [] if all => Ok(StatusTarget::AllRuns),
        [] => Ok(StatusTarget::LatestRun),
        [token] if !all => Ok(StatusTarget::Run(id(token)?)),
        _ => Err("status takes one run id, or --all without a target".into()),
    }
}
pub fn final_target(tokens: &[String]) -> Result<FinalTarget, String> {
    match tokens {
        [] => Ok(FinalTarget::LatestRun),
        [token] => Ok(FinalTarget::Run(id(token)?)),
        _ => Err("final takes at most one run id".into()),
    }
}
pub fn artifact_target(tokens: &[String]) -> Result<ArtifactTarget, String> {
    match tokens {
        [] => Ok(ArtifactTarget::LatestRun),
        [token] => Ok(ArtifactTarget::Task(id(token)?)),
        _ => Err("artifact takes at most one task id".into()),
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiTarget {
    Project,
    Database(String),
}
pub fn tui_target(tokens: &[String]) -> Result<TuiTarget, String> {
    match tokens {
        [] => Ok(TuiTarget::Project),
        [path] => Ok(TuiTarget::Database(path.clone())),
        _ => Err("tui takes at most one database path".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspection_accepts_project_ids_and_rejects_database_grammar() {
        assert_eq!(status_target(&[], false), Ok(StatusTarget::LatestRun));
        assert_eq!(status_target(&[], true), Ok(StatusTarget::AllRuns));
        assert_eq!(final_target(&["7".into()]), Ok(FinalTarget::Run(7)));
        assert_eq!(artifact_target(&["8".into()]), Ok(ArtifactTarget::Task(8)));
        assert!(status_target(&["state.db".into()], false).is_err());
        assert!(status_target(&["7".into()], true).is_err());
        assert!(final_target(&["state.db".into(), "7".into()]).is_err());
        assert!(artifact_target(&["state.db".into(), "7".into()]).is_err());
    }
}
