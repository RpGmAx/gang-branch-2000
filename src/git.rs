use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use anyhow::bail;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changed: u32,
    pub files: Vec<Change>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Branches {
    pub local: Vec<String>,
    pub remote_only: Vec<String>,
}

const GIT_TIMEOUT: Duration = Duration::from_secs(120);

fn git(repo: &Path, args: &[&str]) -> anyhow::Result<String> {
    run_git(repo, args, GIT_TIMEOUT)
}

fn run_git(repo: &Path, args: &[&str], timeout: Duration) -> anyhow::Result<String> {
    run_program("git", repo, args, timeout)
}

/// Runs a command without a terminal to prompt on, killing its whole group on timeout.
pub(crate) fn run_program(
    program: &str,
    repo: &Path,
    args: &[&str],
    timeout: Duration,
) -> anyhow::Result<String> {
    if !repo.is_dir() {
        bail!("Dossier introuvable : {}", repo.display());
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Background polling must not take .git/index.lock away from the user's own git commands.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    detach(&mut command);
    let mut child = command.spawn().map_err(|e| anyhow::anyhow!("git: {e}"))?;
    let stdout = read_to_string_in_thread(child.stdout.take());
    let stderr = read_to_string_in_thread(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            kill_tree(&mut child);
            let _ = child.wait();
            bail!(
                "{program} {} : délai dépassé ({} s)",
                args.join(" "),
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if status.success() {
        return Ok(stdout);
    }
    // Some failures (merge conflicts, diffs) only explain themselves on stdout.
    let message = [stderr.trim(), stdout.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if message.is_empty() {
        bail!("{program} {} : échec ({status})", args.join(" "));
    }
    bail!(message)
}

/// A new session leaves ssh no terminal to prompt on, and lets a timeout kill the whole group.
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
}

/// Without it, a GUI app flashes a console window for every git call.
#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(unix)]
fn kill_tree(child: &mut std::process::Child) {
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
}

#[cfg(windows)]
fn kill_tree(child: &mut std::process::Child) {
    let mut taskkill = Command::new("taskkill");
    taskkill.args(["/T", "/F", "/PID", &child.id().to_string()]);
    detach(&mut taskkill);
    if !taskkill.status().is_ok_and(|s| s.success()) {
        let _ = child.kill();
    }
}

fn read_to_string_in_thread(
    pipe: Option<impl Read + Send + 'static>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

pub fn parse_status_v2(out: &str) -> RepoStatus {
    let mut status = RepoStatus::default();
    for line in out.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            status.branch = (head != "(detached)").then(|| head.to_owned());
        } else if let Some(upstream) = line.strip_prefix("# branch.upstream ") {
            status.upstream = Some(upstream.to_owned());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for part in ab.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    status.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    status.behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(change) = parse_change(line) {
            status.changed += 1;
            status.files.push(change);
        }
    }
    status
}

fn parse_change(line: &str) -> Option<Change> {
    let (path, kind) = match line.split_once(' ')? {
        ("?", path) => (path, ChangeKind::Untracked),
        ("u", rest) => (rest.splitn(10, ' ').nth(9)?, ChangeKind::Conflicted),
        ("2", rest) => {
            let path = rest.splitn(9, ' ').nth(8)?;
            (path.split('\t').next()?, ChangeKind::Renamed)
        }
        ("1", rest) => {
            let xy = rest.get(..2)?;
            let code = xy.chars().rev().find(|&c| c != '.')?;
            let kind = match code {
                'A' => ChangeKind::Added,
                'D' => ChangeKind::Deleted,
                'R' | 'C' => ChangeKind::Renamed,
                _ => ChangeKind::Modified,
            };
            (rest.splitn(8, ' ').nth(7)?, kind)
        }
        _ => return None,
    };
    Some(Change {
        path: path.to_owned(),
        kind,
    })
}

pub fn parse_numstat(out: &str) -> HashMap<String, (u32, u32)> {
    out.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let added = parts.next()?.parse().ok()?;
            let removed = parts.next()?.parse().ok()?;
            Some((parts.next()?.to_owned(), (added, removed)))
        })
        .collect()
}

/// Lines added and removed per tracked file, staged or not.
pub fn line_counts(repo: &Path) -> anyhow::Result<HashMap<String, (u32, u32)>> {
    git(
        repo,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "HEAD",
            "--numstat",
            "--no-renames",
        ],
    )
    .map(|out| parse_numstat(&out))
}

pub fn parse_branches(out: &str) -> Branches {
    let mut local = Vec::new();
    let mut remote = Vec::new();
    for line in out.lines() {
        if let Some(name) = line.strip_prefix("refs/heads/") {
            local.push(name.to_owned());
        } else if let Some(full) = line.strip_prefix("refs/remotes/")
            && let Some((_, name)) = full.split_once('/')
            && name != "HEAD"
        {
            remote.push((full.to_owned(), name.to_owned()));
        }
    }
    let remote_only = remote
        .into_iter()
        .filter(|(_, name)| !local.contains(name))
        .map(|(full, _)| full)
        .collect();
    Branches { local, remote_only }
}

pub fn toplevel(dir: &Path) -> anyhow::Result<PathBuf> {
    git(dir, &["rev-parse", "--show-toplevel"]).map(|out| PathBuf::from(out.trim_end()))
}

pub fn status(repo: &Path) -> anyhow::Result<RepoStatus> {
    git(
        repo,
        &[
            "-c",
            "core.quotePath=false",
            "status",
            "--porcelain=v2",
            "--branch",
        ],
    )
    .map(|out| parse_status_v2(&out))
}

pub fn branches(repo: &Path) -> anyhow::Result<Branches> {
    git(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .map(|out| parse_branches(&out))
}

pub fn fetch(repo: &Path) -> anyhow::Result<()> {
    git(repo, &["fetch", "--prune"]).map(drop)
}

/// When the repo was last fetched, from `FETCH_HEAD`, which every fetch rewrites.
pub fn last_fetch(repo: &Path) -> Option<SystemTime> {
    let path = git(repo, &["rev-parse", "--git-path", "FETCH_HEAD"]).ok()?;
    std::fs::metadata(repo.join(path.trim_end()))
        .and_then(|m| m.modified())
        .ok()
}

pub fn sync(repo: &Path) -> anyhow::Result<()> {
    let status = status(repo)?;
    let Some(branch) = status.branch else {
        bail!("Impossible de synchroniser : HEAD détachée");
    };
    if status.upstream.is_none() {
        return git(repo, &["push", "-u", "origin", &branch]).map(drop);
    }
    git(repo, &["pull"])?;
    git(repo, &["push"]).map(drop)
}

pub fn switch(repo: &Path, target: &str) -> anyhow::Result<()> {
    let is_local = git(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{target}"),
        ],
    )
    .is_ok();
    if is_local {
        git(repo, &["switch", target]).map(drop)
    } else {
        git(repo, &["switch", "--track", target]).map(drop)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// Every checkout `git worktree list --porcelain` reports, the main one first.
pub fn parse_worktrees(out: &str) -> Vec<Worktree> {
    out.split("\n\n")
        .filter(|block| !block.lines().any(|l| l == "bare"))
        .filter_map(|block| {
            let mut lines = block.lines();
            let path = lines.next()?.strip_prefix("worktree ")?;
            let branch = lines
                .find_map(|l| l.strip_prefix("branch refs/heads/"))
                .map(str::to_owned);
            Some(Worktree {
                path: PathBuf::from(path),
                branch,
            })
        })
        .collect()
}

/// The linked worktrees, without the main checkout.
pub fn worktrees(repo: &Path) -> anyhow::Result<Vec<Worktree>> {
    git(repo, &["worktree", "list", "--porcelain"])
        .map(|out| parse_worktrees(&out).into_iter().skip(1).collect())
}

/// A worktree branch checked out in the main checkout, recorded in its local config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Borrow {
    pub branch: String,
    pub worktree: PathBuf,
    previous: String,
}

fn config_get(repo: &Path, key: &str) -> anyhow::Result<Option<String>> {
    let value = git(repo, &["config", "--local", "--get", "--default", "", key])?;
    let value = value.trim_end();
    Ok((!value.is_empty()).then(|| value.to_owned()))
}

const LOAN: &str = "gang-branch-2000.borrow";

pub fn borrowed(main: &Path) -> anyhow::Result<Option<Borrow>> {
    let Some(branch) = config_get(main, &format!("{LOAN}.branch"))? else {
        return Ok(None);
    };
    Ok(Some(Borrow {
        branch,
        worktree: config_get(main, &format!("{LOAN}.worktree"))?
            .unwrap_or_default()
            .into(),
        previous: config_get(main, &format!("{LOAN}.previous"))?.unwrap_or_default(),
    }))
}

/// Checks the worktree's branch out in the main checkout, detaching the worktree.
pub fn borrow(main: &Path, worktree: &Path) -> anyhow::Result<()> {
    if let Some(current) = borrowed(main)? {
        bail!(
            "{} est déjà dans le checkout principal : rendez-la d'abord.",
            current.branch
        );
    }
    let Some(branch) = status(worktree)?.branch else {
        bail!("Ce worktree n'est sur aucune branche.");
    };
    let previous = match status(main)?.branch {
        Some(name) => format!("branch:{name}"),
        None => format!("commit:{}", git(main, &["rev-parse", "HEAD"])?.trim_end()),
    };
    git(worktree, &["switch", "--detach"])?;
    if let Err(e) = git(main, &["switch", &branch]) {
        let _ = git(worktree, &["switch", &branch]);
        return Err(e);
    }
    let worktree = worktree.to_string_lossy();
    for (key, value) in [
        ("branch", branch.as_str()),
        ("worktree", &worktree),
        ("previous", &previous),
    ] {
        git(
            main,
            &["config", "--local", &format!("{LOAN}.{key}"), value],
        )?;
    }
    Ok(())
}

/// Puts the main checkout back where it was and the branch back in its worktree.
/// Safe to retry: a main checkout that already left the branch is not switched again.
pub fn give_back(main: &Path) -> anyhow::Result<()> {
    let Some(loan) = borrowed(main)? else {
        bail!("Aucune branche de worktree dans le checkout principal.");
    };
    if status(main)?.branch.as_deref() == Some(loan.branch.as_str()) {
        match loan.previous.split_once(':') {
            Some(("branch", name)) => git(main, &["switch", name])?,
            Some(("commit", sha)) => git(main, &["switch", "--detach", sha])?,
            _ => bail!("Branche d'origine du checkout principal inconnue."),
        };
    }
    git(&loan.worktree, &["switch", &loan.branch])?;
    git(main, &["config", "--local", "--remove-section", LOAN]).map(drop)
}

/// The latest of the last commit and the uncommitted edits, to tell an old worktree.
pub fn last_activity(repo: &Path, files: &[Change]) -> Option<SystemTime> {
    let committed = git(repo, &["log", "-1", "--format=%ct"])
        .ok()
        .and_then(|out| out.trim().parse::<u64>().ok())
        .map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
    let edited = files.iter().filter_map(|change| {
        std::fs::metadata(repo.join(&change.path))
            .ok()?
            .modified()
            .ok()
    });
    committed.into_iter().chain(edited).max()
}

/// Deleting node_modules and build output can take a while.
const REMOVE_TIMEOUT: Duration = Duration::from_secs(600);

/// Removes a linked worktree; its branch stays. `force` also drops uncommitted changes.
pub fn remove_worktree(main: &Path, worktree: &Path, force: bool) -> anyhow::Result<()> {
    if let Some(loan) = borrowed(main)?
        && loan.worktree == worktree
    {
        bail!(
            "{} est dans le checkout principal : rendez-la avant de supprimer le worktree.",
            loan.branch
        );
    }
    let path = worktree.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path);
    run_git(main, &args, REMOVE_TIMEOUT).map(drop)
}

pub fn remote_url(repo: &Path) -> Option<String> {
    git(repo, &["remote", "get-url", "origin"])
        .ok()
        .map(|out| out.trim().to_owned())
        .filter(|url| !url.is_empty())
}

/// `-D`: squash merges leave the branch's own commits out of master, so `-d` would refuse.
pub fn delete_branch(main: &Path, branch: &str) -> anyhow::Result<()> {
    git(main, &["branch", "-D", branch]).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_upstream_and_counts() {
        let out = "# branch.oid 1234\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -3\n1 .M N... 100644 100644 100644 aa bb src/a.rs\n? notes.txt\n";
        assert_eq!(
            parse_status_v2(out),
            RepoStatus {
                branch: Some("main".into()),
                upstream: Some("origin/main".into()),
                ahead: 2,
                behind: 3,
                changed: 2,
                files: vec![
                    Change {
                        path: "src/a.rs".into(),
                        kind: ChangeKind::Modified,
                    },
                    Change {
                        path: "notes.txt".into(),
                        kind: ChangeKind::Untracked,
                    },
                ],
            }
        );
    }

    #[test]
    fn parses_each_kind_of_change_and_paths_with_spaces() {
        let out = "# branch.head main\n\
1 A. N... 000000 100644 100644 00 aa new file.rs\n\
1 .D N... 100644 100644 000000 aa aa gone.rs\n\
2 R. N... 100644 100644 100644 aa aa R100 to.rs\tfrom.rs\n\
u UU N... 100644 100644 100644 100644 aa bb cc both.rs\n\
1 M. N... 100644 100644 100644 aa bb staged.rs\n";
        let files: Vec<_> = parse_status_v2(out)
            .files
            .into_iter()
            .map(|c| (c.path, c.kind))
            .collect();
        assert_eq!(
            files,
            vec![
                ("new file.rs".into(), ChangeKind::Added),
                ("gone.rs".into(), ChangeKind::Deleted),
                ("to.rs".into(), ChangeKind::Renamed),
                ("both.rs".into(), ChangeKind::Conflicted),
                ("staged.rs".into(), ChangeKind::Modified),
            ]
        );
    }

    #[test]
    fn parses_numstat_and_skips_binary_files() {
        let out = "3\t1\tsrc/a.rs\n-\t-\tlogo.png\n0\t12\tdir/b c.rs\n";
        let counts = parse_numstat(out);
        assert_eq!(counts.get("src/a.rs"), Some(&(3, 1)));
        assert_eq!(counts.get("dir/b c.rs"), Some(&(0, 12)));
        assert_eq!(counts.get("logo.png"), None);
    }

    fn sh_git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo_with_commit() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        sh_git(p, &["init", "-q", "-b", "main"]);
        sh_git(p, &["config", "user.name", "T"]);
        sh_git(p, &["config", "user.email", "t@example.com"]);
        sh_git(p, &["config", "commit.gpgsign", "false"]);
        std::fs::write(p.join("f"), "x").unwrap();
        sh_git(p, &["add", "f"]);
        sh_git(p, &["commit", "-q", "-m", "c"]);
        dir
    }

    #[test]
    fn status_does_not_rewrite_the_index() {
        let dir = repo_with_commit();
        // Same content, new mtime: a plain `git status` would refresh and rewrite the index.
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(dir.path().join("f"), "x").unwrap();
        let before = std::fs::read(dir.path().join(".git/index")).unwrap();

        status(dir.path()).unwrap();

        assert_eq!(
            std::fs::read(dir.path().join(".git/index")).unwrap(),
            before
        );
    }

    #[cfg(unix)]
    #[test]
    fn hung_command_is_killed_after_the_timeout() {
        let dir = repo_with_commit();
        sh_git(
            dir.path(),
            &["config", "core.sshCommand", "sh -c 'sleep 30'"],
        );
        sh_git(
            dir.path(),
            &["remote", "add", "origin", "ssh://example.invalid/x.git"],
        );
        let started = std::time::Instant::now();

        let err = run_git(dir.path(), &["fetch"], Duration::from_secs(1))
            .unwrap_err()
            .to_string();

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(err.contains("délai dépassé"), "error: {err}");
    }

    #[test]
    fn failure_with_empty_stderr_reports_stdout() {
        let dir = repo_with_commit();

        // `git diff --exit-code` fails with the diff on stdout and nothing on stderr.
        std::fs::write(dir.path().join("f"), "changed").unwrap();
        let err = git(dir.path(), &["diff", "--exit-code"])
            .unwrap_err()
            .to_string();

        assert!(err.contains("+changed"), "error: {err}");
    }

    #[test]
    fn parses_detached_head() {
        let out = "# branch.oid 1234\n# branch.head (detached)\n";
        assert_eq!(parse_status_v2(out).branch, None);
    }

    #[test]
    fn status_without_upstream() {
        let out = "# branch.oid 1234\n# branch.head feat\n";
        let status = parse_status_v2(out);
        assert_eq!(status.branch.as_deref(), Some("feat"));
        assert_eq!(status.upstream, None);
        assert_eq!((status.ahead, status.behind), (0, 0));
    }

    #[test]
    fn parses_branches() {
        let out = "refs/heads/main\nrefs/heads/feat/x\nrefs/remotes/origin/HEAD\nrefs/remotes/origin/main\nrefs/remotes/origin/other\n";
        assert_eq!(
            parse_branches(out),
            Branches {
                local: vec!["main".into(), "feat/x".into()],
                remote_only: vec!["origin/other".into()],
            }
        );
    }
}
