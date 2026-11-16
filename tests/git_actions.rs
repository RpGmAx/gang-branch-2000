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
    origin: PathBuf,
    a: PathBuf,
    b: PathBuf,
}

fn configure(clone: &Path) {
    run(clone, &["config", "user.name", "Test"]);
    run(clone, &["config", "user.email", "test@example.com"]);
    run(clone, &["config", "commit.gpgsign", "false"]);
    // Without it, git refuses to pull divergent branches: tests need a deterministic choice.
    run(clone, &["config", "pull.rebase", "false"]);
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    run(
        dir.path(),
        &["init", "-q", "--bare", "-b", "main", "origin.git"],
    );
    run(dir.path(), &["clone", "-q", "origin.git", "a"]);
    configure(&a);
    commit(&a, "README", "hello");
    run(&a, &["push", "-q", "-u", "origin", "main"]);
    run(dir.path(), &["clone", "-q", "origin.git", "b"]);
    configure(&b);
    Fixture {
        _dir: dir,
        origin,
        a,
        b,
    }
}

fn commit(repo: &Path, file: &str, content: &str) {
    std::fs::write(repo.join(file), content).unwrap();
    run(repo, &["add", file]);
    run(repo, &["commit", "-q", "-m", file]);
}

#[test]
fn fetch_then_status_shows_behind() {
    let f = fixture();
    commit(&f.b, "b.txt", "b");
    run(&f.b, &["push", "-q"]);

    git::fetch(&f.a).unwrap();

    assert_eq!(git::status(&f.a).unwrap().behind, 1);
}

#[test]
fn sync_pulls_and_pushes() {
    let f = fixture();
    commit(&f.a, "a.txt", "a");
    commit(&f.b, "b.txt", "b");
    run(&f.b, &["push", "-q"]);
    git::fetch(&f.a).unwrap();

    git::sync(&f.a).unwrap();

    let status = git::status(&f.a).unwrap();
    assert_eq!((status.ahead, status.behind), (0, 0));
    let log = run(&f.origin, &["log", "--format=%s", "main"]);
    assert!(log.lines().any(|l| l == "a.txt"), "origin log: {log}");
    assert!(f.a.join("b.txt").exists());
}

#[test]
fn sync_conflict_reports_the_conflicting_file() {
    let f = fixture();
    commit(&f.a, "README", "from a");
    commit(&f.b, "README", "from b");
    run(&f.b, &["push", "-q"]);

    let err = git::sync(&f.a).unwrap_err().to_string();

    assert!(err.contains("README"), "error: {err}");
}

#[test]
fn sync_publishes_branch_without_upstream() {
    let f = fixture();
    run(&f.a, &["switch", "-q", "-c", "new"]);

    git::sync(&f.a).unwrap();

    assert_eq!(
        git::status(&f.a).unwrap().upstream.as_deref(),
        Some("origin/new")
    );
}

#[test]
fn switch_to_remote_only_creates_tracking_branch() {
    let f = fixture();
    run(&f.b, &["switch", "-q", "-c", "x"]);
    run(&f.b, &["push", "-q", "-u", "origin", "x"]);
    git::fetch(&f.a).unwrap();
    assert_eq!(git::branches(&f.a).unwrap().remote_only, vec!["origin/x"]);

    git::switch(&f.a, "origin/x").unwrap();

    let status = git::status(&f.a).unwrap();
    assert_eq!(status.branch.as_deref(), Some("x"));
    assert_eq!(status.upstream.as_deref(), Some("origin/x"));
}

#[test]
fn switch_to_local_branch() {
    let f = fixture();
    run(&f.a, &["branch", "other"]);

    git::switch(&f.a, "other").unwrap();

    assert_eq!(git::status(&f.a).unwrap().branch.as_deref(), Some("other"));
}

#[test]
fn switch_with_conflicting_changes_errors() {
    let f = fixture();
    run(&f.b, &["switch", "-q", "-c", "y"]);
    commit(&f.b, "README", "changed on y");
    run(&f.b, &["push", "-q", "-u", "origin", "y"]);
    git::fetch(&f.a).unwrap();
    std::fs::write(f.a.join("README"), "local edit").unwrap();

    let err = git::switch(&f.a, "origin/y").unwrap_err().to_string();

    // git's message is localized; the file name is not.
    assert!(err.contains("README"), "error: {err}");
}

#[test]
fn sync_on_detached_head_errors() {
    let f = fixture();
    run(&f.a, &["switch", "-q", "--detach"]);

    let err = git::sync(&f.a).unwrap_err().to_string();

    assert_eq!(err, "Impossible de synchroniser : HEAD détachée");
}

#[test]
fn status_on_non_repo_errors() {
    let dir = tempfile::tempdir().unwrap();

    assert!(git::status(dir.path()).is_err());
}

#[test]
fn fetch_unreachable_remote_errors() {
    let f = fixture();
    run(
        &f.a,
        &["remote", "set-url", "origin", "/nonexistent/repo.git"],
    );

    let err = git::fetch(&f.a).unwrap_err().to_string();

    assert!(!err.is_empty());
}

#[test]
fn last_fetch_is_set_by_a_fetch() {
    let f = fixture();
    assert_eq!(git::last_fetch(&f.a), None);

    git::fetch(&f.a).unwrap();

    let at = git::last_fetch(&f.a).unwrap();
    assert!(at.elapsed().unwrap() < std::time::Duration::from_secs(60));
}

#[test]
fn status_on_missing_folder_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone");

    let err = git::status(&missing).unwrap_err().to_string();

    assert_eq!(err, format!("Dossier introuvable : {}", missing.display()));
}
