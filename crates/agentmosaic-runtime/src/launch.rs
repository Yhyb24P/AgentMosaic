//! The one shell-free process launch representation used by external Agents.

use std::path::{Path, PathBuf};

/// A process program plus its already-tokenized argv.
///
/// This deliberately has no command-string form: callers hand it directly to
/// `std::process::Command`, so AgentMosaic never invokes a shell to launch an
/// external runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl LaunchSpec {
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>) -> Result<Self, String> {
        let launch = Self {
            program: program.into(),
            args,
        };
        launch.validate()?;
        Ok(launch)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.program.as_os_str().is_empty() || self.program.to_string_lossy().trim().is_empty() {
            return Err("launch program is required".into());
        }
        Ok(())
    }

    pub fn program_display(&self) -> String {
        self.program.display().to_string()
    }

    pub fn from_registry(program: Option<&str>, args: Vec<String>) -> Result<Self, String> {
        let program = program
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "registered agent has no executable".to_string())?;
        Self::new(Path::new(program), args)
    }
}

#[cfg(test)]
mod tests {
    use super::LaunchSpec;

    #[test]
    fn rejects_an_empty_program() {
        assert!(LaunchSpec::new("", Vec::new()).is_err());
    }

    #[test]
    fn keeps_wrapper_argv_exactly() {
        let launch =
            LaunchSpec::new("aweswitch", vec!["qw".into(), "-ds".into(), "--acp".into()]).unwrap();
        assert_eq!(launch.args, ["qw", "-ds", "--acp"]);
    }
}
