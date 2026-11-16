// No console window behind the app on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod theme;
mod window;

use std::path::{Path, PathBuf};

use gang_branch_2000::desktop;

fn parse_source() -> anyhow::Result<Option<PathBuf>> {
    let mut source = None;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--source" => {
                let path = it
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--source attend un chemin"))?;
                source = Some(PathBuf::from(path));
            }
            other => anyhow::bail!("argument inconnu : {other}"),
        }
    }
    Ok(source)
}

/// Apps opened from the Finder get a bare PATH, without Homebrew's gh.
#[cfg(target_os = "macos")]
fn add_homebrew_to_path() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if !dirs.iter().any(|d| d == Path::new(extra)) {
            dirs.push(extra.into());
        }
    }
    if let Ok(joined) = std::env::join_paths(dirs) {
        // SAFETY: called first thing in main, before any thread exists.
        unsafe { std::env::set_var("PATH", joined) };
    }
}

fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    add_homebrew_to_path();
    let source = parse_source()?;
    if let (Some(appimage), Some(data)) = (std::env::var_os("APPIMAGE"), dirs::data_dir()) {
        let wrote = desktop::integrate(
            &data,
            Path::new(&appimage),
            &[
                (48, include_bytes!("../assets/icons/gangbranch2000-48.png")),
                (
                    128,
                    include_bytes!("../assets/icons/gangbranch2000-128.png"),
                ),
                (
                    256,
                    include_bytes!("../assets/icons/gangbranch2000-256.png"),
                ),
            ],
        );
        // KDE notices a changed desktop file late, and the taskbar shows its old icon meanwhile.
        if matches!(wrote, Ok(true)) {
            let _ = std::process::Command::new("kbuildsycoca6")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    window::run(source)
}
