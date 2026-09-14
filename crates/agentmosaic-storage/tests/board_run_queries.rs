//! The board's run queries: root reasoning tasks and the descendants of one
//! root. These are the reads the `am` inspection surface resolves a project's
//! runs with, so their selection rules are pinned here.

use agentmosaic_storage::SqliteTaskBoard;
use agentmosaic_team::{TaskBoard, TaskKind};

fn board() -> SqliteTaskBoard {
    SqliteTaskBoard::in_memory().expect("board")
}

#[test]
fn root_tasks_sees_no_roots_when_every_task_has_a_parent_or_is_not_reasoning() {
    let mut board = board();
    // A bulk task with no parent is not a run.
    board
        .create_task("bulk work", None, TaskKind::Bulk, None)
        .expect("bulk root");
    board
        .create_task("utility work", None, TaskKind::Utility, None)
        .expect("utility root");
    // A reasoning task that hangs off another task is not a root.
    board
        .create_task("reasoning child", Some(1), TaskKind::Reasoning, None)
        .expect("reasoning child");

    assert!(board.root_tasks().expect("root tasks").is_empty());
    assert!(board.latest_root_task().expect("latest").is_none());
}

#[test]
fn root_tasks_lists_only_root_reasoning_tasks_in_id_order() {
    let mut board = board();
    let bulk = board
        .create_task("bulk root", None, TaskKind::Bulk, None)
        .expect("bulk root");
    let first = board
        .create_task("first run", None, TaskKind::Reasoning, None)
        .expect("first run");
    board
        .create_task("child of the first run", Some(first), TaskKind::Bulk, None)
        .expect("child");
    let second = board
        .create_task("second run", None, TaskKind::Reasoning, None)
        .expect("second run");
    let third = board
        .create_task("third run", None, TaskKind::Reasoning, None)
        .expect("third run");

    let roots: Vec<u64> = board
        .root_tasks()
        .expect("root tasks")
        .into_iter()
        .map(|task| task.id)
        .collect();
    assert_eq!(roots, vec![first, second, third]);
    assert!(!roots.contains(&bulk));
    assert!(board
        .root_tasks()
        .expect("root tasks")
        .iter()
        .all(|task| task.kind == TaskKind::Reasoning && task.parent_task.is_none()));
}

#[test]
fn latest_root_task_is_the_highest_root_id_and_never_a_lower_one() {
    let mut board = board();
    let older = board
        .create_task("older run", None, TaskKind::Reasoning, None)
        .expect("older run");
    board
        .set_status(older, agentmosaic_team::TaskStatus::Succeeded)
        .expect("succeed the older run");
    let newer = board
        .create_task("newer run", None, TaskKind::Reasoning, None)
        .expect("newer run");
    board
        .set_status(newer, agentmosaic_team::TaskStatus::Failed)
        .expect("fail the newer run");

    // A failed newer run still wins: the query selects the highest root id and
    // never prefers an older succeeded one.
    let latest = board
        .latest_root_task()
        .expect("latest")
        .expect("a run exists");
    assert_eq!(latest.id, newer);
    assert_eq!(latest.status, agentmosaic_team::TaskStatus::Failed);
}

#[test]
fn descendants_of_follows_parent_chains_deeper_than_one_level() {
    let mut board = board();
    let root = board
        .create_task("run", None, TaskKind::Reasoning, None)
        .expect("root");
    let child = board
        .create_task("child", Some(root), TaskKind::Bulk, None)
        .expect("child");
    let grandchild = board
        .create_task("grandchild", Some(child), TaskKind::Tool, None)
        .expect("grandchild");
    let great_grandchild = board
        .create_task("great grandchild", Some(grandchild), TaskKind::Bulk, None)
        .expect("great grandchild");

    let found = board.descendants_of(root).expect("descendants");
    assert_eq!(found, vec![child, grandchild, great_grandchild]);
    assert!(!found.contains(&root));
}

#[test]
fn descendants_of_ignores_chains_that_never_reach_the_root() {
    let mut board = board();
    let root = board
        .create_task("run", None, TaskKind::Reasoning, None)
        .expect("root");
    let other_root = board
        .create_task("another run", None, TaskKind::Reasoning, None)
        .expect("other root");
    let other_child = board
        .create_task("another child", Some(other_root), TaskKind::Bulk, None)
        .expect("other child");
    let orphaned = board
        .create_task("never parented", None, TaskKind::Bulk, None)
        .expect("orphan");
    let orphan_child = board
        .create_task("child of the orphan", Some(orphaned), TaskKind::Bulk, None)
        .expect("orphan child");
    let own_child = board
        .create_task("own child", Some(root), TaskKind::Bulk, None)
        .expect("own child");

    assert_eq!(
        board.descendants_of(root).expect("descendants"),
        vec![own_child]
    );
    assert_eq!(
        board.descendants_of(9999).expect("unknown root"),
        Vec::<u64>::new()
    );
    let mut other = board.descendants_of(other_root).expect("other descendants");
    other.sort_unstable();
    assert_eq!(other, vec![other_child]);
    assert!(!board
        .descendants_of(other_root)
        .expect("other descendants")
        .contains(&orphan_child));
}

#[test]
fn descendants_of_keeps_two_run_subtrees_isolated() {
    let mut board = board();
    let first = board
        .create_task("first run", None, TaskKind::Reasoning, None)
        .expect("first run");
    let first_child = board
        .create_task("first child", Some(first), TaskKind::Bulk, None)
        .expect("first child");
    let first_grandchild = board
        .create_task("first grandchild", Some(first_child), TaskKind::Tool, None)
        .expect("first grandchild");
    let second = board
        .create_task("second run", None, TaskKind::Reasoning, None)
        .expect("second run");
    let second_child = board
        .create_task("second child", Some(second), TaskKind::Tool, None)
        .expect("second child");

    let first_subtree = board.descendants_of(first).expect("first subtree");
    let second_subtree = board.descendants_of(second).expect("second subtree");
    assert_eq!(first_subtree, vec![first_child, first_grandchild]);
    assert_eq!(second_subtree, vec![second_child]);
    for id in &second_subtree {
        assert!(!first_subtree.contains(id));
    }
    for id in &first_subtree {
        assert!(!second_subtree.contains(id));
    }
}
