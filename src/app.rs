//! Top-level application state and window layout.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::archive::{self, Format};
use crate::comicvine::Lookup;
use crate::convert::{self, CompressOptions};
use crate::jobs::{self, JobEvent, JobHandle, JobKind, JobReport};
use crate::library::scanner::{self, ScanEvent, ScanHandle, ScanReport};
use crate::library::{ComicRecord, Database, stats};
use crate::ui::{
    AboutDialog, ComicAction, ComicGrid, ComicTable, ComicVineDialog, ComicVineTarget, ComicView, CompressDialog, CompressOutcome,
    DetailsPane, EditorOutcome, ErrorsDialog, FieldValues, FolderTree, InfoEditor, QuitOutcome, Scope, ScrapeOutcome,
    Selection, SeriesCheckDialog, SeriesCheckOutcome, SettingsDialog, SettingsOutcome, StatsDialog, ToastKind, Toasts,
    TreeAction, comic_grid, progress_bar,
    show_quit_dialog, widgets,
};
use crate::icons::{self, Tone};
use crate::settings::{Settings, ViewMode};
use crate::theme;
use crate::thumbnails::Thumbnails;
use crate::util::{format_bytes, now_secs, open_in_file_manager};

/// Width of the details pane, including its margins.
const DETAILS_WIDTH: f32 = 320.0;

pub struct ComicEditorApp {
    db_path: PathBuf,
    db: Database,
    roots: Vec<PathBuf>,
    comics: Vec<ComicRecord>,

    tree: FolderTree,
    /// Text in the box above the folder tree.
    tree_filter: String,
    table: ComicTable,
    grid: ComicGrid,
    details: DetailsPane,
    view_mode: ViewMode,
    selection: Selection,
    thumbnails: Thumbnails,
    filter: Filter,
    /// Indices into `comics` that pass the filter, in display order.
    visible: Vec<usize>,
    /// Comics added at or after this time (seconds since the Unix epoch)
    /// count as recently added.
    recent_since: i64,

    settings: Settings,

    editor: Option<InfoEditor>,
    comicvine_dialog: Option<ComicVineDialog>,
    /// The ComicVine dialog was opened from the editor and fills its form
    /// instead of saving.
    comicvine_fills_editor: bool,
    settings_dialog: Option<SettingsDialog>,
    compress_dialog: Option<CompressDialog>,
    series_check: Option<SeriesCheckDialog>,
    stats: Option<StatsDialog>,
    about: Option<AboutDialog>,
    /// A comic to scroll into view in the list or grid next frame.
    scroll_to: Option<PathBuf>,
    /// Last used compression settings, offered again next time.
    compress_options: CompressOptions,

    scan: Option<ScanProgress>,
    job: Option<JobProgress>,
    status: String,
    toasts: Toasts,
    /// What the last scan or job couldn't process, offered from the status bar.
    errors: Option<ErrorsDialog>,
    show_errors: bool,
    /// The user tried to quit while a job was running and is being asked.
    confirm_quit: bool,
    /// Quit as soon as the (cancelled) job has stopped.
    quit_when_idle: bool,
}

/// What the user has narrowed the comic list down to.
struct Filter {
    scope: Scope,
    include_subfolders: bool,
    search: String,
}

struct ScanProgress {
    handle: ScanHandle,
    done: usize,
    total: usize,
}

struct JobProgress {
    handle: JobHandle,
    done: usize,
    total: usize,
    current: String,
}

impl ComicEditorApp {
    pub fn new(db_path: PathBuf, ctx: &egui::Context) -> Result<Self> {
        let db = Database::open(&db_path)?;
        let settings = Settings::load();
        let mut app = Self {
            db_path,
            db,
            roots: Vec::new(),
            comics: Vec::new(),
            tree: FolderTree::default(),
            tree_filter: String::new(),
            table: ComicTable::default(),
            grid: ComicGrid::default(),
            details: DetailsPane::default(),
            view_mode: settings.default_view,
            selection: Selection::default(),
            thumbnails: Thumbnails::new(ctx),
            filter: Filter {
                scope: Scope::All,
                include_subfolders: true,
                search: String::new(),
            },
            visible: Vec::new(),
            recent_since: 0,
            settings,
            editor: None,
            comicvine_dialog: None,
            comicvine_fills_editor: false,
            settings_dialog: None,
            compress_dialog: None,
            series_check: None,
            stats: None,
            about: None,
            scroll_to: None,
            compress_options: CompressOptions::default(),
            scan: None,
            job: None,
            status: String::new(),
            toasts: Toasts::default(),
            errors: None,
            show_errors: false,
            confirm_quit: false,
            quit_when_idle: false,
        };
        app.table.set_hidden_columns(&app.settings.hidden_columns);
        app.reload()?;
        Ok(app)
    }

    /// Reloads everything from the database and rebuilds derived views.
    fn reload(&mut self) -> Result<()> {
        self.roots = self.db.roots()?;
        self.comics = self.db.all_comics()?;
        self.recent_since = now_secs() - i64::from(self.settings.recent_days) * 86_400;
        self.tree = FolderTree::build(&self.roots, &self.comics, self.recent_since);
        self.refresh_visible();
        Ok(())
    }

    fn refresh_visible(&mut self) {
        let needle = self.filter.search.trim().to_lowercase();
        self.visible = self
            .comics
            .iter()
            .enumerate()
            .filter(|(_, c)| self.in_selected_folder(c))
            .filter(|(_, c)| needle.is_empty() || c.matches(&needle))
            .map(|(i, _)| i)
            .collect();
        self.table.sort(&self.comics, &mut self.visible);
    }

    fn in_selected_folder(&self, comic: &ComicRecord) -> bool {
        match &self.filter.scope {
            Scope::All => true,
            Scope::Recent => comic.added >= self.recent_since,
            Scope::Folder(folder) if self.filter.include_subfolders => comic.path.starts_with(folder),
            Scope::Folder(folder) => comic.folder() == folder,
        }
    }

    /// Rescans every library folder.
    pub fn start_scan(&mut self, ctx: &egui::Context) {
        self.refresh(ctx, None);
    }

    /// Re-reads `folder` (or the whole library) from disk: picks up new,
    /// changed and deleted comics.
    fn refresh(&mut self, ctx: &egui::Context, folder: Option<PathBuf>) {
        if self.scan.is_some() {
            self.notify(ToastKind::Info, "Already scanning");
            return;
        }
        let (folders, label) = match folder {
            Some(folder) => {
                let name = folder.file_name().unwrap_or(folder.as_os_str()).to_string_lossy().into_owned();
                (vec![folder], format!("Refreshing {name}…"))
            }
            None => (self.roots.clone(), "Scanning library…".to_owned()),
        };
        if folders.is_empty() {
            return;
        }
        let ctx = ctx.clone();
        let handle = scanner::start(self.db_path.clone(), folders, move || ctx.request_repaint());
        self.scan = Some(ScanProgress { handle, done: 0, total: 0 });
        self.status = label;
    }

    fn poll_scan(&mut self) {
        let Some(scan) = &mut self.scan else { return };
        let mut finished = None;
        while let Ok(event) = scan.handle.events.try_recv() {
            match event {
                ScanEvent::Progress { done, total } => {
                    scan.done = done;
                    scan.total = total;
                }
                ScanEvent::Finished(result) => finished = Some(result),
            }
        }

        if let Some(result) = finished {
            self.scan = None;
            let scanned = result.is_ok();
            match result {
                Ok(report) => {
                    let status = describe_scan(&report);
                    let problems = !report.errors.is_empty() || !report.unavailable.is_empty();
                    self.set_errors("Problems found while scanning", scan_errors(report));
                    // A routine scan that found nothing wrong isn't worth interrupting for.
                    if problems { self.notify(ToastKind::Warning, status) } else { self.status = status }
                }
                Err(e) => self.notify(ToastKind::Error, format!("Scan failed: {e:#}")),
            }
            let reloaded = self.reload();
            if scanned && reloaded.is_ok() {
                self.thumbnails.prune_cache(&self.comics);
            }
            self.report_error(reloaded);
        }
    }

    fn poll_job(&mut self) {
        let Some(job) = &mut self.job else { return };
        let mut finished = None;
        while let Ok(event) = job.handle.events.try_recv() {
            match event {
                JobEvent::Progress { done, total, current } => {
                    job.done = done;
                    job.total = total;
                    job.current = current;
                }
                JobEvent::Finished(result) => finished = Some(result),
            }
        }

        if let Some(result) = finished {
            let kind = job.handle.kind;
            self.job = None;
            match result {
                Ok(report) => {
                    let status = describe_job(kind, &report);
                    let toast = match (report.errors.is_empty(), report.cancelled) {
                        (false, _) => ToastKind::Warning,
                        (true, true) => ToastKind::Info,
                        (true, false) => ToastKind::Success,
                    };
                    self.set_errors(&format!("{} failed for some comics", kind.title()), report.errors);
                    self.notify(toast, status);
                }
                Err(e) => self.notify(ToastKind::Error, format!("{} failed: {e:#}", kind.title())),
            }
            let reloaded = self.reload();
            self.report_error(reloaded);
        }
    }

    // ----- metadata editing -----

    fn open_editor(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if self.job_running() {
            return;
        }

        let infos: Result<Vec<_>> = paths
            .iter()
            .map(|p| archive::read_comic_info(p).map_err(|e| e.context(p.display().to_string())))
            .collect();
        let infos = match infos {
            Ok(infos) => infos,
            Err(e) => return self.report_error(Err(e)),
        };

        let has_cbr = self
            .comics
            .iter()
            .any(|c| c.format == Format::Cbr && paths.contains(&c.path));
        let read_only = has_cbr.then(|| "CBR files are read-only — convert them to CBZ to edit".to_owned());

        self.editor = Some(InfoEditor::new(paths, &infos, read_only));
    }

    /// Writes metadata changes, which may differ per comic, in the background.
    fn save_info(&mut self, ctx: &egui::Context, edits: Vec<(PathBuf, FieldValues)>) {
        let targets = edits.iter().map(|(path, _)| path.clone()).collect();
        let edits: HashMap<PathBuf, FieldValues> = edits.into_iter().collect();
        let work = move |path: &Path| {
            let mut info = archive::read_comic_info(path)?;
            for (name, value) in edits.get(path).into_iter().flatten() {
                info.set(name, value);
            }
            archive::write_comic_info(path, &info)?;
            ComicRecord::load(path)
        };
        self.start_job(ctx, JobKind::SaveInfo, targets, work);
    }

    // ----- ComicVine -----

    fn open_comicvine(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        if self.job_running() || paths.is_empty() {
            return;
        }
        let Some(api_key) = self.comicvine_api_key() else { return };

        // Keep the user's (display) order; CBRs can't be written, so skip them.
        let by_path: HashMap<&Path, &ComicRecord> = self.comics.iter().map(|c| (c.path.as_path(), c)).collect();
        let comics: Vec<&ComicRecord> = paths.iter().filter_map(|p| by_path.get(p.as_path()).copied()).collect();
        let writable: Vec<&ComicRecord> = comics.iter().copied().filter(|c| c.format != Format::Cbr).collect();
        if writable.len() < comics.len() {
            let skipped = comics.len() - writable.len();
            let message = format!("Skipped {skipped} CBR file(s) — convert them to CBZ first");
            // Not `notify`: `comics` still borrows `self.comics`.
            self.toasts.push(ToastKind::Warning, message.clone());
            self.status = message;
        }
        let targets: Vec<ComicVineTarget> = writable
            .into_iter()
            .map(|c| ComicVineTarget { path: c.path.clone(), file_name: c.file_name(), lookup: Lookup::for_comic(c) })
            .collect();
        if targets.is_empty() {
            return;
        }
        self.comicvine_dialog = Some(ComicVineDialog::new(targets, api_key, "Apply", ctx));
        self.comicvine_fills_editor = false;
    }

    /// Looks up the comic being edited, searching with the form's current values.
    fn look_up_from_editor(&mut self, ctx: &egui::Context) {
        let Some(editor) = &self.editor else { return };
        let paths = editor.targets().to_vec();
        if paths.len() > 1 {
            // Each comic gets its own match, which one shared form can't show.
            self.editor = None;
            return self.open_comicvine(ctx, paths);
        }

        let path = paths[0].clone();
        let record = self.comics.iter().find(|c| c.path == path);
        let mut lookup = record.map(Lookup::for_comic).unwrap_or_default();
        for (field, value) in [("Series", &mut lookup.series), ("Number", &mut lookup.number)] {
            if !editor.value(field).is_empty() {
                *value = editor.value(field).to_owned();
            }
        }
        lookup.year = editor.value("Year").parse().ok().or(lookup.year);
        let file_name = record.map(ComicRecord::file_name).unwrap_or_default();

        let Some(api_key) = self.comicvine_api_key() else { return };
        let target = ComicVineTarget { path, file_name, lookup };
        self.comicvine_dialog = Some(ComicVineDialog::new(vec![target], api_key, "Use These Values", ctx));
        self.comicvine_fills_editor = true;
    }

    /// The API key, or `None` after asking the user to enter one.
    fn comicvine_api_key(&mut self) -> Option<String> {
        if self.settings.comicvine_api_key.is_empty() {
            let message = "Enter your ComicVine API key to look up comics.".to_owned();
            self.settings_dialog = Some(SettingsDialog::new(self.settings.clone(), Some(message)));
            return None;
        }
        Some(self.settings.comicvine_api_key.clone())
    }

    fn show_comicvine_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.comicvine_dialog else { return };
        match dialog.show(ctx) {
            Some(ScrapeOutcome::Apply(edits)) => {
                self.comicvine_dialog = None;
                match (&mut self.editor, edits.first()) {
                    (Some(editor), Some((_, values))) if self.comicvine_fills_editor => editor.fill(values),
                    _ => self.save_info(ctx, edits),
                }
            }
            Some(ScrapeOutcome::Cancel) => self.comicvine_dialog = None,
            None => {}
        }
    }

    fn show_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.settings_dialog else { return };
        match dialog.show(ctx) {
            Some(SettingsOutcome::Save(settings)) => {
                self.settings_dialog = None;
                if settings.default_view != self.settings.default_view {
                    self.view_mode = settings.default_view;
                }
                let recent_changed = settings.recent_days != self.settings.recent_days;
                self.settings = settings;
                let saved = self.settings.save();
                self.report_error(saved);
                if recent_changed {
                    let reloaded = self.reload();
                    self.report_error(reloaded);
                }
            }
            Some(SettingsOutcome::Cancel) => self.settings_dialog = None,
            None => {}
        }
    }

    // ----- conversion and compression -----

    fn convert_to_cbz(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        if self.job_running() {
            return;
        }
        let targets: Vec<PathBuf> = self
            .comics
            .iter()
            .filter(|c| c.needs_cbz_conversion() && paths.contains(&c.path))
            .map(|c| c.path.clone())
            .collect();
        if targets.is_empty() {
            self.notify(ToastKind::Info, "Selected comics are already CBZ");
            return;
        }
        let work = |path: &Path| ComicRecord::load(&convert::convert_to_cbz(path)?);
        self.start_job(ctx, JobKind::ConvertToCbz, targets, work);
    }

    fn open_compress_dialog(&mut self, paths: Vec<PathBuf>) {
        if self.job_running() || paths.is_empty() {
            return;
        }

        // Compressing again would change nothing: AVIF pages are never re-encoded.
        let (already, targets): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(|path| {
            self.comics.iter().any(|c| c.path == *path && c.compressed)
        });
        if targets.is_empty() {
            let message = match already.len() {
                1 => "Already compressed".to_owned(),
                n => format!("All {n} comics are already compressed"),
            };
            self.notify(ToastKind::Info, message);
            return;
        }

        if let Err(e) = convert::check_avifenc() {
            return self.report_error(Err(e));
        }
        self.compress_dialog = Some(CompressDialog::new(targets, already.len(), self.compress_options));
    }

    fn show_compress_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.compress_dialog else { return };
        match dialog.show(ctx) {
            Some(CompressOutcome::Start { targets, options }) => {
                self.compress_dialog = None;
                self.compress_options = options;
                let work = move |path: &Path| ComicRecord::load(&convert::compress_avif(path, options)?);
                self.start_job(ctx, JobKind::CompressAvif, targets, work);
            }
            Some(CompressOutcome::Cancel) => self.compress_dialog = None,
            None => {}
        }
    }

    // ----- background jobs -----

    /// True (with a status message) if a job is already running.
    fn job_running(&mut self) -> bool {
        if self.job.is_some() {
            self.notify(ToastKind::Info, "Wait for the current job to finish");
        }
        self.job.is_some()
    }

    fn start_job<W>(&mut self, ctx: &egui::Context, kind: JobKind, paths: Vec<PathBuf>, work: W)
    where
        W: Fn(&Path) -> Result<ComicRecord> + Send + 'static,
    {
        let ctx = ctx.clone();
        let total = paths.len();
        let handle = jobs::spawn(kind, self.db_path.clone(), paths, work, move || ctx.request_repaint());
        self.job = Some(JobProgress { handle, done: 0, total, current: String::new() });
    }

    fn modal_open(&self) -> bool {
        self.editor.is_some()
            || self.compress_dialog.is_some()
            || self.comicvine_dialog.is_some()
            || self.settings_dialog.is_some()
            || self.about.is_some()
            || self.show_errors
            || self.confirm_quit
    }

    fn show_errors_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.errors.as_ref().filter(|_| self.show_errors) else { return };
        if dialog.show(ctx) {
            // Seen; nothing left to offer until the next scan or job.
            self.errors = None;
            self.show_errors = false;
        }
    }

    /// Asks before quitting mid-job, so a comic isn't left half-rewritten.
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.job.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_quit = !self.quit_when_idle;
        }
        if self.quit_when_idle && self.job.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let Some(job) = self.job.as_ref().filter(|_| self.confirm_quit) else {
            self.confirm_quit = false;
            return;
        };
        match show_quit_dialog(ctx, job.handle.kind.title()) {
            Some(QuitOutcome::StopAndQuit) => {
                job.handle.cancel();
                self.quit_when_idle = true;
                self.confirm_quit = false;
            }
            Some(QuitOutcome::KeepRunning) => self.confirm_quit = false,
            None => {}
        }
    }

    fn show_editor(&mut self, ctx: &egui::Context) {
        // Hidden while a dialog it opened (ComicVine, or Settings to enter an
        // API key) is on top, so the two don't both react to keys like Escape.
        if self.comicvine_dialog.is_some() || self.settings_dialog.is_some() {
            return;
        }
        let Some(editor) = &mut self.editor else { return };
        match editor.show(ctx) {
            Some(EditorOutcome::Save { targets, changes }) => {
                self.editor = None;
                let edits = targets.into_iter().map(|path| (path, changes.clone())).collect();
                self.save_info(ctx, edits);
            }
            Some(EditorOutcome::LookUpComicVine) => self.look_up_from_editor(ctx),
            Some(EditorOutcome::Cancel) => self.editor = None,
            None => {}
        }
    }

    // ----- series check -----

    /// Checks the comics in `folder` (and its subfolders), or the whole library.
    fn open_series_check(&mut self, ctx: &egui::Context, folder: Option<PathBuf>) {
        let entered = match self.db.all_collects() {
            Ok(entered) => entered,
            Err(e) => return self.report_error(Err(e)),
        };
        let comics = self.comics_under(folder.as_deref());
        self.series_check = Some(SeriesCheckDialog::new(scope_label(folder.as_deref()), comics, entered, ctx));
    }

    /// Opens Library Stats for `folder` (and its subfolders), or the whole library.
    fn open_stats(&mut self, folder: Option<PathBuf>) {
        let original_sizes = match self.db.original_sizes() {
            Ok(sizes) => sizes,
            Err(e) => return self.report_error(Err(e)),
        };
        let comics = self.comics_under(folder.as_deref());
        let stats = stats::compute(&comics, &original_sizes, self.recent_since);
        self.stats = Some(StatsDialog::new(scope_label(folder.as_deref()), stats, self.settings.recent_days));
    }

    fn comics_under(&self, folder: Option<&Path>) -> Vec<ComicRecord> {
        self.comics
            .iter()
            .filter(|c| folder.is_none_or(|f| c.path.starts_with(f)))
            .cloned()
            .collect()
    }

    fn show_series_check(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.series_check else { return };
        match dialog.show(ctx) {
            Some(SeriesCheckOutcome::SetCollects(path, issues)) => {
                let saved = self.db.set_collects(&path, &issues);
                if saved.is_ok() {
                    dialog.set_entered(path, issues);
                }
                self.report_error(saved);
            }
            Some(SeriesCheckOutcome::Show(path)) => self.show_comic(&path),
            Some(SeriesCheckOutcome::Close) => self.series_check = None,
            None => {}
        }
    }

    /// Selects a comic in the library and scrolls to it, showing its folder.
    fn show_comic(&mut self, path: &Path) {
        let Some(comic) = self.comics.iter().find(|c| c.path == path) else {
            return self.notify(ToastKind::Info, "That comic is no longer in the library");
        };
        self.filter.scope = Scope::Folder(comic.folder().to_path_buf());
        self.filter.search.clear();
        self.refresh_visible();
        if let Some(position) = self.visible.iter().position(|&i| self.comics[i].path == path) {
            self.selection.select_only(position, path);
            self.scroll_to = Some(path.to_path_buf());
        }
    }

    // ----- library management -----

    fn add_folder(&mut self, ctx: &egui::Context) {
        let Some(folder) = rfd::FileDialog::new().set_title("Add Library Folder").pick_folder() else {
            return;
        };
        self.report_error(self.db.add_root(&folder));
        let reloaded = self.reload();
        self.report_error(reloaded);
        self.start_scan(ctx);
    }

    fn remove_root(&mut self, root: &Path) {
        self.report_error(self.db.remove_root(root));
        if self.filter.scope.folder().is_some_and(|f| f.starts_with(root)) {
            self.filter.scope = Scope::All;
        }
        let reloaded = self.reload();
        self.report_error(reloaded);
    }

    /// Replaces the list offered by the status bar's problems link.
    fn set_errors(&mut self, title: &str, errors: Vec<(PathBuf, String)>) {
        self.show_errors = false;
        self.errors = (!errors.is_empty()).then(|| ErrorsDialog::new(title.to_owned(), errors));
    }

    /// Shows an error; keeps the app running.
    fn report_error(&mut self, result: Result<()>) {
        if let Err(e) = result {
            eprintln!("error: {e:#}");
            self.notify(ToastKind::Error, format!("Error: {e:#}"));
        }
    }

    /// Shows `text` in the status bar and as a toast.
    fn notify(&mut self, kind: ToastKind, text: impl Into<String>) {
        let text = text.into();
        self.toasts.push(kind, text.clone());
        self.status = text;
    }

    fn handle_tree_action(&mut self, action: TreeAction, ctx: &egui::Context) {
        match action {
            TreeAction::Select(scope) => {
                if scope == Scope::Recent {
                    self.table.sort_newest_first();
                }
                self.filter.scope = scope;
                // Picking a folder means "show me this folder", not "search within it".
                self.filter.search.clear();
                self.refresh_visible();
            }
            TreeAction::Refresh(folder) => self.refresh(ctx, folder),
            TreeAction::CheckSeries(folder) => self.open_series_check(ctx, folder),
            TreeAction::Stats(folder) => self.open_stats(folder),
            TreeAction::RemoveRoot(root) => self.remove_root(&root),
            TreeAction::OpenInFileManager(dir) => self.open_dir(&dir),
        }
    }

    fn handle_comic_action(&mut self, action: ComicAction, ctx: &egui::Context) {
        match action {
            ComicAction::EditInfo(paths) => self.open_editor(paths),
            ComicAction::ConvertToCbz(paths) => self.convert_to_cbz(ctx, paths),
            ComicAction::CompressAvif(paths) => self.open_compress_dialog(paths),
            ComicAction::LookUpComicVine(paths) => self.open_comicvine(ctx, paths),
            ComicAction::OpenInFileManager(dir) => self.open_dir(&dir),
        }
    }

    fn open_dir(&mut self, dir: &Path) {
        self.report_error(open_in_file_manager(dir).map_err(Into::into));
    }

    // ----- layout -----

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add(icons::button("folder-new", "", "Add Library Folder…")).clicked() {
                    ui.close();
                    self.add_folder(ui.ctx());
                }
                let can_scan = self.scan.is_none() && !self.roots.is_empty();
                if ui.add_enabled(can_scan, icons::button("view-refresh", "", "Refresh Library")).clicked() {
                    ui.close();
                    self.start_scan(ui.ctx());
                }
                let can_check = !self.comics.is_empty();
                if ui.add_enabled(can_check, icons::button("view-statistics", "", "Check Series…")).clicked() {
                    ui.close();
                    self.open_series_check(ui.ctx(), None);
                }
                if ui.add_enabled(can_check, icons::button("office-chart-bar", "", "Library Stats…")).clicked() {
                    ui.close();
                    self.open_stats(None);
                }
                ui.separator();
                if ui.add(icons::button("configure", "", "Settings…")).clicked() {
                    ui.close();
                    self.settings_dialog = Some(SettingsDialog::new(self.settings.clone(), None));
                }
                ui.separator();
                if ui.add(icons::button("application-exit", "", "Quit")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.add(icons::button("help-about", "", "About Comic Editor")).clicked() {
                    ui.close();
                    self.about = Some(AboutDialog::new(self.db_path.clone()));
                }
            });
        });
    }

    /// Actions on the selected comics on the left; search and view options
    /// on the right.
    fn toolbar(&mut self, ui: &mut egui::Ui) -> Option<ComicAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            // Labels only when there's room for them.
            let compact = ui.available_width() < 1000.0;
            let disabled_reason = if self.job.is_some() {
                Some("Wait for the current job to finish")
            } else if self.selection.is_empty() {
                Some("Select comics first")
            } else {
                None
            };
            type MakeAction = fn(Vec<PathBuf>) -> ComicAction;
            let actions: [(&str, &str, &str, MakeAction); 4] = [
                ("document-edit", "Edit Info", "Edit the selected comics' info (Ctrl+I)", ComicAction::EditInfo),
                ("globe", "Look Up", "Look up the selected comics on ComicVine", ComicAction::LookUpComicVine),
                ("convert", "Convert", "Convert the selected comics to CBZ", ComicAction::ConvertToCbz),
                ("archive-insert", "Compress", "Compress the selected comics' pages to AVIF", ComicAction::CompressAvif),
            ];
            for (i, (icon, label, tooltip, make)) in actions.into_iter().enumerate() {
                if i == 2 {
                    ui.separator();
                }
                let text = if compact && icons::image(icon).is_some() { "" } else { label };
                let response = ui
                    .add_enabled(disabled_reason.is_none(), icons::button(icon, "", text))
                    .on_hover_text(tooltip)
                    .on_disabled_hover_text(disabled_reason.unwrap_or_default());
                if response.clicked() {
                    action = Some(make(self.selection.paths_in_order(&self.comics, &self.visible)));
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.details_toggle(ui);
                ui.add_space(4.0);
                self.view_switch(ui);
                if self.view_mode == ViewMode::Grid {
                    ui.add_space(8.0);
                    ui.add(
                        egui::Slider::new(
                            &mut self.grid.cell_width,
                            comic_grid::MIN_CELL_WIDTH..=comic_grid::MAX_CELL_WIDTH,
                        )
                        .show_value(false),
                    )
                    .on_hover_text("Cover size");
                }
                ui.add_space(8.0);
                self.search_box(ui);
            });
        });
        action
    }

    fn search_box(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().clamp(120.0, 340.0);
        let hint = "Search series, title, publisher, writer, file…";
        let search = widgets::search_field(ui, &mut self.filter.search, "comic_search", hint, width)
            .on_hover_text("Ctrl+F");
        let find = !self.modal_open() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F));
        if find {
            search.request_focus();
        }
        let escape = (search.has_focus() || search.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape));
        if escape && !self.filter.search.is_empty() {
            self.filter.search.clear();
            self.refresh_visible();
        } else if search.changed() {
            self.refresh_visible();
        }
    }

    fn details_toggle(&mut self, ui: &mut egui::Ui) {
        let shown = self.settings.show_details;
        let tone = if shown { Tone::Selected } else { Tone::Normal };
        let button = icons::button_toned("documentinfo", "ℹ", "Details", tone).selected(shown).frame_when_inactive(true);
        if ui.add(button).on_hover_text("Show or hide the details pane").clicked() {
            self.settings.show_details = !shown;
            let saved = self.settings.save();
            self.report_error(saved);
        }
    }

    /// List and Covers as one two-part button, laid out right to left.
    fn view_switch(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let r = ui.visuals().widgets.inactive.corner_radius.nw;
            let modes = [
                (ViewMode::Grid, "view-list-icons", "▦", egui::CornerRadius { nw: 0, sw: 0, ne: r, se: r }),
                (ViewMode::List, "view-list-details", "☰", egui::CornerRadius { nw: r, sw: r, ne: 0, se: 0 }),
            ];
            for (mode, icon, glyph, corners) in modes {
                let selected = self.view_mode == mode;
                let tone = if selected { Tone::Selected } else { Tone::Normal };
                let button = icons::button_toned(icon, glyph, mode.label(), tone)
                    .selected(selected)
                    .frame_when_inactive(true)
                    .corner_radius(corners);
                if ui.add(button).clicked() {
                    self.view_mode = mode;
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(format!("{} comics", self.visible.len()));
            let selected = self.selection.len();
            if selected > 0 {
                ui.separator();
                ui.label(format!("{selected} selected"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(job) = &self.job {
                    let cancelling = job.handle.is_cancelling();
                    if ui.add_enabled(!cancelling, egui::Button::new("Cancel")).clicked() {
                        job.handle.cancel();
                    }
                    let text = format!("{} {}/{}", job.handle.kind.title(), job.done, job.total);
                    progress_bar(ui, 200.0, fraction(job.done, job.total), &text);
                    let cancel_text =
                        if self.quit_when_idle { "Quitting after current comic…" } else { "Cancelling after current comic…" };
                    ui.label(if cancelling { cancel_text } else { &job.current });
                } else if let Some(scan) = &self.scan {
                    if scan.total == 0 {
                        // Still finding the comics; there's nothing to count yet.
                        ui.spinner();
                    } else {
                        progress_bar(ui, 200.0, fraction(scan.done, scan.total), &format!("{}/{}", scan.done, scan.total));
                    }
                }
                if let Some(errors) = &self.errors {
                    let icon = icons::image("dialog-warning");
                    let text = match errors.count() {
                        1 => "1 problem".to_owned(),
                        n => format!("{n} problems"),
                    };
                    let text = if icon.is_some() { text } else { format!("⚠ {text}") };
                    if ui.link(text).on_hover_text("Show details").clicked() {
                        self.show_errors = true;
                    }
                    if let Some(icon) = icon {
                        ui.add(icon);
                    }
                }
                ui.label(&self.status);
            });
        });
    }

    fn tree_filter_box(&mut self, ui: &mut egui::Ui) {
        // One row, laid out right-to-left so the clear button is placed first
        // and the text box fills exactly the space left. (Sizing the box from
        // the panel width made the panel grow every frame; a bare right-to-left
        // layout would claim the panel's whole height.)
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clear_button = icons::button("edit-clear-locationbar-rtl", "✖", "").frame(false);
                let clear = !self.tree_filter.is_empty() && ui.add(clear_button).on_hover_text("Clear").clicked();
                let response =
                    widgets::search_field(ui, &mut self.tree_filter, "tree_filter", "Filter folders…", f32::INFINITY);
                let escape = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
                if escape || clear {
                    self.tree_filter.clear();
                }
            });
        });
    }

    /// Draws the list or grid of comics.
    fn comic_view(&mut self, ui: &mut egui::Ui) -> Option<ComicAction> {
        let scroll_to = self
            .scroll_to
            .take()
            .and_then(|path| self.visible.iter().position(|&i| self.comics[i].path == path));
        let mut view = ComicView {
            comics: &self.comics,
            visible: &self.visible,
            selection: &mut self.selection,
        };
        match self.view_mode {
            ViewMode::Grid => self.grid.show(ui, &mut view, &mut self.thumbnails, scroll_to),
            ViewMode::List => {
                let response = self.table.show(ui, &mut view, scroll_to);
                if response.sort_changed {
                    self.table.sort(&self.comics, &mut self.visible);
                }
                if response.columns_changed {
                    self.settings.hidden_columns = self.table.hidden_columns();
                    let saved = self.settings.save();
                    self.report_error(saved);
                }
                response.action
            }
        }
    }

    fn empty_library_hint(&mut self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                ui.heading("Your library is empty");
                ui.add_space(8.0);
                if ui.button("Add Library Folder…").clicked() {
                    self.add_folder(ui.ctx());
                }
            });
        });
    }
}

impl eframe::App for ComicEditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        theme::reload_if_changed(&ctx);
        self.poll_scan();
        self.poll_job();
        self.thumbnails.begin_frame(&ctx);
        if !self.modal_open() && ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.refresh(&ctx, self.filter.scope.folder().map(Path::to_path_buf));
        }

        let panel_fill = ui.visuals().panel_fill;
        let bar = |margin: egui::Margin| egui::Frame::new().fill(panel_fill).inner_margin(margin);
        egui::Panel::top("menu_bar").frame(bar(egui::Margin::symmetric(8, 3))).show(ui, |ui| self.menu_bar(ui));
        let status_bar = egui::Panel::bottom("status_bar")
            .frame(bar(egui::Margin::symmetric(10, 5)))
            .show(ui, |ui| self.status_bar(ui));

        // Capped so the comic pane always stays visible, even if a saved
        // width from an earlier session is too large.
        let max_tree_width = (ui.available_width() * 0.5).max(200.0);
        let tree_action = egui::Panel::left("library_tree")
            .resizable(true)
            .default_size(260.0)
            .size_range(160.0..=max_tree_width)
            .frame(bar(egui::Margin::same(10)))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("LIBRARY").small().family(theme::bold()).weak());
                ui.add_space(2.0);
                self.tree_filter_box(ui);
                let subfolders = ui
                    .checkbox(&mut self.filter.include_subfolders, "Include subfolders")
                    .on_hover_text("Also show comics in the selected folder's subfolders");
                if subfolders.changed() {
                    self.refresh_visible();
                }
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| self.tree.show(ui, &self.filter.scope, &self.tree_filter))
                    .inner
            })
            .inner;
        if let Some(action) = tree_action {
            self.handle_tree_action(action, &ctx);
        }

        // Added after the tree, so it spans only the comic and details panes.
        let mut actions = Vec::new();
        if !self.roots.is_empty() {
            let toolbar = egui::Panel::top("toolbar").frame(bar(egui::Margin::symmetric(10, 8)));
            actions.extend(toolbar.show(ui, |ui| self.toolbar(ui)).inner);
        }

        if self.settings.show_details && !self.roots.is_empty() {
            // Fixed, so it doesn't change size as different comics are selected.
            let details = egui::Panel::right("details")
                .resizable(false)
                .exact_size(DETAILS_WIDTH)
                .frame(bar(egui::Margin::same(12)))
                .show(ui, |ui| {
                    let selected: Vec<&ComicRecord> = self
                        .visible
                        .iter()
                        .map(|&i| &self.comics[i])
                        .filter(|c| self.selection.contains(&c.path))
                        .collect();
                    self.details.show(ui, &selected, &mut self.thumbnails)
                });
            actions.extend(details.inner);
        }

        // The comics sit on the scheme's view background, like a list in a KDE app.
        let view_frame = egui::Frame::new().fill(theme::palette().view_bg).inner_margin(egui::Margin {
            left: 4,
            top: 4,
            ..Default::default()
        });
        egui::CentralPanel::default().frame(view_frame).show(ui, |ui| {
            if self.roots.is_empty() {
                self.empty_library_hint(ui);
                return;
            }
            actions.extend(self.comic_view(ui));
        });
        // The panes sit behind any open dialog; ignore them meanwhile.
        if !self.modal_open() {
            for action in actions {
                self.handle_comic_action(action, &ctx);
            }
        }

        self.show_editor(&ctx);
        self.show_compress_dialog(&ctx);
        self.show_comicvine_dialog(&ctx);
        self.show_settings_dialog(&ctx);
        self.show_errors_dialog(&ctx);
        self.show_series_check(&ctx);
        if let Some(stats) = &mut self.stats && !stats.show(&ctx) {
            self.stats = None;
        }
        if let Some(about) = &mut self.about && about.show(&ctx) {
            self.about = None;
        }
        self.handle_close_request(&ctx);
        self.toasts.show(&ctx, status_bar.response.rect.height());
    }
}

/// "the whole library", or "“Batman” and its subfolders".
fn scope_label(folder: Option<&Path>) -> String {
    match folder {
        Some(folder) => {
            let name = folder.file_name().unwrap_or(folder.as_os_str()).to_string_lossy();
            format!("“{name}” and its subfolders")
        }
        None => "the whole library".to_owned(),
    }
}

fn describe_scan(report: &ScanReport) -> String {
    let mut text = format!("Scan complete: {} updated, {} removed", report.updated, report.removed);
    if !report.errors.is_empty() {
        text += &format!(", {} unreadable", report.errors.len());
        for (path, error) in &report.errors {
            eprintln!("unreadable: {}: {error}", path.display());
        }
    }
    if !report.unavailable.is_empty() {
        text += &format!(", {} folder(s) not found", report.unavailable.len());
    }
    text
}

/// Unreadable comics, then missing library folders.
fn scan_errors(report: ScanReport) -> Vec<(PathBuf, String)> {
    let missing = "Library folder not found or not readable. Its comics stay in the library until it's back.";
    let mut errors = report.errors;
    errors.extend(report.unavailable.into_iter().map(|root| (root, missing.to_owned())));
    errors
}

fn describe_job(kind: JobKind, report: &JobReport) -> String {
    let title = kind.title();
    let mut text = format!("{title}: {} done", report.succeeded);
    if kind == JobKind::CompressAvif && report.bytes_before > 0 {
        let saved = report.bytes_before.saturating_sub(report.bytes_after);
        let percent = saved as f64 / report.bytes_before as f64 * 100.0;
        text += &format!(
            // Plain "->": the default egui font has no arrow glyph.
            " ({} -> {}, saved {percent:.0}%)",
            format_bytes(report.bytes_before),
            format_bytes(report.bytes_after)
        );
    }
    if report.cancelled {
        text += ", cancelled";
    }
    if !report.errors.is_empty() {
        text += &format!(", {} failed", report.errors.len());
        for (path, error) in &report.errors {
            eprintln!("{title} failed: {}: {error}", path.display());
        }
    }
    text
}

fn fraction(done: usize, total: usize) -> f32 {
    if total == 0 { 0.0 } else { done as f32 / total as f32 }
}


