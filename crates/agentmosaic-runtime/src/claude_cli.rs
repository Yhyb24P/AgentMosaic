//! Verified non-interactive Claude Code invocation construction.

use crate::{LaunchSpec, RuntimeError};

/// Shell-free argv for Claude Code 2.1.268's supported stream-json surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCliInvocation {
    pub launch: LaunchSpec,
    pub args: Vec<String>,
}

impl ClaudeCliInvocation {
    /// Create a fresh isolated non-interactive session. Prompt text is passed
    /// over stdin, never in argv.
    pub fn start(launch: LaunchSpec, json_schema: Option<&str>) -> Self {
        let mut args = base_args(&launch);
        append_schema(&mut args, json_schema);
        Self { launch, args }
    }

    /// Resume one exact foreign session with the same security boundary.
    pub fn resume(
        launch: LaunchSpec,
        session_id: &str,
        json_schema: Option<&str>,
    ) -> Result<Self, RuntimeError> {
        if session_id.trim().is_empty() {
            return Err(RuntimeError::Protocol(
                "Claude resume requires a non-empty session id".into(),
            ));
        }
        let mut args = base_args(&launch);
        args.extend(["--resume".into(), session_id.into()]);
        append_schema(&mut args, json_schema);
        Ok(Self { launch, args })
    }
}

fn base_args(launch: &LaunchSpec) -> Vec<String> {
    let mut args = launch.args.clone();
    args.extend([
        "--bare".into(),
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
        "--permission-mode".into(),
        "dontAsk".into(),
        "--permission-prompts".into(),
        "none".into(),
    ]);
    args
}

fn append_schema(args: &mut Vec<String>, schema: Option<&str>) {
    if let Some(schema) = schema.filter(|schema| !schema.trim().is_empty()) {
        args.extend(["--json-schema".into(), schema.into()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_uses_only_verified_machine_flags() {
        let launch = LaunchSpec::new("claude", vec!["--model".into(), "sonnet".into()]).unwrap();
        let invocation = ClaudeCliInvocation::resume(launch, "session-1", Some("{}")).unwrap();
        assert_eq!(invocation.args[0..2], ["--model", "sonnet"]);
        assert!(invocation
            .args
            .windows(2)
            .any(|part| part == ["--output-format", "stream-json"]));
        assert!(invocation
            .args
            .windows(2)
            .any(|part| part == ["--resume", "session-1"]));
        assert!(invocation.args.contains(&"--bare".into()));
        assert!(ClaudeCliInvocation::resume(
            LaunchSpec::new("claude", Vec::new()).unwrap(),
            " ",
            None
        )
        .is_err());
    }
}
