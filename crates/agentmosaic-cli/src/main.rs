//! Small Rust normal-path CLI for the durable team board.
//!
//! Parsing comes first and has no side effects: a command name that clap does
//! not recognize fails before any database is opened or created.

mod args;
mod commands;
mod json;
mod output;
mod project;
mod render;
mod target;

use clap::Parser;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() {
        // Bare `am` is a usage error on stderr, never help on stdout.
        eprint!("{}", args::help_text());
        std::process::exit(2);
    }
    let cli = match args::Cli::try_parse_from(std::iter::once("am".to_string()).chain(argv)) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    let command = cli.command;
    if let args::Command::Events {
        target,
        json,
        follow: true,
    } = command
    {
        if let Err(error) = commands::events::follow(target, json, std::io::stdout()) {
            eprintln!("{error}");
            std::process::exit(2);
        }
        return;
    }
    match commands::dispatch_status(command) {
        Ok(dispatch) => {
            if let Some(payload) = dispatch.payload {
                println!("{payload}");
            }
            if dispatch.exit != 0 {
                std::process::exit(dispatch.exit);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    /// The internal contract every handler still honours: `Ok` payload to
    /// stdout with exit 0, `Err` payload to stderr with exit 2.
    fn run(args: &[&str]) -> Result<String, String> {
        let cli =
            crate::args::Cli::try_parse_from(std::iter::once("am").chain(args.iter().copied()))
                .map_err(|error| error.to_string())?;
        crate::commands::dispatch(cli.command).map(|payload| payload.unwrap_or_default())
    }

    #[test]
    fn rejects_unknown_command() {
        assert!(run(&["unknown", ":memory:"]).is_err());
    }

    #[test]
    fn normal_usage_does_not_advertise_the_internal_bridge() {
        let help = crate::args::help_text();
        assert!(!help.contains("__internal"));
        assert!(!help.contains("codex-mcp"));
    }

    #[test]
    fn registry_rejects_zero_concurrency_before_persisting_it() {
        let result = run(&[
            "register", ":memory:", "worker", "worker", "worker", "acp", "qwen", "--acp", "0", "-",
        ]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("greater than zero"));
    }

    #[test]
    fn run_acp_persists_invalid_configuration_as_failed() {
        let database = std::env::temp_dir().join(format!(
            "agentmosaic_cli_invalid_acp_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = database.to_string_lossy().into_owned();
        run(&[
            "register", &db, "worker", "worker", "worker", "acp", "qwen", "--acp", "1", "-",
        ])
        .unwrap();
        let submitted = run(&["submit", &db, "bulk", "bounded"]).unwrap();
        let task = submitted.strip_prefix("submitted task=").unwrap();
        let result = run(&[
            "run-acp",
            &db,
            task,
            "worker",
            &std::env::temp_dir().to_string_lossy(),
            "-",
            "30",
            "../outside",
        ]);
        assert!(result.is_err());
        let status = run(&["status", &db]).unwrap();
        assert!(status.contains("status=failed"));
        let _ = std::fs::remove_file(database);
    }

    #[test]
    fn continue_acp_rejects_source_without_a_completed_binding() {
        let database = std::env::temp_dir().join(format!(
            "agentmosaic_cli_continue_reject_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = database.to_string_lossy().into_owned();
        run(&[
            "register", &db, "worker", "worker", "worker", "acp", "qwen", "--acp", "1", "-",
        ])
        .unwrap();
        let source = run(&["submit", &db, "bulk", "source"]).unwrap();
        let next = run(&["submit", &db, "bulk", "next"]).unwrap();
        let source_id = source.strip_prefix("submitted task=").unwrap();
        let next_id = next.strip_prefix("submitted task=").unwrap();
        let result = run(&[
            "continue-acp",
            &db,
            next_id,
            "worker",
            source_id,
            &std::env::temp_dir().to_string_lossy(),
            "-",
            "30",
        ]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no external binding"));
        let status = run(&["status", &db]).unwrap();
        assert!(status.contains(&format!("task={next_id} status=pending")));
        let _ = std::fs::remove_file(database);
    }
}
