//! Removed entrypoints fail at parsing, before touching a user database.
use std::process::Command;
#[test]
fn retired_surface_is_not_callable() {
    let missing =
        std::env::temp_dir().join(format!("am_retired_surface_{}.db", std::process::id()));
    assert!(!missing.exists());
    for name in [
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
        let output = Command::new(env!("CARGO_BIN_EXE_am"))
            .arg(name)
            .arg(&missing)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{name}");
        assert!(output.stdout.is_empty(), "{name}");
        assert!(!missing.exists(), "{name} created state");
    }
}
