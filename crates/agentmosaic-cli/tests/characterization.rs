use std::process::Command;
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}
#[test]
fn version_and_help_are_plain_text() {
    let version = cli().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout),
        format!("am {}\n", env!("CARGO_PKG_VERSION"))
    );
    for args in [vec!["--help"], vec!["help"], vec!["help", "run"]] {
        let output = cli().args(args).output().unwrap();
        assert!(output.status.success());
        assert!(!output.stdout.is_empty());
        assert!(!output.stdout.contains(&0x1b));
    }
    let bare = cli().output().unwrap();
    assert_eq!(bare.status.code(), Some(2));
    assert!(bare.stdout.is_empty());
}
#[test]
fn invalid_invocations_fail_before_state_access() {
    for args in [
        vec!["unknown"],
        vec!["run", "--resume", "invalid"],
        vec!["final", "database.db", "1"],
        vec!["artifact", "database.db", "1"],
        vec!["agent", "add"],
    ] {
        let output = cli().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
    }
}
