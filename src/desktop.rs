use std::path::Path;

pub const APP_ID: &str = "gang-branch-2000";
/// No dash: icon lookup falls back on the part before a dash, and themes may have one.
/// Set by build.rs, with a hash of the SVG.
const ICON_NAME: &str = env!("ICON_NAME");
const ICON_PREFIX: &str = "gangbranch2000";
pub const APP_NAME: &str = "Gang Branch 2000";

/// Ids this app had before: their launcher would show it twice in the menu.
const FORMER_IDS: &[&str] = &["branche", "merge-simpson"];
const FORMER_ICONS: &[&str] = &["branche", "merge-simpson", "mergesimpson"];

/// Desktop Entry spec: a quoted argument escapes `"`, `` ` ``, `$` and `\`, and string
/// values escape the backslash once more.
fn quote_exec(path: &Path) -> String {
    let mut quoted = String::from("\"");
    for c in path.to_string_lossy().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push_str("\\\\");
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

pub fn desktop_entry(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={APP_NAME}\nComment=Branche, Fetch et Sync de vos dépôts git\nExec={}\nIcon={ICON_NAME}\nTerminal=false\nCategories=Development;RevisionControl;\n",
        quote_exec(exec)
    )
}

/// Wayland compositors take a window's icon from the desktop file named after its app id.
/// `icons` are PNGs by size: KWin and Plasma list icon folders when they start, and
/// miss a `scalable` folder created later, while sized folders usually exist already.
/// Returns whether anything was written.
pub fn integrate(data_dir: &Path, exec: &Path, icons: &[(u32, &[u8])]) -> std::io::Result<bool> {
    // Icons go first: the desktop file is what Plasma watches.
    let mut files: Vec<_> = icons
        .iter()
        .map(|(size, png)| {
            (
                data_dir.join(format!("icons/hicolor/{size}x{size}/apps/{ICON_NAME}.png")),
                png.to_vec(),
            )
        })
        .collect();
    files.push((
        data_dir.join(format!("applications/{APP_ID}.desktop")),
        desktop_entry(exec).into_bytes(),
    ));
    for id in FORMER_IDS {
        let _ = std::fs::remove_file(data_dir.join(format!("applications/{id}.desktop")));
    }
    for name in FORMER_ICONS {
        for dir in ["48x48", "128x128", "256x256"] {
            let icon = data_dir.join(format!("icons/hicolor/{dir}/apps/{name}.png"));
            let _ = std::fs::remove_file(icon);
        }
        let icon = data_dir.join(format!("icons/hicolor/scalable/apps/{name}.svg"));
        let _ = std::fs::remove_file(icon);
    }
    for dir in ["48x48", "128x128", "256x256", "scalable"] {
        let Ok(entries) = std::fs::read_dir(data_dir.join(format!("icons/hicolor/{dir}/apps")))
        else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if stem.starts_with(ICON_PREFIX) && stem != ICON_NAME {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    let mut wrote = false;
    for (path, content) in files {
        if std::fs::read(&path).ok().as_deref() == Some(content.as_slice()) {
            continue;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, content)?;
        wrote = true;
    }
    Ok(wrote)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_quotes_the_exec_path() {
        let entry = desktop_entry(Path::new("/home/me/My Apps/gang-branch-2000.AppImage"));
        assert!(
            entry.contains("\nExec=\"/home/me/My Apps/gang-branch-2000.AppImage\"\n"),
            "{entry}"
        );
        assert!(entry.contains("\nName=Gang Branch 2000\n"));
        assert!(entry.contains(&format!("\nIcon={ICON_NAME}\n")));
        // A dash would make icon themes fall back on the part before it (Papirus has « merge »).
        assert!(ICON_NAME.starts_with("gangbranch2000") && !ICON_NAME.contains('-'));
    }

    #[test]
    fn integrate_writes_once_and_follows_a_moved_appimage() {
        let data = tempfile::tempdir().unwrap();
        let icons: &[(u32, &[u8])] = &[(48, b"png48"), (256, b"png256")];

        assert!(
            integrate(
                data.path(),
                Path::new("/a/gang-branch-2000.AppImage"),
                icons
            )
            .unwrap()
        );
        assert!(
            !integrate(
                data.path(),
                Path::new("/a/gang-branch-2000.AppImage"),
                icons
            )
            .unwrap()
        );
        assert!(
            integrate(
                data.path(),
                Path::new("/b/gang-branch-2000.AppImage"),
                icons
            )
            .unwrap()
        );

        let entry =
            std::fs::read_to_string(data.path().join("applications/gang-branch-2000.desktop"))
                .unwrap();
        assert!(entry.contains("Exec=\"/b/gang-branch-2000.AppImage\""));
        // Sized PNG folders: KWin and Plasma, already running, miss a new `scalable` folder.
        let icon = |size| {
            data.path()
                .join(format!("icons/hicolor/{size}x{size}/apps/{ICON_NAME}.png"))
        };
        assert_eq!(std::fs::read(icon(48)).unwrap(), b"png48");
        assert_eq!(std::fs::read(icon(256)).unwrap(), b"png256");
    }

    #[test]
    fn integrate_removes_the_launcher_of_former_names() {
        let data = tempfile::tempdir().unwrap();
        let old = [
            "applications/branche.desktop",
            "icons/hicolor/48x48/apps/branche.png",
            "icons/hicolor/256x256/apps/branche.png",
            "icons/hicolor/scalable/apps/branche.svg",
            "icons/hicolor/48x48/apps/merge-simpson.png",
            "applications/merge-simpson.desktop",
            "icons/hicolor/128x128/apps/mergesimpson.png",
        ];
        for path in old {
            let path = data.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "old").unwrap();
        }

        integrate(data.path(), Path::new("/a/gang-branch-2000.AppImage"), &[]).unwrap();

        for path in old {
            assert!(!data.path().join(path).exists(), "{path}");
        }
        assert!(!integrate(data.path(), Path::new("/a/gang-branch-2000.AppImage"), &[]).unwrap());
    }

    #[test]
    fn integrate_replaces_earlier_versions_of_the_icon() {
        let data = tempfile::tempdir().unwrap();
        let apps = data.path().join("icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&apps).unwrap();
        for old in ["gangbranch2000.png", "gangbranch2000deadbeef.png"] {
            std::fs::write(apps.join(old), "old").unwrap();
        }
        std::fs::write(apps.join("someoneelse.png"), "keep").unwrap();

        integrate(
            data.path(),
            Path::new("/a/gang-branch-2000.AppImage"),
            &[(48, b"new")],
        )
        .unwrap();

        let mut names: Vec<_> = std::fs::read_dir(&apps)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![format!("{ICON_NAME}.png"), "someoneelse.png".into()]
        );
    }
}
