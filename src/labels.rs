use std::time::Duration;

use crate::git::{ChangeKind, RepoStatus};

pub fn branch_label(status: &RepoStatus) -> String {
    status
        .branch
        .clone()
        .unwrap_or_else(|| "(HEAD détachée)".into())
}

pub fn matches_filter(branch: &str, filter: &str) -> bool {
    branch.to_lowercase().contains(&filter.to_lowercase())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Clean,
    Behind,
    Ahead,
    Unpublished,
    Detached,
    Error,
}

pub fn tone(status: &Result<RepoStatus, String>) -> Tone {
    let Ok(status) = status else {
        return Tone::Error;
    };
    match (&status.branch, &status.upstream) {
        (None, _) => Tone::Detached,
        (Some(_), None) => Tone::Unpublished,
        _ if status.behind > 0 => Tone::Behind,
        _ if status.ahead > 0 => Tone::Ahead,
        _ => Tone::Clean,
    }
}

pub fn sync_label(status: &RepoStatus) -> String {
    if status.branch.is_some() && status.upstream.is_none() {
        return "Publier".into();
    }
    match (status.behind, status.ahead) {
        (0, 0) => "À jour".into(),
        (behind, 0) => format!("Sync ↓{behind}"),
        (0, ahead) => format!("Sync ↑{ahead}"),
        (behind, ahead) => format!("Sync ↓{behind} ↑{ahead}"),
    }
}

fn count(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn details(status: &RepoStatus) -> Vec<String> {
    let mut parts = Vec::new();
    if status.branch.is_some() && status.upstream.is_none() {
        parts.push("branche non publiée".into());
    }
    if status.behind > 0 {
        parts.push(count(
            status.behind,
            "commit à récupérer",
            "commits à récupérer",
        ));
    }
    if status.ahead > 0 {
        parts.push(count(status.ahead, "commit à envoyer", "commits à envoyer"));
    }
    parts
}

pub fn files_label(n: u32) -> String {
    count(n, "fichier modifié", "fichiers modifiés")
}

pub fn worktrees_label(n: u32) -> String {
    count(n, "worktree", "worktrees")
}

pub fn change_label(kind: ChangeKind) -> (&'static str, &'static str) {
    match kind {
        ChangeKind::Modified => ("M", "modifié"),
        ChangeKind::Added => ("A", "ajouté"),
        ChangeKind::Deleted => ("D", "supprimé"),
        ChangeKind::Renamed => ("R", "renommé"),
        ChangeKind::Untracked => ("N", "nouveau, non suivi"),
        ChangeKind::Conflicted => ("!", "en conflit"),
    }
}

fn ago(what: &str, elapsed: Duration) -> String {
    let minutes = elapsed.as_secs() / 60;
    match minutes {
        0 => format!("{what} à l'instant"),
        1..60 => format!("{what} il y a {minutes} min"),
        60..1440 => format!("{what} il y a {} h", minutes / 60),
        _ => format!("{what} il y a {} j", minutes / 1440),
    }
}

pub fn fetched_ago(elapsed: Duration) -> String {
    ago("fetch", elapsed)
}

pub fn touched_ago(elapsed: Duration) -> String {
    ago("modifié", elapsed)
}

/// A worktree untouched for two weeks is probably done with or forgotten.
pub fn is_stale(elapsed: Duration) -> bool {
    elapsed > Duration::from_secs(14 * 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked(ahead: u32, behind: u32) -> RepoStatus {
        RepoStatus {
            branch: Some("main".into()),
            upstream: Some("origin/main".into()),
            ahead,
            behind,
            ..Default::default()
        }
    }

    #[test]
    fn detached_head_label() {
        let status = RepoStatus {
            branch: None,
            upstream: None,
            ..tracked(0, 0)
        };
        assert_eq!(branch_label(&status), "(HEAD détachée)");
    }

    #[test]
    fn filter_is_case_insensitive_substring() {
        assert!(matches_filter("TICKET-1234-devices", "ticket-12"));
        assert!(matches_filter("main", ""));
        assert!(!matches_filter("main", "feat"));
    }

    #[test]
    fn tone_by_state() {
        assert_eq!(tone(&Ok(tracked(0, 0))), Tone::Clean);
        assert_eq!(tone(&Ok(tracked(0, 2))), Tone::Behind);
        // Sync pulls first, so a diverged branch reads as behind.
        assert_eq!(tone(&Ok(tracked(1, 2))), Tone::Behind);
        assert_eq!(tone(&Ok(tracked(1, 0))), Tone::Ahead);
        let unpublished = RepoStatus {
            upstream: None,
            ..tracked(0, 0)
        };
        assert_eq!(tone(&Ok(unpublished)), Tone::Unpublished);
        let detached = RepoStatus {
            branch: None,
            upstream: None,
            ..tracked(0, 0)
        };
        assert_eq!(tone(&Ok(detached)), Tone::Detached);
        assert_eq!(tone(&Err("x".into())), Tone::Error);
    }

    #[test]
    fn sync_label_carries_the_counts() {
        assert_eq!(sync_label(&tracked(0, 0)), "À jour");
        assert_eq!(sync_label(&tracked(0, 2)), "Sync ↓2");
        assert_eq!(sync_label(&tracked(1, 0)), "Sync ↑1");
        assert_eq!(sync_label(&tracked(1, 2)), "Sync ↓2 ↑1");
        let unpublished = RepoStatus {
            upstream: None,
            ..tracked(0, 0)
        };
        assert_eq!(sync_label(&unpublished), "Publier");
    }

    #[test]
    fn details_spell_out_the_state_in_french() {
        let status = RepoStatus {
            changed: 1,
            ..tracked(3, 1)
        };
        assert_eq!(
            details(&status),
            vec!["1 commit à récupérer", "3 commits à envoyer"]
        );
        let dirty = RepoStatus {
            changed: 4,
            ..tracked(0, 0)
        };
        // Files and worktrees get their own toggles in the window.
        assert!(details(&dirty).is_empty());
        let unpublished = RepoStatus {
            upstream: None,
            ..tracked(0, 0)
        };
        assert_eq!(details(&unpublished), vec!["branche non publiée"]);
        let detached = RepoStatus {
            branch: None,
            upstream: None,
            ..tracked(0, 0)
        };
        // The branch line already says it.
        assert!(details(&detached).is_empty());
        assert!(details(&tracked(0, 0)).is_empty());
    }

    #[test]
    fn fetched_ago_rounds_down() {
        use std::time::Duration;
        assert_eq!(fetched_ago(Duration::from_secs(20)), "fetch à l'instant");
        assert_eq!(fetched_ago(Duration::from_secs(150)), "fetch il y a 2 min");
        assert_eq!(
            fetched_ago(Duration::from_secs(3 * 3600 + 59)),
            "fetch il y a 3 h"
        );
        assert_eq!(
            fetched_ago(Duration::from_secs(50 * 3600)),
            "fetch il y a 2 j"
        );
    }

    #[test]
    fn plural_counts() {
        assert_eq!(files_label(1), "1 fichier modifié");
        assert_eq!(files_label(4), "4 fichiers modifiés");
        assert_eq!(worktrees_label(1), "1 worktree");
        assert_eq!(worktrees_label(7), "7 worktrees");
    }

    #[test]
    fn change_letters_and_words() {
        use crate::git::ChangeKind;
        assert_eq!(change_label(ChangeKind::Modified), ("M", "modifié"));
        assert_eq!(
            change_label(ChangeKind::Untracked),
            ("N", "nouveau, non suivi")
        );
        assert_eq!(change_label(ChangeKind::Conflicted), ("!", "en conflit"));
    }

    #[test]
    fn worktree_age() {
        use std::time::Duration;
        let day = 86_400;
        assert_eq!(
            touched_ago(Duration::from_secs(150)),
            "modifié il y a 2 min"
        );
        assert_eq!(
            touched_ago(Duration::from_secs(40 * day)),
            "modifié il y a 40 j"
        );
        assert!(!is_stale(Duration::from_secs(13 * day)));
        assert!(is_stale(Duration::from_secs(15 * day)));
    }
}
