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
    fn retired_commands_are_rejected_before_opening_state() {
        for command in [
            "advanced",
            "register",
            "registry",
            "run-acp",
            "continue-acp",
            "run-team",
            "resume-team",
            "submit",
            "cancel",
            "override",
            "recover",
            "recover-all",
            "resume",
            "binding",
            "__internal",
        ] {
            assert!(run(&[command, ":memory:"]).is_err(), "{command}");
        }
    }
}
