use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Draft,
    Merged,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pr {
    pub number: u32,
    pub state: PrState,
    pub url: String,
}

pub fn web_url(remote: &str) -> Option<String> {
    let path = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("https://github.com/"))
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))?;
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    Some(format!("https://github.com/{path}"))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPr {
    head_ref_name: String,
    is_draft: bool,
    number: u32,
    state: String,
    url: String,
}

/// `gh pr list` output, newest first: the first PR of a branch is the one that counts.
pub fn parse_prs(json: &str) -> anyhow::Result<HashMap<String, Pr>> {
    let mut prs = HashMap::new();
    for pr in serde_json::from_str::<Vec<GhPr>>(json)? {
        let state = match pr.state.as_str() {
            "MERGED" => PrState::Merged,
            "CLOSED" => PrState::Closed,
            _ if pr.is_draft => PrState::Draft,
            _ => PrState::Open,
        };
        prs.entry(pr.head_ref_name).or_insert(Pr {
            number: pr.number,
            state,
            url: pr.url,
        });
    }
    Ok(prs)
}

/// The user's own PRs on the repo, by head branch. Needs `gh`, logged in.
pub fn prs(repo: &Path) -> anyhow::Result<HashMap<String, Pr>> {
    let out = crate::git::run_program(
        "gh",
        repo,
        &[
            "pr",
            "list",
            "--author",
            "@me",
            "--state",
            "all",
            "--limit",
            "200",
            "--json",
            "headRefName,isDraft,number,state,url",
        ],
        Duration::from_secs(30),
    )?;
    parse_prs(&out)
}

pub fn pr_label(pr: &Pr) -> String {
    let state = match pr.state {
        PrState::Open => "ouverte",
        PrState::Draft => "brouillon",
        PrState::Merged => "mergée",
        PrState::Closed => "fermée",
    };
    format!("PR #{} {state}", pr.number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_url_from_every_remote_form() {
        let expected = Some("https://github.com/octo-org/octo-app".to_owned());
        assert_eq!(web_url("git@github.com:octo-org/octo-app.git"), expected);
        assert_eq!(
            web_url("https://github.com/octo-org/octo-app.git"),
            expected
        );
        assert_eq!(web_url("https://github.com/octo-org/octo-app"), expected);
        assert_eq!(
            web_url("ssh://git@github.com/octo-org/octo-app.git"),
            expected
        );
        assert_eq!(web_url("/srv/git/local.git"), None);
    }

    #[test]
    fn parse_prs_keeps_the_newest_per_branch() {
        let json = r#"[
            {"headRefName":"feat/x","isDraft":false,"number":12,"state":"OPEN","url":"u12"},
            {"headRefName":"feat/x","isDraft":false,"number":3,"state":"CLOSED","url":"u3"},
            {"headRefName":"wip","isDraft":true,"number":10,"state":"OPEN","url":"u10"},
            {"headRefName":"done","isDraft":false,"number":9,"state":"MERGED","url":"u9"}
        ]"#;

        let prs = parse_prs(json).unwrap();

        assert_eq!(
            prs["feat/x"],
            Pr {
                number: 12,
                state: PrState::Open,
                url: "u12".into()
            }
        );
        assert_eq!(prs["wip"].state, PrState::Draft);
        assert_eq!(prs["done"].state, PrState::Merged);
    }

    #[test]
    fn pr_labels() {
        assert_eq!(
            pr_label(&Pr {
                number: 9,
                state: PrState::Merged,
                url: String::new()
            }),
            "PR #9 mergée"
        );
        assert_eq!(
            pr_label(&Pr {
                number: 9,
                state: PrState::Draft,
                url: String::new()
            }),
            "PR #9 brouillon"
        );
        assert_eq!(
            pr_label(&Pr {
                number: 9,
                state: PrState::Open,
                url: String::new()
            }),
            "PR #9 ouverte"
        );
        assert_eq!(
            pr_label(&Pr {
                number: 9,
                state: PrState::Closed,
                url: String::new()
            }),
            "PR #9 fermée"
        );
    }
}
