//! Construct real Agent drivers from the durable registry.
//!
//! A team run needs one live driver per registered Agent. The durable registry
//! (`agent_registry`) stores only non-secret facts: the kind, the executable,
//! the argv, and a JSON object of non-secret options. This module turns those
//! rows into [`AcpWorkerConfig`] / [`CodexTeamDriverConfig`] drivers.
//!
//! It never guesses and it never skips: a row whose kind cannot drive an
//! automatic team run is a hard error, because a silently skipped Agent would
//! make the scheduler's routing lie about who can do the work. Option keys that
//! look like credentials (`token`, `key`, `secret`, `password`, `endpoint`) are
//! refused outright, so a secret can never be smuggled through this path.
//!
//! `driver_config_json` is a JSON object. Unknown keys are ignored (one object
//! may also carry the Lead's own options), and every option has a documented
//! default:
//!
//! - `acp`: `auth_method` (string or null, default none), `timeout_seconds`
//!   (default 300), `max_prompt_bytes` (default 32768), `max_result_bytes`
//!   (default 16384), `artifact_paths` (relative paths inside the repository,
//!   default none).
//! - `codex-app-server`: `mcp_command` (required, an existing file),
//!   `artifact_paths` (relative, default none), `max_events` (default 200),
//!   `overrides` (extra `codex -c` values, default none).
//!
//! One additional set of keys configures a `codex-app-server` agent when it is
//! the run's Lead: `model` (the resident thread's model), `max_prompt_bytes`
//! (default 32768), `max_answer_bytes` (default 16384), and `max_events`
//! (default 200). The Lead's `driver_args` are not used: the Lead brain speaks
//! the `app-server --stdio` protocol itself.
//!
//! The run's repository is the working directory of every driver built here.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agent_code_storage::AgentRegistryRecord;
use agent_code_team::{AgentDriver, DriverKind};
use serde_json::{Map, Value};

use crate::{
    AcpWorkerConfig, CodexTeamDriverConfig, PersistedAcpWorkerDriver, PersistedCodexTeamDriver,
};

/// Default bound for one ACP task.
pub const DEFAULT_ACP_TIMEOUT_SECONDS: u64 = 300;
/// Default byte bound for one ACP prompt.
pub const DEFAULT_ACP_MAX_PROMPT_BYTES: usize = 32768;
/// Default byte bound for one ACP result.
pub const DEFAULT_ACP_MAX_RESULT_BYTES: usize = 16384;
/// Default event bound for one Codex app-server task turn.
pub const DEFAULT_CODEX_MAX_EVENTS: usize = 200;

/// Driver option keys that must never appear in `driver_config_json`: a driver
/// config is durable, shareable text, never a credential store.
const SECRET_KEY_MARKERS: [&str; 5] = ["token", "key", "secret", "password", "endpoint"];

/// A validation error from the driver factory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverFactoryError {
    /// The row carries no `driver_kind`, so no driver can be constructed.
    MissingDriverKind(String),
    /// The row names a kind that cannot drive an automatic team run.
    UnsupportedDriverKind { agent: String, kind: String },
    /// The row has no executable.
    MissingExecutable(String),
    /// `driver_args_json` is not a JSON string array.
    InvalidDriverArgs { agent: String, detail: String },
    /// `driver_config_json` is missing, malformed, or carries a bad option.
    InvalidDriverConfig { agent: String, detail: String },
    /// `driver_config_json` carries a key that looks like a credential.
    SecretLikeConfigKey { agent: String, key: String },
    /// An artifact path escapes the repository.
    InvalidArtifactPath { agent: String, path: String },
    /// The underlying driver refused its configuration.
    Driver { agent: String, detail: String },
}

impl std::fmt::Display for DriverFactoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingDriverKind(agent) => {
                write!(f, "agent `{agent}` has no driver kind")
            }
            Self::UnsupportedDriverKind { agent, kind } => write!(
                f,
                "agent `{agent}` has driver kind `{kind}`, which cannot drive an automatic team run"
            ),
            Self::MissingExecutable(agent) => {
                write!(f, "agent `{agent}` has no executable")
            }
            Self::InvalidDriverArgs { agent, detail } => {
                write!(f, "agent `{agent}` has invalid driver args: {detail}")
            }
            Self::InvalidDriverConfig { agent, detail } => {
                write!(f, "agent `{agent}` has an invalid driver config: {detail}")
            }
            Self::SecretLikeConfigKey { agent, key } => write!(
                f,
                "agent `{agent}` driver config key `{key}` looks like a credential; driver configs must never carry secrets"
            ),
            Self::InvalidArtifactPath { agent, path } => write!(
                f,
                "agent `{agent}` artifact path `{path}` must be a non-empty path inside the repository"
            ),
            Self::Driver { agent, detail } => {
                write!(f, "agent `{agent}` driver could not be constructed: {detail}")
            }
        }
    }
}

impl std::error::Error for DriverFactoryError {}

/// Builds one live driver per durable registry row.
pub struct DriverFactory {
    database: PathBuf,
    repo: PathBuf,
}

impl DriverFactory {
    pub fn new(database: impl Into<PathBuf>, repo: impl Into<PathBuf>) -> Self {
        Self {
            database: database.into(),
            repo: repo.into(),
        }
    }

    /// Build a driver for every record. Every record must produce exactly one
    /// driver: a record that cannot is an error, never a skip.
    pub fn build(
        &self,
        records: &[AgentRegistryRecord],
    ) -> Result<BTreeMap<String, Arc<dyn AgentDriver>>, DriverFactoryError> {
        let mut drivers: BTreeMap<String, Arc<dyn AgentDriver>> = BTreeMap::new();
        for record in records {
            let driver = self.build_one(record)?;
            drivers.insert(record.id.clone(), driver);
        }
        Ok(drivers)
    }

    fn build_one(
        &self,
        record: &AgentRegistryRecord,
    ) -> Result<Arc<dyn AgentDriver>, DriverFactoryError> {
        let agent = record.id.clone();
        let Some(raw_kind) = record
            .driver_kind
            .as_deref()
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
        else {
            return Err(DriverFactoryError::MissingDriverKind(agent));
        };
        let kind = DriverKind::restore(raw_kind).ok_or_else(|| {
            DriverFactoryError::UnsupportedDriverKind {
                agent: agent.clone(),
                kind: raw_kind.to_string(),
            }
        })?;
        let options = parse_agent_options(&agent, record.driver_config_json.as_deref())?;
        match kind {
            DriverKind::Acp => self.build_acp(record, &options),
            DriverKind::CodexAppServer => self.build_codex(record, &options),
            DriverKind::Native | DriverKind::Cli => {
                Err(DriverFactoryError::UnsupportedDriverKind {
                    agent,
                    kind: raw_kind.to_string(),
                })
            }
        }
    }

    fn build_acp(
        &self,
        record: &AgentRegistryRecord,
        options: &AgentOptions,
    ) -> Result<Arc<dyn AgentDriver>, DriverFactoryError> {
        let agent = record.id.clone();
        let command = executable(record)?;
        let args = driver_args(record)?;
        let timeout_seconds = options
            .timeout_seconds
            .unwrap_or(DEFAULT_ACP_TIMEOUT_SECONDS);
        if timeout_seconds == 0 {
            return Err(DriverFactoryError::InvalidDriverConfig {
                agent: agent.clone(),
                detail: "timeout_seconds must be greater than zero".into(),
            });
        }
        let max_prompt_bytes = options
            .max_prompt_bytes
            .unwrap_or(DEFAULT_ACP_MAX_PROMPT_BYTES);
        let max_result_bytes = options
            .max_result_bytes
            .unwrap_or(DEFAULT_ACP_MAX_RESULT_BYTES);
        if max_prompt_bytes == 0 || max_result_bytes == 0 {
            return Err(DriverFactoryError::InvalidDriverConfig {
                agent: agent.clone(),
                detail: "max_prompt_bytes and max_result_bytes must be greater than zero".into(),
            });
        }
        for path in &options.artifact_paths {
            validate_artifact_path(&agent, path)?;
        }
        let config = AcpWorkerConfig {
            runtime_kind: "acp".into(),
            command: PathBuf::from(command),
            args,
            auth_method: options.auth_method.clone(),
            working_directory: self.repo.clone(),
            timeout: Duration::from_secs(timeout_seconds),
            max_prompt_bytes,
            max_result_bytes,
            artifact_paths: options
                .artifact_paths
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>(),
        };
        let driver = PersistedAcpWorkerDriver::new(config, self.database.clone(), agent.clone())
            .map_err(|error| DriverFactoryError::Driver {
                agent: agent.clone(),
                detail: error.to_string(),
            })?;
        Ok(Arc::new(driver))
    }

    fn build_codex(
        &self,
        record: &AgentRegistryRecord,
        options: &AgentOptions,
    ) -> Result<Arc<dyn AgentDriver>, DriverFactoryError> {
        let agent = record.id.clone();
        let command = executable(record)?;
        let mcp_command = options
            .mcp_command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| DriverFactoryError::InvalidDriverConfig {
                agent: agent.clone(),
                detail: "codex-app-server requires a non-empty `mcp_command`".into(),
            })?;
        let max_events = options.max_events.unwrap_or(DEFAULT_CODEX_MAX_EVENTS);
        if max_events == 0 {
            return Err(DriverFactoryError::InvalidDriverConfig {
                agent: agent.clone(),
                detail: "max_events must be greater than zero".into(),
            });
        }
        for path in &options.artifact_paths {
            validate_artifact_path(&agent, path)?;
        }
        let config = CodexTeamDriverConfig {
            command,
            working_directory: self.repo.clone(),
            mcp_command: PathBuf::from(mcp_command),
            artifact_paths: options.artifact_paths.clone(),
            max_events,
            overrides: options.overrides.clone(),
        };
        let driver = PersistedCodexTeamDriver::new(config, self.database.clone(), agent.clone())
            .map_err(|error| DriverFactoryError::Driver {
                agent: agent.clone(),
                detail: error,
            })?;
        Ok(Arc::new(driver))
    }
}

/// The non-secret driver options one `driver_config_json` object may carry.
///
/// They are parsed once and shared: the driver factory consumes the driver-side
/// options, and the product runner consumes the Lead-side options (`model`,
/// `max_answer_bytes`) of the same object. Unknown keys are tolerated so one
/// object can serve both, but a secret-looking key is always refused.
#[derive(Debug, Clone, Default)]
pub(crate) struct AgentOptions {
    pub(crate) auth_method: Option<String>,
    pub(crate) timeout_seconds: Option<u64>,
    pub(crate) max_prompt_bytes: Option<usize>,
    pub(crate) max_result_bytes: Option<usize>,
    pub(crate) artifact_paths: Vec<String>,
    pub(crate) mcp_command: Option<String>,
    pub(crate) max_events: Option<usize>,
    pub(crate) overrides: Vec<String>,
    pub(crate) model: Option<String>,
    pub(crate) max_answer_bytes: Option<usize>,
}

/// Parse and validate one `driver_config_json` body. A missing body is an empty
/// option set (every option then takes its documented default).
pub(crate) fn parse_agent_options(
    agent: &str,
    raw: Option<&str>,
) -> Result<AgentOptions, DriverFactoryError> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(AgentOptions::default());
    };
    let parsed: Value =
        serde_json::from_str(raw).map_err(|error| DriverFactoryError::InvalidDriverConfig {
            agent: agent.to_string(),
            detail: format!("driver_config_json is not valid JSON: {error}"),
        })?;
    let Value::Object(map) = parsed else {
        return Err(DriverFactoryError::InvalidDriverConfig {
            agent: agent.to_string(),
            detail: "driver_config_json must be a JSON object".into(),
        });
    };
    for key in map.keys() {
        let lowered = key.to_ascii_lowercase();
        if SECRET_KEY_MARKERS
            .iter()
            .any(|marker| lowered.contains(marker))
        {
            return Err(DriverFactoryError::SecretLikeConfigKey {
                agent: agent.to_string(),
                key: key.clone(),
            });
        }
    }
    Ok(AgentOptions {
        auth_method: optional_string(agent, &map, "auth_method")?,
        timeout_seconds: optional_u64(agent, &map, "timeout_seconds")?,
        max_prompt_bytes: optional_usize(agent, &map, "max_prompt_bytes")?,
        max_result_bytes: optional_usize(agent, &map, "max_result_bytes")?,
        artifact_paths: string_array(agent, &map, "artifact_paths")?,
        mcp_command: optional_string(agent, &map, "mcp_command")?,
        max_events: optional_usize(agent, &map, "max_events")?,
        overrides: string_array(agent, &map, "overrides")?,
        model: optional_string(agent, &map, "model")?,
        max_answer_bytes: optional_usize(agent, &map, "max_answer_bytes")?,
    })
}

fn invalid_config(agent: &str, detail: impl Into<String>) -> DriverFactoryError {
    DriverFactoryError::InvalidDriverConfig {
        agent: agent.to_string(),
        detail: detail.into(),
    }
}

fn optional_string(
    agent: &str,
    map: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, DriverFactoryError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid_config(
            agent,
            format!("`{key}` must be a string or null"),
        )),
    }
}

fn optional_u64(
    agent: &str,
    map: &Map<String, Value>,
    key: &str,
) -> Result<Option<u64>, DriverFactoryError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| invalid_config(agent, format!("`{key}` must be a non-negative number"))),
        Some(_) => Err(invalid_config(agent, format!("`{key}` must be a number"))),
    }
}

fn optional_usize(
    agent: &str,
    map: &Map<String, Value>,
    key: &str,
) -> Result<Option<usize>, DriverFactoryError> {
    let value = optional_u64(agent, map, key)?;
    match value {
        None => Ok(None),
        Some(value) => usize::try_from(value)
            .map(Some)
            .map_err(|_| invalid_config(agent, format!("`{key}` is too large"))),
    }
}

fn string_array(
    agent: &str,
    map: &Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, DriverFactoryError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(value) => Ok(value.clone()),
                _ => Err(invalid_config(
                    agent,
                    format!("`{key}` must be an array of strings"),
                )),
            })
            .collect(),
        Some(_) => Err(invalid_config(
            agent,
            format!("`{key}` must be an array of strings"),
        )),
    }
}

fn executable(record: &AgentRegistryRecord) -> Result<String, DriverFactoryError> {
    record
        .executable
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| DriverFactoryError::MissingExecutable(record.id.clone()))
}

fn driver_args(record: &AgentRegistryRecord) -> Result<Vec<String>, DriverFactoryError> {
    let Some(raw) = record
        .driver_args_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(Vec::new());
    };
    serde_json::from_str(raw).map_err(|error| DriverFactoryError::InvalidDriverArgs {
        agent: record.id.clone(),
        detail: format!("stored driver args are not a JSON string array: {error}"),
    })
}

/// An artifact path must be non-empty, relative, and cannot walk out of the
/// repository. A path that is joined to the repository root afterwards
/// therefore always stays inside it.
fn validate_artifact_path(agent: &str, path: &str) -> Result<(), DriverFactoryError> {
    let invalid = || DriverFactoryError::InvalidArtifactPath {
        agent: agent.to_string(),
        path: path.to_string(),
    };
    let candidate = Path::new(path);
    if path.trim().is_empty() || candidate.is_absolute() {
        return Err(invalid());
    }
    if candidate.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) | Component::CurDir
        )
    }) {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: Option<&str>, config: Option<&str>) -> AgentRegistryRecord {
        AgentRegistryRecord {
            id: "worker".into(),
            name: "worker".into(),
            tier: "worker".into(),
            driver_kind: kind.map(str::to_string),
            executable: Some("agent".into()),
            runtime_version: None,
            driver_args_json: Some("[]".into()),
            max_concurrency: Some(1),
            tags_json: None,
            driver_config_json: config.map(str::to_string),
        }
    }

    fn factory() -> DriverFactory {
        DriverFactory::new(
            std::env::temp_dir().join("driver_factory_unit.db"),
            std::env::temp_dir(),
        )
    }

    /// The test binary itself: an existing file, so the Codex driver's
    /// "RAS MCP executable must exist" guard is satisfied.
    fn existing_file() -> String {
        std::env::current_exe()
            .expect("current test executable")
            .display()
            .to_string()
    }

    /// The factory's rejection for `records`. `build`'s `Ok` type carries trait
    /// objects, so the error is extracted by matching instead of `unwrap_err`.
    fn build_error(records: &[AgentRegistryRecord]) -> DriverFactoryError {
        match factory().build(records) {
            Ok(_) => panic!("expected the factory to reject the record"),
            Err(error) => error,
        }
    }

    #[test]
    fn defaults_and_known_options_parse() {
        let options = parse_agent_options("worker", None).unwrap();
        assert_eq!(options.timeout_seconds, None);
        assert!(options.artifact_paths.is_empty());
        let options = parse_agent_options(
            "worker",
            Some(r#"{"auth_method":null,"timeout_seconds":7,"max_prompt_bytes":100,"max_result_bytes":200,"artifact_paths":["a.txt","nested/b.txt"],"model":"gpt","overrides":["x=1"]}"#),
        )
        .unwrap();
        assert_eq!(options.auth_method, None);
        assert_eq!(options.timeout_seconds, Some(7));
        assert_eq!(options.max_prompt_bytes, Some(100));
        assert_eq!(options.max_result_bytes, Some(200));
        assert_eq!(options.artifact_paths, vec!["a.txt", "nested/b.txt"]);
        assert_eq!(options.model.as_deref(), Some("gpt"));
        assert_eq!(options.overrides, vec!["x=1"]);
        assert!(parse_agent_options("worker", Some("[1,2]")).is_err());
        assert!(parse_agent_options("worker", Some("not json")).is_err());
        assert!(parse_agent_options("worker", Some(r#"{"timeout_seconds":-1}"#)).is_err());
        assert!(parse_agent_options("worker", Some(r#"{"timeout_seconds":"7"}"#)).is_err());
    }

    #[test]
    fn secret_looking_keys_are_refused() {
        for raw in [
            r#"{"api_key":"x"}"#,
            r#"{"API_TOKEN":"x"}"#,
            r#"{"client_secret":"x"}"#,
            r#"{"db_password":"x"}"#,
            r#"{"endpoint_url":"x"}"#,
        ] {
            let error = parse_agent_options("worker", Some(raw)).unwrap_err();
            assert!(
                matches!(error, DriverFactoryError::SecretLikeConfigKey { .. }),
                "{raw} was accepted: {error}"
            );
        }
    }

    #[test]
    fn unsupported_and_missing_kinds_fail_closed() {
        let error = build_error(&[record(None, None)]);
        assert!(matches!(error, DriverFactoryError::MissingDriverKind(_)));
        for kind in ["native", "cli", "gpt-5"] {
            let error = build_error(&[record(Some(kind), None)]);
            assert!(
                matches!(error, DriverFactoryError::UnsupportedDriverKind { .. }),
                "{kind} was accepted"
            );
        }
    }

    #[test]
    fn every_agent_gets_exactly_one_driver() {
        let codex_config = format!(r#"{{"mcp_command":{:?}}}"#, existing_file());
        let drivers = factory()
            .build(&[
                record(Some("acp"), Some(r#"{"timeout_seconds":1}"#)),
                AgentRegistryRecord {
                    id: "lead".into(),
                    ..record(Some("codex-app-server"), Some(&codex_config))
                },
            ])
            .unwrap();
        assert_eq!(drivers.keys().collect::<Vec<_>>(), vec!["lead", "worker"]);
    }

    #[test]
    fn codex_requires_an_mcp_command() {
        let error = build_error(&[record(Some("codex-app-server"), None)]);
        assert!(matches!(
            error,
            DriverFactoryError::InvalidDriverConfig { .. }
        ));
    }

    #[test]
    fn artifact_paths_must_stay_inside_the_repository() {
        for raw in [
            r#"{"artifact_paths":["/etc/passwd"]}"#,
            r#"{"artifact_paths":["../outside.txt"]}"#,
            r#"{"artifact_paths":[""]}"#,
            r#"{"artifact_paths":["./inside.txt"]}"#,
        ] {
            let error = build_error(&[record(Some("acp"), Some(raw))]);
            assert!(
                matches!(error, DriverFactoryError::InvalidArtifactPath { .. }),
                "{raw} was accepted"
            );
        }
        factory()
            .build(&[record(
                Some("acp"),
                Some(r#"{"artifact_paths":["nested/inside.txt"]}"#),
            )])
            .unwrap();
    }

    #[test]
    fn invalid_argv_is_reported() {
        let mut row = record(Some("acp"), None);
        row.driver_args_json = Some(r#"{"not":"an array"}"#.into());
        let error = build_error(&[row]);
        assert!(matches!(
            error,
            DriverFactoryError::InvalidDriverArgs { .. }
        ));
    }

    #[test]
    fn a_missing_executable_is_reported() {
        let mut row = record(Some("acp"), None);
        row.executable = None;
        let error = build_error(&[row]);
        assert!(matches!(error, DriverFactoryError::MissingExecutable(_)));
    }
}
