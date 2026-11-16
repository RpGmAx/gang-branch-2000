use std::path::{Path, PathBuf};

use crate::workspace::Repo;

#[derive(serde::Serialize, serde::Deserialize)]
struct Config {
    source: PathBuf,
}

pub fn config_file() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_default()
        .join("gang-branch-2000/config.json")
}

pub fn load_saved(config: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(config).ok()?;
    serde_json::from_str::<Config>(&text).ok().map(|c| c.source)
}

pub fn save(config: &Path, source: &Path) -> anyhow::Result<()> {
    if let Some(dir) = config.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(&Config {
        source: source.to_owned(),
    })?;
    std::fs::write(config, text)?;
    Ok(())
}

/// A file is read as a VS Code workspace; a folder resolves to the git repo containing it.
pub fn load_repos(source: &Path) -> anyhow::Result<Vec<Repo>> {
    if source.is_file() {
        return crate::workspace::load_workspace(source);
    }
    let root = crate::git::toplevel(source)
        .map_err(|_| anyhow::anyhow!("Ce dossier n'est pas un dépôt git."))?;
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());
    Ok(vec![Repo { name, path: root }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git_init(dir: &Path) {
        let ok = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok);
    }

    #[test]
    fn saved_source_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("sub/config.json");

        save(&config, Path::new("/home/me/repo")).unwrap();

        assert_eq!(load_saved(&config), Some(PathBuf::from("/home/me/repo")));
    }

    #[test]
    fn no_saved_source_when_config_is_missing() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(load_saved(&dir.path().join("config.json")), None);
    }

    #[test]
    fn git_folder_is_a_single_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("myrepo");
        std::fs::create_dir(&repo).unwrap();
        git_init(&repo);

        assert_eq!(
            load_repos(&repo).unwrap(),
            vec![Repo {
                name: "myrepo".into(),
                path: repo.canonicalize().unwrap(),
            }]
        );
    }

    #[test]
    fn subfolder_of_a_repo_resolves_to_its_root() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("myrepo");
        std::fs::create_dir_all(repo.join("src/deep")).unwrap();
        git_init(&repo);

        let repos = load_repos(&repo.join("src/deep")).unwrap();

        assert_eq!(repos[0].path, repo.canonicalize().unwrap());
    }

    #[test]
    fn non_git_folder_errors() {
        let dir = tempfile::tempdir().unwrap();

        let err = load_repos(dir.path()).unwrap_err().to_string();

        assert_eq!(err, "Ce dossier n'est pas un dépôt git.");
    }

    #[test]
    fn workspace_file_lists_its_folders() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("w.code-workspace");
        std::fs::write(&file, r#"{"folders": [{"path": "a"}, {"path": "b"}]}"#).unwrap();

        let names: Vec<_> = load_repos(&file)
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect();

        assert_eq!(names, ["a", "b"]);
    }
}
