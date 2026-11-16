# Gang Branch 2000 — design

## Why

Opening VS Code only to check or change the branch, fetch and sync a handful of
repos is heavy. This app replaces that use, so VS Code no longer needs to be opened
for it.

## Goals

- For each repo, show the branch, the commits behind and ahead of its
  upstream (↓/↑), and the number of modified files.
- Change the branch, Fetch, and Sync, like VS Code does.
- Show the worktrees of each repo and let one be tested in the main checkout.

## Non-goals

- Committing, staging, and viewing a diff (VS Code stays the tool for that).
- Managing repos other than the ones listed in the workspace file.

## Anti-goals

- Doing something different from what VS Code would do. Every action runs the
  same `git` command, with the user's own git config.
- Hanging on a credentials prompt: `GIT_TERMINAL_PROMPT=0`, and the git error is
  shown as is.

## Approaches considered

1. **Rust + egui (eframe), a single binary** — chosen. Small, quick to write, and
   the window opens in under a second. The rendering isn't KDE-native.
2. Rust + GTK4 — looks more like the system, but in GNOME style under KDE, with
   about twice as much code.
3. A Plasma widget (QML) — always visible, but painful to debug, and outside Rust.

Option 1 was picked. A tray icon came first, then went: the window is the app.

## Design

- **Source** (added 2026-11-16): on first launch the window asks
  for a git folder or a `.code-workspace` file (native picker through the XDG portal).
  The choice is kept in `~/.config/gang-branch-2000/config.json`; « Changer… » at the top of
  the window picks another one. A folder gives one
  repo (the git root containing it); a workspace gives its `folders[].path` (JSONC,
  relative to the file), in VS Code order. `--source <path>` forces a source without
  touching the config.
- **Name, launch, icon** (2026-11-16): shown as « Gang Branch 2000 »
  (app id, crate, binary and config folder `gang-branch-2000`; earlier « Branché », then
  « Merge Simpson »). Icon `gangbranch2000<hash of the SVG>`: no dash,
  since icon lookup falls back on the part before a dash, and a hash, since KDE caches
  icons by name and kept showing a former design under the same name. The icon: a big head in sunglasses whose body is a
  git branch, two smaller ones behind it, on an orange-to-pink sunset (a first version
  with three heads on stems looked like a candlestick at taskbar size). No tray icon: the app is its window,
  closing it quits, and the window fetches every repo on opening and every 5 min. Each
  AppImage launch writes `~/.local/share/applications/gang-branch-2000.desktop` and its
  icons, removes those of the former names and runs `kbuildsycoca6` when it wrote
  something, since winit ignores window icons on Wayland and KWin reads them from that
  file. Renderer: glow, without accesskit nor the system font scan (first frame ~45 ms
  instead of ~180 ms).
- **Packaging** (2026-11-16): `scripts/package.sh [linux|windows|macos|all]`. Linux: the
  AppImage. Windows: a single `.exe`, no installer, icon and name embedded by `build.rs`
  (winresource), no console window, git spawned with `CREATE_NO_WINDOW` and killed with
  `taskkill /T` on timeout; cross-built from Linux with mingw-w64. macOS: a universal
  `.app` (lipo of arm64 and x86_64) in a `.dmg`, ad-hoc signed, not notarized; Homebrew
  folders are added to `PATH` since Finder launches get a bare one. Only buildable on a
  Mac, hence `.github/workflows/package.yml` running each build on its own system.
  Icons for all three come from `scripts/make-icons.sh`.
- **Worktree age and removal**: a worktree shows « modifié il y a … », the later of
  its last commit and its uncommitted edits, in amber after 14 days. The trash button
  asks for confirmation, says the branch is kept and how many uncommitted files will
  be lost, then runs `git worktree remove` (`--force` when dirty). A lent worktree
  cannot be removed.
- **GitHub** (2026-11-16): the repo name opens `origin` on GitHub. `gh pr list --author
  @me --state all` (every 2 min, newest PR per branch) gives PR badges on the current
  branch and on each worktree, clickable. Removing a worktree can also delete its
  branch (`git branch -D`, since squash merges defeat `-d`), ticked by default when
  its PR is merged and flagged in red otherwise. The icon is installed as PNGs in `hicolor/{48,128,256}x…/apps`: KWin and
  plasmashell list icon folders when they start and showed « ? » for an icon in a
  `scalable` folder created afterwards (an absolute `Icon=` path did not help either).
- **Window** (French labels, redesigned 2026-11-16): one lane per
  repo. A 4 px stripe on the left gives the state: sage up to date, amber behind,
  cyan ahead, violet unpublished, red error. The repo name sits small above the branch,
  which is the largest text (Fira Sans Condensed, since branch names are long ticket
  slugs) and opens the branch picker: local branches, then remote branches with no
  local branch (choosing one creates the tracking branch), with a filter field. A
  detail line says what is going on (« 2 commits à récupérer », « 1 fichier modifié »,
  « fetch il y a 2 min », from the mtime of `FETCH_HEAD`). On the right, Fetch is an
  icon button and Sync is the main button: filled with the state colour and carrying
  the counts (« Sync ↓2 », « Sync ↑1 », « Publier »), or a quiet « À jour » when there
  is nothing to do. A single header line shows the source, « Changer » and « Tout
  fetch »; there is no refresh button, status refreshes every 5 s and after each
  action. Fira Sans (OFL) is embedded; dark and light palettes follow the system
  theme. Git errors are shown under the lane.
- **Changed files and worktrees** (added 2026-11-16): in the
  detail line, « N fichiers modifiés » and « N worktrees » unfold under the lane. The
  file list shows a status letter, the path and `+a −r` (`git diff HEAD --numstat`);
  clicking a file opens it with `xdg-open`. Each linked worktree (`git worktree list`)
  shows its folder, branch and state, with « Tester ici »: the worktree is detached
  and the main checkout switches to its branch, since the local stack serves the main
  checkout. Uncommitted changes stay in the worktree (said in the button's hover).
  The loan is recorded in the repo's local config (`gang-branch-2000.borrow.*`: branch,
  worktree, previous branch or commit of the main checkout); « Rendre » puts the main
  checkout back where it was and reattaches the worktree. One loan at a time per repo.
  If the main checkout cannot switch, the worktree is reattached and the error shown.
- **git commands** (same as VS Code):
  - Fetch: `git fetch --prune`.
  - Sync: `git pull`, then `git push`. With no upstream, it publishes the branch:
    `git push -u origin <branch>`.
  - Branch: `git switch <b>` for a local branch, or `git switch --track origin/<b>`
    for a remote one.
## Left to the builder

- The exact layout and the icons.
- The test split: parsers are covered by unit tests, and git actions are tested
  against temporary repos.
