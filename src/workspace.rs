use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub name: String,
    pub path: PathBuf,
}

#[derive(serde::Deserialize)]
struct WorkspaceFile {
    folders: Vec<Folder>,
}

#[derive(serde::Deserialize)]
struct Folder {
    path: String,
}

pub fn load_workspace(file: &Path) -> anyhow::Result<Vec<Repo>> {
    let text =
        std::fs::read_to_string(file).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
    let parsed: WorkspaceFile =
        json5::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
    let base = file.parent().unwrap_or(Path::new("."));
    Ok(parsed
        .folders
        .into_iter()
        .map(|folder| {
            let path = base.join(&folder.path);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(folder.path);
            Repo { name, path }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_jsonc_with_comments_tabs_and_trailing_commas() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("w.code-workspace");
        std::fs::write(
            &file,
            "{\n  // the repos\n  \"folders\": [\n\t{\n\t\t\"path\": \"home\"\n\t},\n\t{\n\t\t\"path\": \"projects/api\",\n\t},\n],\n  \"settings\": {}\n}\n",
        )
        .unwrap();

        let repos = load_workspace(&file).unwrap();

        assert_eq!(
            repos,
            vec![
                Repo {
                    name: "home".into(),
                    path: dir.path().join("home"),
                },
                Repo {
                    name: "api".into(),
                    path: dir.path().join("projects/api"),
                },
            ]
        );
    }

    #[test]
    fn missing_folders_key_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("w.code-workspace");
        std::fs::write(&file, "{}").unwrap();

        assert!(load_workspace(&file).is_err());
    }
}
