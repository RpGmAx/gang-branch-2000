use std::path::{Path, PathBuf};
use std::process::Command;

use gang_branch_2000::git;

fn run(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    main: PathBuf,
    wt: PathBuf,
}

fn commit(repo: &Path, file: &str, content: &str) {
    std::fs::write(repo.join(file), content).unwrap();
    run(repo, &["add", file]);
    run(repo, &["commit", "-q", "-m", file]);
}

/// A main checkout on `main` and a linked worktree on `feat`, one commit ahead.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    let wt = dir.path().join("wt");
    run(dir.path(), &["init", "-q", "-b", "main", "main"]);
    run(&main, &["config", "user.name", "Test"]);
    run(&main, &["config", "user.email", "test@example.com"]);
    run(&main, &["config", "commit.gpgsign", "false"]);
    commit(&main, "README", "hello");
    run(
        &main,
        &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
    );
    commit(&wt, "feat.txt", "feat");
    Fixture {
        _dir: dir,
        main,
        wt,
    }
}

fn branch(repo: &Path) -> Option<String> {
    git::status(repo).unwrap().branch
}

#[test]
fn lists_linked_worktrees_only() {
    let f = fixture();
    let detached = f.main.parent().unwrap().join("detached");
    run(
        &f.main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            detached.to_str().unwrap(),
        ],
    );

    let worktrees = git::worktrees(&f.main).unwrap();

    // git sorts linked worktrees by path.
    assert_eq!(
        worktrees,
        vec![
            git::Worktree {
                path: detached.canonicalize().unwrap(),
                branch: None,
            },
            git::Worktree {
                path: f.wt.canonicalize().unwrap(),
                branch: Some("feat".into()),
            },
        ]
    );
}

#[test]
fn borrow_moves_the_branch_into_the_main_checkout() {
    let f = fixture();

    git::borrow(&f.main, &f.wt).unwrap();

    assert_eq!(branch(&f.main).as_deref(), Some("feat"));
    assert_eq!(branch(&f.wt), None);
    assert!(f.main.join("feat.txt").exists());
    let borrow = git::borrowed(&f.main).unwrap().unwrap();
    assert_eq!(borrow.branch, "feat");
    assert_eq!(borrow.worktree, f.wt);
}

#[test]
fn give_back_restores_both_checkouts() {
    let f = fixture();
    git::borrow(&f.main, &f.wt).unwrap();
    // Tested and fixed in the main checkout: the worktree must get that commit back.
    commit(&f.main, "fix.txt", "fix");

    git::give_back(&f.main).unwrap();

    assert_eq!(branch(&f.main).as_deref(), Some("main"));
    assert_eq!(branch(&f.wt).as_deref(), Some("feat"));
    assert!(f.wt.join("fix.txt").exists());
    assert_eq!(git::borrowed(&f.main).unwrap(), None);
}

#[test]
fn give_back_returns_a_detached_main_checkout_to_its_commit() {
    let f = fixture();
    run(&f.main, &["switch", "-q", "--detach"]);
    let before = run(&f.main, &["rev-parse", "HEAD"]);

    git::borrow(&f.main, &f.wt).unwrap();
    git::give_back(&f.main).unwrap();

    assert_eq!(branch(&f.main), None);
    assert_eq!(run(&f.main, &["rev-parse", "HEAD"]), before);
}

#[test]
fn failed_borrow_leaves_the_worktree_on_its_branch() {
    let f = fixture();
    // feat.txt exists on feat: an untracked one in main blocks the switch.
    std::fs::write(f.main.join("feat.txt"), "in the way").unwrap();

    let err = git::borrow(&f.main, &f.wt).unwrap_err().to_string();

    assert!(err.contains("feat.txt"), "error: {err}");
    assert_eq!(branch(&f.main).as_deref(), Some("main"));
    assert_eq!(branch(&f.wt).as_deref(), Some("feat"));
    assert_eq!(git::borrowed(&f.main).unwrap(), None);
}

#[test]
fn only_one_loan_at_a_time() {
    let f = fixture();
    let other = f.main.parent().unwrap().join("other");
    run(
        &f.main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "other",
            other.to_str().unwrap(),
        ],
    );
    git::borrow(&f.main, &f.wt).unwrap();

    let err = git::borrow(&f.main, &other).unwrap_err().to_string();

    assert!(err.contains("feat"), "error: {err}");
    assert_eq!(branch(&other).as_deref(), Some("other"));
}

#[test]
fn borrow_of_a_detached_worktree_errors() {
    let f = fixture();
    run(&f.wt, &["switch", "-q", "--detach"]);

    assert!(git::borrow(&f.main, &f.wt).is_err());
}

#[test]
fn line_counts_cover_staged_and_unstaged_changes() {
    let f = fixture();
    std::fs::write(f.main.join("README"), "hello\nworld\n").unwrap();
    std::fs::write(f.main.join("staged.txt"), "a\nb\nc\n").unwrap();
    run(&f.main, &["add", "staged.txt"]);

    let counts = git::line_counts(&f.main).unwrap();

    assert_eq!(counts.get("README"), Some(&(2, 1)));
    assert_eq!(counts.get("staged.txt"), Some(&(3, 0)));
}

fn commit_dated(repo: &Path, file: &str, date: &str) {
    std::fs::write(repo.join(file), file).unwrap();
    run(repo, &["add", file]);
    let ok = Command::new("git")
        .args(["commit", "-q", "-m", file])
        .env("GIT_COMMITTER_DATE", date)
        .current_dir(repo)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

#[test]
fn last_activity_is_the_last_commit_of_a_clean_worktree() {
    let f = fixture();
    commit_dated(&f.wt, "old.txt", "2026-01-01T00:00:00Z");
    let status = git::status(&f.wt).unwrap();

    let at = git::last_activity(&f.wt, &status.files).unwrap();

    let expected = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_767_225_600);
    assert_eq!(at, expected);
}

#[test]
fn last_activity_counts_uncommitted_edits() {
    let f = fixture();
    commit_dated(&f.wt, "old.txt", "2026-01-01T00:00:00Z");
    std::fs::write(f.wt.join("old.txt"), "edited").unwrap();
    let status = git::status(&f.wt).unwrap();

    let at = git::last_activity(&f.wt, &status.files).unwrap();

    assert!(at.elapsed().unwrap() < std::time::Duration::from_secs(60));
}

#[test]
fn remove_worktree_keeps_the_branch() {
    let f = fixture();

    git::remove_worktree(&f.main, &f.wt, false).unwrap();

    assert!(!f.wt.exists());
    assert!(git::worktrees(&f.main).unwrap().is_empty());
    assert!(
        git::branches(&f.main)
            .unwrap()
            .local
            .contains(&"feat".to_string())
    );
}

#[test]
fn remove_dirty_worktree_needs_force() {
    let f = fixture();
    std::fs::write(f.wt.join("draft.txt"), "wip").unwrap();

    assert!(git::remove_worktree(&f.main, &f.wt, false).is_err());
    assert!(f.wt.exists());

    git::remove_worktree(&f.main, &f.wt, true).unwrap();
    assert!(!f.wt.exists());
}

#[test]
fn lent_worktree_cannot_be_removed() {
    let f = fixture();
    git::borrow(&f.main, &f.wt).unwrap();

    let err = git::remove_worktree(&f.main, &f.wt, true)
        .unwrap_err()
        .to_string();

    assert!(err.contains("feat"), "error: {err}");
    assert!(f.wt.exists());
}

#[test]
fn delete_branch_after_removing_its_worktree() {
    let f = fixture();
    assert!(
        git::delete_branch(&f.main, "feat").is_err(),
        "checked out in the worktree"
    );

    git::remove_worktree(&f.main, &f.wt, false).unwrap();
    git::delete_branch(&f.main, "feat").unwrap();

    assert!(
        !git::branches(&f.main)
            .unwrap()
            .local
            .contains(&"feat".to_string())
    );
}

#[test]
fn remote_url_of_origin() {
    let f = fixture();
    assert_eq!(git::remote_url(&f.main), None);
    run(
        &f.main,
        &["remote", "add", "origin", "git@github.com:o/r.git"],
    );

    assert_eq!(
        git::remote_url(&f.main).as_deref(),
        Some("git@github.com:o/r.git")
    );
}

#[test]
fn a_loan_is_recorded_under_the_app_name() {
    let f = fixture();

    git::borrow(&f.main, &f.wt).unwrap();

    let config = run(&f.main, &["config", "--local", "--list"]);
    assert!(
        config.contains("gang-branch-2000.borrow.branch=feat"),
        "{config}"
    );
}
