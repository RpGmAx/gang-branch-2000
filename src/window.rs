use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Frame, Layout, Margin, Popup, PopupCloseBehavior,
    Rect, RichText, Sense, Stroke, Ui, pos2, vec2,
};
use gang_branch_2000::git::{self, Borrow, Branches, Change, RepoStatus, Worktree};
use gang_branch_2000::github::{self, Pr, PrState};
use gang_branch_2000::labels::{self, Tone};
use gang_branch_2000::workspace::Repo;
use gang_branch_2000::{desktop, source};

use crate::theme::{self, Palette};

const REFRESH_EVERY: Duration = Duration::from_secs(5);
const FETCH_EVERY: Duration = Duration::from_secs(5 * 60);
const STRIPE: f32 = 4.0;
const LANE_HEIGHT: f32 = 54.0;

enum Msg {
    Status {
        generation: u64,
        index: usize,
        result: Result<Box<Snapshot>, String>,
    },
    ActionDone {
        generation: u64,
        index: usize,
        result: Result<(), String>,
    },
    Picked(Option<PathBuf>),
}

struct Snapshot {
    status: RepoStatus,
    branches: Branches,
    fetched: Option<SystemTime>,
    counts: HashMap<String, (u32, u32)>,
    worktrees: Vec<WorktreeRow>,
    borrow: Option<Borrow>,
    web: Option<String>,
    // None when not asked this time: the previous list stays.
    prs: Option<HashMap<String, Pr>>,
}

struct WorktreeRow {
    tree: Worktree,
    // Only loaded while the list is unfolded: a status per worktree costs a git call each.
    status: Option<RepoStatus>,
    touched: Option<SystemTime>,
}

struct ConfirmRemove {
    index: usize,
    path: PathBuf,
    branch: Option<String>,
    pr: Option<Pr>,
    dirty: u32,
    delete_branch: bool,
}

impl ConfirmRemove {
    fn new(index: usize, wt: &WorktreeRow, prs: &HashMap<String, Pr>) -> Self {
        let pr = wt.tree.branch.as_ref().and_then(|b| prs.get(b)).cloned();
        Self {
            index,
            path: wt.tree.path.clone(),
            branch: wt.tree.branch.clone(),
            delete_branch: pr.as_ref().is_some_and(|pr| pr.state == PrState::Merged),
            pr,
            dirty: wt.status.as_ref().map_or(0, |s| s.changed),
        }
    }
}

/// gh goes over the network: PRs are reloaded far less often than the status.
const PRS_EVERY: Duration = Duration::from_secs(120);

enum Request {
    Fetch,
    Sync,
    Switch(String),
    Borrow(PathBuf),
    GiveBack,
    AskRemove(ConfirmRemove),
    RemoveWorktree {
        path: PathBuf,
        force: bool,
        branch: Option<String>,
    },
    Refresh,
}

enum Pick {
    Folder,
    Workspace,
}

struct Row {
    repo: Repo,
    status: Option<Result<RepoStatus, String>>,
    branches: Branches,
    fetched: Option<SystemTime>,
    counts: HashMap<String, (u32, u32)>,
    worktrees: Vec<WorktreeRow>,
    borrow: Option<Borrow>,
    show_files: bool,
    show_worktrees: bool,
    web: Option<String>,
    prs: HashMap<String, Pr>,
    prs_asked: Option<Instant>,
    loading: bool,
    busy: bool,
    error: Option<String>,
    filter: String,
}

struct App {
    source: Option<PathBuf>,
    persist: bool,
    source_error: Option<String>,
    rows: Vec<Row>,
    // Bumped on every source change so results from the previous source are dropped.
    generation: u64,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    last_refresh: Option<Instant>,
    picking: bool,
    confirm: Option<ConfirmRemove>,
    last_auto_fetch: Option<Instant>,
}

pub fn run(source_override: Option<PathBuf>) -> anyhow::Result<()> {
    let persist = source_override.is_none();
    let initial = source_override.or_else(|| source::load_saved(&source::config_file()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(desktop::APP_NAME)
            .with_app_id(desktop::APP_ID)
            .with_inner_size([760.0, window_height()])
            .with_min_inner_size([560.0, 220.0])
            // Windows, macOS and X11 use it; Wayland takes the desktop file's icon instead.
            .with_icon(egui::IconData {
                rgba: include_bytes!("../assets/icons/gangbranch2000-128.rgba").to_vec(),
                width: 128,
                height: 128,
            }),
        ..Default::default()
    };
    eframe::run_native(
        desktop::APP_NAME,
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            #[cfg(debug_assertions)]
            if let Ok(forced) = std::env::var("GB2K_THEME") {
                cc.egui_ctx.set_theme(match forced.as_str() {
                    "light" => egui::Theme::Light,
                    _ => egui::Theme::Dark,
                });
            }
            let (tx, rx) = channel();
            let mut app = App {
                source: None,
                persist,
                source_error: None,
                rows: Vec::new(),
                generation: 0,
                tx,
                rx,
                last_refresh: None,
                picking: false,
                confirm: None,
                last_auto_fetch: None,
            };
            if let Some(path) = initial {
                app.set_source(path, false);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

fn window_height() -> f32 {
    #[cfg(debug_assertions)]
    if let Some(h) = std::env::var("GB2K_HEIGHT")
        .ok()
        .and_then(|h| h.parse().ok())
    {
        return h;
    }
    380.0
}

impl App {
    fn set_source(&mut self, path: PathBuf, save: bool) {
        match source::load_repos(&path) {
            Ok(repos) => {
                if save
                    && self.persist
                    && let Err(e) = source::save(&source::config_file(), &path)
                {
                    self.source_error = Some(e.to_string());
                    return;
                }
                self.generation += 1;
                self.rows = repos
                    .into_iter()
                    .map(|repo| Row {
                        repo,
                        status: None,
                        branches: Branches::default(),
                        fetched: None,
                        counts: HashMap::new(),
                        worktrees: Vec::new(),
                        borrow: None,
                        show_files: false,
                        show_worktrees: false,
                        web: None,
                        prs: HashMap::new(),
                        prs_asked: None,
                        loading: false,
                        busy: false,
                        error: None,
                        filter: String::new(),
                    })
                    .collect();
                #[cfg(debug_assertions)]
                if std::env::var_os("GB2K_UNFOLD").is_some() {
                    for row in &mut self.rows {
                        (row.show_files, row.show_worktrees) = (true, true);
                    }
                }
                self.source = Some(path);
                self.source_error = None;
                self.last_refresh = None;
            }
            Err(e) => self.source_error = Some(format!("{} : {e}", path.display())),
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.last_refresh = Some(Instant::now());
        for (index, row) in self.rows.iter_mut().enumerate() {
            if row.loading || row.busy {
                continue;
            }
            row.loading = true;
            let path = row.repo.path.clone();
            let (with_counts, with_worktrees) = (row.show_files, row.show_worktrees);
            let with_prs = row.prs_asked.is_none_or(|t| t.elapsed() >= PRS_EVERY);
            if with_prs {
                row.prs_asked = Some(Instant::now());
            }
            let (tx, ctx, generation) = (self.tx.clone(), ctx.clone(), self.generation);
            std::thread::spawn(move || {
                let result = snapshot(&path, with_counts, with_worktrees, with_prs)
                    .map(Box::new)
                    .map_err(|e| e.to_string());
                let _ = tx.send(Msg::Status {
                    generation,
                    index,
                    result,
                });
                ctx.request_repaint();
            });
        }
    }

    fn start(&mut self, ctx: &egui::Context, index: usize, request: Request) {
        let row = &mut self.rows[index];
        if row.busy {
            return;
        }
        row.busy = true;
        row.error = None;
        let path = row.repo.path.clone();
        let (tx, ctx, generation) = (self.tx.clone(), ctx.clone(), self.generation);
        std::thread::spawn(move || {
            let result = match request {
                Request::Fetch => git::fetch(&path),
                Request::Sync => git::sync(&path),
                Request::Switch(target) => git::switch(&path, &target),
                Request::Borrow(worktree) => git::borrow(&path, &worktree),
                Request::GiveBack => git::give_back(&path),
                Request::RemoveWorktree {
                    path: worktree,
                    force,
                    branch,
                } => git::remove_worktree(&path, &worktree, force).and_then(|()| match branch {
                    Some(branch) => git::delete_branch(&path, &branch),
                    None => Ok(()),
                }),
                Request::Refresh | Request::AskRemove(_) => Ok(()),
            }
            .map_err(|e| e.to_string());
            let _ = tx.send(Msg::ActionDone {
                generation,
                index,
                result,
            });
            ctx.request_repaint();
        });
    }

    fn pick(&mut self, ctx: &egui::Context, kind: Pick) {
        self.picking = true;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new();
            let picked = match kind {
                Pick::Folder => dialog.set_title("Choisir un dossier git").pick_folder(),
                Pick::Workspace => dialog
                    .set_title("Choisir un fichier workspace")
                    .add_filter("Workspace VS Code", &["code-workspace"])
                    .pick_file(),
            };
            let _ = tx.send(Msg::Picked(picked));
            ctx.request_repaint();
        });
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Status {
                    generation,
                    index,
                    result,
                } if generation == self.generation => {
                    let row = &mut self.rows[index];
                    row.loading = false;
                    match result {
                        Ok(snap) => {
                            row.status = Some(Ok(snap.status));
                            row.branches = snap.branches;
                            row.fetched = snap.fetched;
                            row.counts = snap.counts;
                            row.worktrees = snap.worktrees;
                            row.borrow = snap.borrow;
                            row.web = snap.web;
                            if let Some(prs) = snap.prs {
                                row.prs = prs;
                            }
                        }
                        Err(e) => row.status = Some(Err(e)),
                    }
                }
                Msg::ActionDone {
                    generation,
                    index,
                    result,
                } if generation == self.generation => {
                    let row = &mut self.rows[index];
                    row.busy = false;
                    row.error = result.err();
                    self.last_refresh = None;
                }
                Msg::Picked(picked) => {
                    self.picking = false;
                    if let Some(path) = picked {
                        self.set_source(path, true);
                    }
                }
                _ => {}
            }
        }
    }

    fn welcome(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() / 2.0 - 110.0).max(24.0));
            ui.label(
                RichText::new(desktop::APP_NAME)
                    .font(FontId::new(30.0, theme::branch()))
                    .color(p.text),
            );
            ui.add_space(6.0);
            ui.set_max_width(420.0);
            ui.label(
                RichText::new(
                    "Choisissez le dépôt git à suivre, ou un fichier .code-workspace pour en suivre plusieurs.",
                )
                .color(p.muted),
            );
            ui.add_space(20.0);
            ui.add_enabled_ui(!self.picking, |ui| {
                let primary = egui::Button::new(
                    RichText::new("Choisir un dossier git").color(p.base),
                )
                .fill(p.link)
                .stroke(Stroke::NONE)
                .min_size(vec2(240.0, 34.0));
                if ui.add(primary).clicked() {
                    self.pick(&ctx, Pick::Folder);
                }
                ui.add_space(4.0);
                let secondary = egui::Button::new("Choisir un fichier workspace")
                    .min_size(vec2(240.0, 34.0));
                if ui.add(secondary).clicked() {
                    self.pick(&ctx, Pick::Workspace);
                }
            });
            if let Some(error) = &self.source_error {
                ui.add_space(12.0);
                ui.colored_label(p.error(), error);
            }
        });
    }

    fn header(&mut self, ui: &mut Ui, source: &Path) {
        let p = theme::palette(ui.ctx());
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.set_min_height(30.0);
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            paint_folder(ui, rect, p.muted);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let fetch_all = ui
                    .add_enabled(!self.rows.is_empty(), egui::Button::new("Tout fetch"))
                    .on_hover_text("Récupérer les nouveautés de tous les dépôts");
                if fetch_all.clicked() {
                    for index in 0..self.rows.len() {
                        self.start(&ctx, index, Request::Fetch);
                    }
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.set_max_width(ui.available_width() - 80.0);
                    ui.add(
                        egui::Label::new(RichText::new(source_label(source)).color(p.muted))
                            .truncate(),
                    )
                    .on_hover_text(source.display().to_string());
                    let change = ui.add_enabled(!self.picking, egui::Link::new("Changer"));
                    Popup::menu(&change).show(|ui| {
                        ui.set_min_width(220.0);
                        if ui.button("Un dossier git").clicked() {
                            ui.close();
                            self.pick(&ctx, Pick::Folder);
                        }
                        if ui.button("Un fichier workspace").clicked() {
                            ui.close();
                            self.pick(&ctx, Pick::Workspace);
                        }
                    });
                });
            });
        });
        if let Some(error) = &self.source_error {
            ui.colored_label(p.error(), error);
        }
    }

    fn confirm_remove(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &mut self.confirm else {
            return;
        };
        let p = theme::palette(ctx);
        let mut decision = None;
        let modal = egui::Modal::new(egui::Id::unique("remove-worktree"))
            .frame(
                Frame::new()
                    .fill(p.raised)
                    .stroke(Stroke::new(1.0, p.hairline))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(
                    RichText::new(format!(
                        "Supprimer le worktree {} ?",
                        folder_name(&confirm.path)
                    ))
                    .heading()
                    .color(p.text),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "Le dossier {} sera supprimé.",
                        source_label(&confirm.path)
                    ))
                    .color(p.muted),
                );
                if let Some(branch) = &confirm.branch {
                    ui.add_space(8.0);
                    ui.checkbox(
                        &mut confirm.delete_branch,
                        RichText::new(format!("Supprimer aussi la branche {branch}")).color(p.text),
                    );
                    let note = match (&confirm.pr, confirm.delete_branch) {
                        (Some(pr), _) if pr.state == PrState::Merged => Some((
                            format!("{} : son travail est dans master.", github::pr_label(pr)),
                            p.muted,
                        )),
                        (pr, true) => Some((
                            format!(
                                "{} : les commits que personne n'a récupérés seront perdus.",
                                pr.as_ref().map_or("Pas de PR".to_owned(), github::pr_label)
                            ),
                            p.red,
                        )),
                        _ => None,
                    };
                    if let Some((text, color)) = note {
                        ui.label(RichText::new(text).small().color(color));
                    }
                }
                if confirm.dirty > 0 {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Ses {} seront perdus.",
                            labels::files_label(confirm.dirty)
                        ))
                        .color(p.red),
                    );
                }
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let remove =
                        egui::Button::new(RichText::new("Supprimer").color(Color32::WHITE))
                            .fill(p.red)
                            .stroke(Stroke::NONE)
                            .min_size(vec2(104.0, 32.0));
                    if ui.add(remove).clicked() {
                        decision = Some(true);
                    }
                    if ui
                        .add(egui::Button::new("Annuler").min_size(vec2(96.0, 32.0)))
                        .clicked()
                    {
                        decision = Some(false);
                    }
                });
            });
        if modal.should_close() && decision.is_none() {
            decision = Some(false);
        }
        if let Some(remove) = decision {
            let confirm = self.confirm.take().expect("checked above");
            if remove {
                self.start(
                    ctx,
                    confirm.index,
                    Request::RemoveWorktree {
                        path: confirm.path,
                        force: confirm.dirty > 0,
                        branch: confirm.branch.filter(|_| confirm.delete_branch),
                    },
                );
            }
        }
    }

    fn lanes(&mut self, ui: &mut Ui) {
        let mut requests = Vec::new();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, row) in self.rows.iter_mut().enumerate() {
                    lane(ui, index, row, &mut requests);
                }
            });
        for (index, request) in requests {
            match request {
                Request::Refresh => self.last_refresh = None,
                Request::AskRemove(confirm) => self.confirm = Some(confirm),
                request => self.start(ui.ctx(), index, request),
            }
        }
    }
}

fn lane(ui: &mut Ui, index: usize, row: &mut Row, requests: &mut Vec<(usize, Request)>) {
    let p = theme::palette(ui.ctx());
    let tone = row.status.as_ref().map(labels::tone);
    let stripe = tone.map_or(p.hairline, |t| p.tone(t));
    let shown = Frame::new()
        .fill(p.lane)
        .inner_margin(Margin {
            left: 20,
            right: 14,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), LANE_HEIGHT),
                Layout::right_to_left(Align::Center),
                |ui| {
                    if let Some(Ok(status)) = &row.status {
                        ui.add_enabled_ui(!row.busy, |ui| {
                            if status.branch.is_some()
                                && sync_button(ui, p, status, tone.unwrap_or(Tone::Clean)).clicked()
                            {
                                requests.push((index, Request::Sync));
                            }
                            if fetch_button(ui, p).clicked() {
                                requests.push((index, Request::Fetch));
                            }
                        });
                        if row.busy {
                            ui.spinner();
                        }
                    }
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.spacing_mut().interact_size.y = 0.0;
                        repo_name(ui, p, &row.repo.name, row.web.as_deref());
                        match &row.status {
                            None => {
                                ui.label(RichText::new("…").heading().color(p.muted));
                            }
                            Some(Err(e)) => {
                                ui.label(
                                    RichText::new("Statut indisponible").heading().color(p.text),
                                );
                                ui.add(
                                    egui::Label::new(RichText::new(e).small().color(p.error()))
                                        .truncate(),
                                );
                            }
                            Some(Ok(status)) => {
                                ui.add_enabled_ui(!row.busy, |ui| {
                                    branch_picker(
                                        ui,
                                        p,
                                        index,
                                        status,
                                        &row.branches,
                                        &mut row.filter,
                                        requests,
                                    );
                                });
                                let status = status.clone();
                                detail_line(ui, p, index, &status, row, requests);
                            }
                        }
                    });
                },
            );
            if row.show_files
                && let Some(Ok(status)) = &row.status
            {
                file_list(ui, p, &row.repo.path, &status.files, &row.counts);
            }
            if row.show_worktrees {
                worktree_list(ui, p, index, row, requests);
            }
            if let Some(error) = &row.error {
                ui.add_space(6.0);
                let text: String = error.lines().take(6).collect::<Vec<_>>().join("\n");
                ui.add(egui::Label::new(RichText::new(text).small().color(p.error())).wrap());
            }
        });
    let rect = shown.response.rect;
    let painter = ui.painter();
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(STRIPE, rect.height())),
        CornerRadius::ZERO,
        stripe,
    );
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.hairline),
    );
}

fn sync_button(ui: &mut Ui, p: &Palette, status: &RepoStatus, tone: Tone) -> egui::Response {
    let label = labels::sync_label(status);
    let button = match p.on_tone(tone) {
        Some(on) => egui::Button::new(RichText::new(label).color(on))
            .fill(p.tone(tone))
            .stroke(Stroke::NONE),
        None => egui::Button::new(RichText::new(label).color(p.muted)),
    };
    ui.add(button.min_size(vec2(104.0, 32.0)))
        .on_hover_text(match tone {
            Tone::Unpublished => "Publier la branche sur origin",
            _ => "Récupérer puis envoyer les commits (git pull, git push)",
        })
}

fn fetch_button(ui: &mut Ui, p: &Palette) -> egui::Response {
    icon_button(ui, p, 32.0, paint_fetch, p.text)
        .on_hover_text("Fetch : récupérer les nouveautés sans toucher à la branche")
}

fn icon_button(
    ui: &mut Ui,
    p: &Palette,
    side: f32,
    paint: fn(&Ui, Rect, Color32),
    hover_color: Color32,
) -> egui::Response {
    let response = ui.add(egui::Button::new("").min_size(vec2(side, side)));
    let color = if response.hovered() {
        hover_color
    } else {
        p.muted
    };
    paint(ui, response.rect.shrink(side * 0.28), color);
    response
}

fn paint_trash(ui: &Ui, rect: Rect, color: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.4, color);
    let lid = rect.top() + rect.height() * 0.2;
    painter.line_segment([pos2(rect.left(), lid), pos2(rect.right(), lid)], stroke);
    let handle = rect.width() * 0.2;
    painter.line(
        vec![
            pos2(rect.center().x - handle, lid),
            pos2(rect.center().x - handle, rect.top()),
            pos2(rect.center().x + handle, rect.top()),
            pos2(rect.center().x + handle, lid),
        ],
        stroke,
    );
    let inset = rect.width() * 0.15;
    painter.line(
        vec![
            pos2(rect.left() + inset, lid),
            pos2(rect.left() + inset * 1.5, rect.bottom()),
            pos2(rect.right() - inset * 1.5, rect.bottom()),
            pos2(rect.right() - inset, lid),
        ],
        stroke,
    );
}

fn detail_line(
    ui: &mut Ui,
    p: &Palette,
    index: usize,
    status: &RepoStatus,
    row: &mut Row,
    requests: &mut Vec<(usize, Request)>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        for part in labels::details(status) {
            ui.label(RichText::new(part).small().color(p.text));
        }
        if status.changed > 0
            && disclosure(ui, &labels::files_label(status.changed), p.dirty, &mut row.show_files)
        {
            requests.push((index, Request::Refresh));
        }
        if !row.worktrees.is_empty()
            && disclosure(
                ui,
                &labels::worktrees_label(row.worktrees.len() as u32),
                p.text,
                &mut row.show_worktrees,
            )
        {
            requests.push((index, Request::Refresh));
        }
        if let Some(pr) = status.branch.as_ref().and_then(|b| row.prs.get(b)) {
            pr_badge(ui, p, pr);
        }
        if let Some(borrow) = &row.borrow {
            let name = folder_name(&borrow.worktree);
            let give_back = ui
                .add_enabled(
                    !row.busy,
                    egui::Link::new(RichText::new(format!("Rendre à {name}")).small()),
                )
                .on_hover_text(format!(
                    "{} vient du worktree {name} : remettre le checkout principal sur sa branche d'avant et rendre la branche au worktree",
                    borrow.branch
                ));
            if give_back.clicked() {
                requests.push((index, Request::GiveBack));
            }
        }
        if let Some(elapsed) = row.fetched.and_then(|t| t.elapsed().ok()) {
            ui.label(
                RichText::new(labels::fetched_ago(elapsed))
                    .small()
                    .color(p.muted),
            );
        }
    });
}

/// A clickable label with a chevron that folds and unfolds a section; true when toggled.
fn disclosure(ui: &mut Ui, text: &str, color: Color32, open: &mut bool) -> bool {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::TextStyle::Small.resolve(ui.style()),
        color,
    );
    let size = vec2(galley.size().x + 14.0, galley.size().y);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().galley(rect.min, galley, color);
    let chevron =
        Rect::from_center_size(pos2(rect.right() - 5.0, rect.center().y), vec2(10.0, 10.0));
    paint_chevron_scaled(ui, chevron, color, *open, 3.0);
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        *open = !*open;
    }
    response.clicked()
}

const FILES_SHOWN: usize = 60;

fn file_list(
    ui: &mut Ui,
    p: &Palette,
    repo: &Path,
    files: &[Change],
    counts: &HashMap<String, (u32, u32)>,
) {
    ui.add_space(4.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    for change in files.iter().take(FILES_SHOWN) {
        let (letter, word) = labels::change_label(change.kind);
        let letter_color = match change.kind {
            git::ChangeKind::Modified => p.amber,
            git::ChangeKind::Added => p.sage,
            git::ChangeKind::Deleted | git::ChangeKind::Conflicted => p.red,
            git::ChangeKind::Renamed => p.cyan,
            git::ChangeKind::Untracked => p.violet,
        };
        let deleted = change.kind == git::ChangeKind::Deleted;
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
        if response.hovered() && !deleted {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(4), p.raised);
        }
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(6.0, 0.0)))
                .layout(Layout::right_to_left(Align::Center)),
        );
        let (added, removed) = counts.get(&change.path).copied().unwrap_or_default();
        if removed > 0 {
            child.label(RichText::new(format!("−{removed}")).small().color(p.red));
        }
        if added > 0 {
            child.label(RichText::new(format!("+{added}")).small().color(p.sage));
        }
        child.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_sized(
                [12.0, 18.0],
                egui::Label::new(
                    RichText::new(letter)
                        .font(FontId::new(12.0, theme::medium()))
                        .color(letter_color),
                ),
            );
            let (dir, name) = match change.path.rsplit_once('/') {
                Some((dir, name)) => (format!("{dir}/"), name),
                None => (String::new(), change.path.as_str()),
            };
            let mut job = egui::text::LayoutJob::default();
            let font = FontId::new(13.0, egui::FontFamily::Proportional);
            job.append(&dir, 0.0, egui::TextFormat::simple(font.clone(), p.muted));
            job.append(name, 0.0, egui::TextFormat::simple(font, p.text));
            ui.add(egui::Label::new(job).truncate().selectable(false));
        });
        let response = response.on_hover_text(if deleted {
            format!("{}, {word}", change.path)
        } else {
            format!("{}, {word} : cliquer pour l'ouvrir", change.path)
        });
        if response.clicked() && !deleted {
            open_path(&repo.join(&change.path));
        }
    }
    if files.len() > FILES_SHOWN {
        ui.label(
            RichText::new(format!("et {} autres", files.len() - FILES_SHOWN))
                .small()
                .color(p.muted),
        );
    }
}

fn worktree_list(
    ui: &mut Ui,
    p: &Palette,
    index: usize,
    row: &Row,
    requests: &mut Vec<(usize, Request)>,
) {
    ui.add_space(4.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    for wt in &row.worktrees {
        let lent = row.borrow.as_ref().filter(|b| b.worktree == wt.tree.path);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        ui.painter().hline(
            rect.x_range(),
            rect.top() + 0.5,
            Stroke::new(1.0, p.hairline),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::right_to_left(Align::Center)),
        );
        child.add_enabled_ui(!row.busy, |ui| {
            if lent.is_none() {
                let remove = icon_button(ui, p, 26.0, paint_trash, p.red)
                    .on_hover_text("Supprimer ce worktree");
                if remove.clicked() {
                    requests.push((
                        index,
                        Request::AskRemove(ConfirmRemove::new(index, wt, &row.prs)),
                    ));
                }
            }
            if lent.is_some() {
                let give_back = ui
                    .add(egui::Button::new("Rendre").min_size(vec2(96.0, 26.0)))
                    .on_hover_text("Remettre le checkout principal sur sa branche d'avant et rendre la branche au worktree");
                if give_back.clicked() {
                    requests.push((index, Request::GiveBack));
                }
            } else if wt.tree.branch.is_some() {
                let dirty = wt.status.as_ref().map_or(0, |s| s.changed);
                let hover = match (&row.borrow, dirty) {
                    (Some(other), _) => format!("Rendez d'abord {} à son worktree", other.branch),
                    (None, 0) => "Passer le checkout principal sur cette branche, pour la tester sur la stack locale".to_owned(),
                    (None, n) => format!(
                        "Passer le checkout principal sur cette branche. Les {} du worktree restent dans le worktree.",
                        labels::files_label(n)
                    ),
                };
                let test_here = ui
                    .add_enabled(
                        row.borrow.is_none(),
                        egui::Button::new("Tester ici").min_size(vec2(96.0, 26.0)),
                    )
                    .on_hover_text(&hover)
                    .on_disabled_hover_text(&hover);
                if test_here.clicked() {
                    requests.push((index, Request::Borrow(wt.tree.path.clone())));
                }
            }
        });
        let branch = wt.tree.branch.as_ref().or(lent.map(|loan| &loan.branch));
        if let Some(pr) = branch.and_then(|b| row.prs.get(b)) {
            pr_badge(&mut child, p, pr);
        }
        if let Some(elapsed) = wt.touched.and_then(|t| t.elapsed().ok()) {
            let color = if labels::is_stale(elapsed) {
                p.amber
            } else {
                p.muted
            };
            child.label(
                RichText::new(labels::touched_ago(elapsed))
                    .small()
                    .color(color),
            );
        }
        if let Some(status) = &wt.status {
            for part in labels::details(status).into_iter().rev() {
                child.label(RichText::new(part).small().color(p.muted));
            }
            if status.changed > 0 {
                child.label(
                    RichText::new(labels::files_label(status.changed))
                        .small()
                        .color(p.dirty),
                );
            }
        }
        if lent.is_some() {
            child.label(
                RichText::new("dans le checkout principal")
                    .small()
                    .color(p.cyan),
            );
        }
        child.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(
                RichText::new(folder_name(&wt.tree.path))
                    .font(FontId::new(13.0, theme::medium()))
                    .color(p.text),
            );
            let branch = match (&wt.tree.branch, lent) {
                (Some(branch), _) => branch.clone(),
                (None, Some(loan)) => loan.branch.clone(),
                (None, None) => "(HEAD détachée)".to_owned(),
            };
            ui.add(
                egui::Label::new(
                    RichText::new(&branch)
                        .font(FontId::new(14.0, theme::branch()))
                        .color(p.muted),
                )
                .truncate(),
            )
            .on_hover_text(wt.tree.path.display().to_string());
        });
    }
}

fn snapshot(
    path: &Path,
    with_counts: bool,
    with_worktrees: bool,
    with_prs: bool,
) -> anyhow::Result<Snapshot> {
    let status = git::status(path)?;
    let counts = if with_counts && status.changed > 0 {
        git::line_counts(path).unwrap_or_default()
    } else {
        HashMap::new()
    };
    let worktrees = git::worktrees(path)
        .unwrap_or_default()
        .into_iter()
        .map(|tree| {
            let status = with_worktrees
                .then(|| git::status(&tree.path).ok())
                .flatten();
            let touched = status
                .as_ref()
                .and_then(|s| git::last_activity(&tree.path, &s.files));
            WorktreeRow {
                tree,
                status,
                touched,
            }
        })
        .collect();
    Ok(Snapshot {
        branches: git::branches(path)?,
        fetched: git::last_fetch(path),
        borrow: git::borrowed(path).unwrap_or(None),
        web: git::remote_url(path).and_then(|url| github::web_url(&url)),
        prs: with_prs.then(|| github::prs(path).unwrap_or_default()),
        status,
        counts,
        worktrees,
    })
}

fn repo_name(ui: &mut Ui, p: &Palette, name: &str, web: Option<&str>) {
    let font = FontId::new(12.0, theme::medium());
    let Some(url) = web else {
        ui.add(egui::Label::new(RichText::new(name).font(font).color(p.muted)).truncate());
        return;
    };
    let id = ui.next_auto_id();
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let text = RichText::new(name)
        .font(font)
        .color(if hovered { p.link } else { p.muted });
    let response = ui
        .add(
            egui::Label::new(if hovered { text.underline() } else { text })
                .truncate()
                .sense(Sense::click()),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("Ouvrir sur GitHub : {url}"));
    if response.clicked() {
        open_path(Path::new(url));
    }
}

fn pr_badge(ui: &mut Ui, p: &Palette, pr: &Pr) {
    let color = match pr.state {
        PrState::Open => p.sage,
        PrState::Draft => p.muted,
        PrState::Merged => p.violet,
        PrState::Closed => p.red,
    };
    let response = ui
        .add(
            egui::Label::new(RichText::new(github::pr_label(pr)).small().color(color))
                .sense(Sense::click()),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(&pr.url);
    if response.clicked() {
        open_path(Path::new(&pr.url));
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Opens a file, folder or URL with the system's default app.
fn open_path(path: &Path) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    if let Ok(mut child) = std::process::Command::new(opener)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        std::thread::spawn(move || child.wait());
    }
}

fn branch_picker(
    ui: &mut Ui,
    p: &Palette,
    index: usize,
    status: &RepoStatus,
    branches: &Branches,
    filter: &mut String,
    requests: &mut Vec<(usize, Request)>,
) {
    let name = labels::branch_label(status);
    let trigger = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.set_max_width(ui.available_width() - 24.0);
            let text = ui.add(
                egui::Label::new(RichText::new(&name).heading().color(p.text))
                    .truncate()
                    .sense(Sense::click()),
            );
            let (rect, chevron) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
            let hovered = text.hovered() || chevron.hovered();
            paint_chevron(ui, rect, if hovered { p.text } else { p.muted });
            text.union(chevron)
        })
        .inner
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(&name);

    Popup::menu(&trigger)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .width(380.0)
        .show(|ui| {
            ui.add(
                egui::TextEdit::singleline(filter)
                    .hint_text("Filtrer les branches")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(4.0);
            let query = filter.clone();
            let mut chosen = false;
            egui::ScrollArea::vertical()
                .max_height(340.0)
                .show(ui, |ui| {
                    let mut entry = |ui: &mut Ui, name: &str| {
                        let current = status.branch.as_deref() == Some(name);
                        if ui.selectable_label(current, name).clicked() {
                            if !current {
                                requests.push((index, Request::Switch(name.to_owned())));
                            }
                            chosen = true;
                            ui.close();
                        }
                    };
                    for name in branches
                        .local
                        .iter()
                        .filter(|b| labels::matches_filter(b, &query))
                    {
                        entry(ui, name);
                    }
                    let remote: Vec<_> = branches
                        .remote_only
                        .iter()
                        .filter(|b| labels::matches_filter(b, &query))
                        .collect();
                    if !remote.is_empty() {
                        ui.add_space(6.0);
                        ui.label(RichText::new("Sur le dépôt distant").small().color(p.muted));
                        for name in remote {
                            entry(ui, name);
                        }
                    }
                });
            if chosen {
                filter.clear();
            }
        });
}

fn paint_chevron(ui: &Ui, rect: Rect, color: Color32) {
    paint_chevron_scaled(ui, rect, color, false, 4.0);
}

fn paint_chevron_scaled(ui: &Ui, rect: Rect, color: Color32, up: bool, half: f32) {
    let c = rect.center();
    let dy = if up { -half / 2.0 } else { half / 2.0 };
    ui.painter().line(
        vec![
            pos2(c.x - half, c.y - dy),
            pos2(c.x, c.y + dy),
            pos2(c.x + half, c.y - dy),
        ],
        Stroke::new(1.6, color),
    );
}

fn paint_fetch(ui: &Ui, rect: Rect, color: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.6, color);
    let x = rect.center().x;
    let tip = rect.bottom() - 4.0;
    painter.line_segment([pos2(x, rect.top()), pos2(x, tip)], stroke);
    painter.line(
        vec![
            pos2(x - 4.0, tip - 4.0),
            pos2(x, tip),
            pos2(x + 4.0, tip - 4.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            pos2(rect.left(), rect.bottom()),
            pos2(rect.right(), rect.bottom()),
        ],
        stroke,
    );
}

fn paint_folder(ui: &Ui, rect: Rect, color: Color32) {
    let r = rect.shrink2(vec2(1.0, 2.5));
    let stroke = Stroke::new(1.4, color);
    let tab = r.left() + r.width() * 0.4;
    ui.painter().line(
        vec![
            pos2(r.left(), r.bottom()),
            pos2(r.left(), r.top()),
            pos2(tab, r.top()),
            pos2(tab + 2.0, r.top() + 2.0),
            pos2(r.right(), r.top() + 2.0),
            pos2(r.right(), r.bottom()),
            pos2(r.left(), r.bottom()),
        ],
        stroke,
    );
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        self.drain();
        if self.source.is_some()
            && self
                .last_refresh
                .is_none_or(|t| t.elapsed() >= REFRESH_EVERY)
        {
            self.refresh(&ctx);
        }
        if !self.rows.is_empty()
            && self
                .last_auto_fetch
                .is_none_or(|t| t.elapsed() >= FETCH_EVERY)
        {
            self.last_auto_fetch = Some(Instant::now());
            for index in 0..self.rows.len() {
                self.start(&ctx, index, Request::Fetch);
            }
        }
        ctx.request_repaint_after(REFRESH_EVERY);
        #[cfg(debug_assertions)]
        {
            if std::env::var_os("GB2K_CONFIRM").is_some()
                && self.confirm.is_none()
                && let Some((index, wt)) = self.rows.iter().enumerate().find_map(|(i, r)| {
                    r.worktrees
                        .iter()
                        .find(|w| w.tree.branch.is_some())
                        .map(|w| (i, w))
                })
            {
                self.confirm = Some(ConfirmRemove::new(index, wt, &self.rows[index].prs));
            }
            debug_snapshot(&ctx, &self.rows);
        }

        let Some(source) = self.source.clone() else {
            egui::CentralPanel::default_margins().show(ui, |ui| self.welcome(ui));
            return;
        };

        egui::Panel::top("source")
            .frame(
                Frame::new()
                    .fill(p.base)
                    .inner_margin(Margin::symmetric(16, 10))
                    .stroke(Stroke::NONE),
            )
            .show(ui, |ui| self.header(ui, &source));
        egui::CentralPanel::default_margins()
            .frame(
                Frame::new()
                    .fill(p.base)
                    .stroke(Stroke::new(1.0, p.hairline)),
            )
            .show(ui, |ui| self.lanes(ui));
        self.confirm_remove(&ctx);
    }
}

fn source_label(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_owned)) {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

/// Dev aid: `GB2K_SNAPSHOT=out.ppm` saves the window once every repo has loaded, then exits.
#[cfg(debug_assertions)]
fn debug_snapshot(ctx: &egui::Context, rows: &[Row]) {
    use std::sync::OnceLock;
    static STARTED: OnceLock<Instant> = OnceLock::new();
    static SENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Ok(path) = std::env::var("GB2K_SNAPSHOT") else {
        return;
    };
    let started = *STARTED.get_or_init(Instant::now);
    ctx.request_repaint_after(Duration::from_millis(100));
    let loaded = rows.iter().all(|r| r.status.is_some());
    if !loaded
        || started.elapsed() < Duration::from_secs(2)
        || SENT.swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
        egui::ScreenshotCallback::new(move |image| {
            let [w, h] = image.size;
            let mut bytes = format!("P6\n{w} {h}\n255\n").into_bytes();
            for pixel in &image.pixels {
                bytes.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
            }
            let _ = std::fs::write(&path, bytes);
            std::process::exit(0);
        }),
    ));
}
