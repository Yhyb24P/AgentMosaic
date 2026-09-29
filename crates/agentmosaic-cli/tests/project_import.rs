//! Explicit schema import through the project command, without in-place migration.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_am"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "am_import_cli_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../agentmosaic-storage/tests/fixtures/v0_3_0_state.db")
}
#[test]
fn legacy_project_requires_explicit_import_and_preserves_source() {
    let root = root();
    std::fs::create_dir(root.join(".agentmosaic")).unwrap();
    let source = root.join(".agentmosaic/state.db");
    std::fs::copy(fixture(), &source).unwrap();
    let before = std::fs::read(&source).unwrap();
    let dest = root.join(".agentmosaic/state-v14.db");
    for args in [vec!["status"], vec!["init"]] {
        let refusal = run(&root, &args);
        assert!(!refusal.status.success());
        assert!(String::from_utf8_lossy(&refusal.stderr).contains("am import"));
        assert!(!dest.exists());
    }
    let imported = run(&root, &["import", ".agentmosaic/state.db"]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert_eq!(std::fs::read(&source).unwrap(), before);
    let target = std::fs::read(&dest).unwrap();
    assert!(run(&root, &["status", "--all", "--json"]).status.success());
    assert!(!run(&root, &["import", ".agentmosaic/state.db"])
        .status
        .success());
    assert_eq!(std::fs::read(&dest).unwrap(), target);
    assert_eq!(std::fs::read(&source).unwrap(), before);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn import_before_init_resolves_the_git_root() {
    let root = root();
    assert!(Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&root)
        .status()
        .unwrap()
        .success());
    let nested = root.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let imported = run(&nested, &["import", fixture().to_str().unwrap()]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(root.join(".agentmosaic/state-v14.db").exists());
    assert!(!nested.join(".agentmosaic").exists());
    assert!(std::fs::read_to_string(root.join(".gitignore"))
        .unwrap()
        .contains("/.agentmosaic/"));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn unsupported_import_creates_no_database() {
    let root = root();
    let source = root.join("future.db");
    let conn = rusqlite::Connection::open(&source).unwrap();
    conn.pragma_update(None, "user_version", 999).unwrap();
    drop(conn);
    let before = std::fs::read(&source).unwrap();
    let output = run(&root, &["import", "future.db"]);
    assert!(!output.status.success());
    assert!(!root.join(".agentmosaic/state-v14.db").exists());
    assert_eq!(std::fs::read(&source).unwrap(), before);
    std::fs::remove_dir_all(root).unwrap();
}
