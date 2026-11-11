//! choab-zip — a native, 7-Zip-style archiver for Linux.
//!
//! Pure Rust: sevenz-rust2 for 7z, `zip` for zip, `tar` + codecs for tarballs,
//! egui for the UI. No Wine, no Qt, no `7z` subprocess. Text files are
//! previewed in-app; other types are handed to the desktop's default app.

mod design;
mod engine;
mod ui;

use eframe::egui;
use design::Icon;
use engine::{
    human, CompressionLevel, CreateOptions, Error as EngineError, Format, SevenZMethod, ZipMethod,
};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Upper bound on what the built-in viewer will load, to stay responsive.
const VIEW_LIMIT: u64 = 8 * 1024 * 1024;
/// Upper bound on bytes rendered as text (egui chokes on huge strings).
const RENDER_LIMIT: usize = 400_000;

/// Extensions that the desktop can present better than a text viewer.
/// Everything else is sniffed: small text opens in-app, binary is delegated.
/// Note: images (PNG, JPG, BMP, GIF, WEBP) are opened natively in the built-in viewer!
const EXTERNAL_EXTS: &[&str] = &[
    // Documents
    "pdf", "djvu", "djv", "epub", "mobi", "azw", "azw3", "fb2", "cbz", "cbr",
    "doc", "docx", "odt", "rtf", "xls", "xlsx", "ods", "ppt", "pptx", "odp",
    // Audio / video
    "mp3", "flac", "wav", "ogg", "oga", "opus", "m4a", "aac", "wma", "mid", "midi",
    "mp4", "m4v", "mkv", "webm", "avi", "mov", "wmv", "flv", "mpg", "mpeg", "ts",
    "m2ts", "ogv", "3gp",
    // Fonts and desktop formats
    "ttf", "otf", "woff", "woff2", "desktop", "torrent", "iso", "img",
];

pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.rsplit_once('.').map(|(_, ext)| ext),
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp")
    )
}

enum Msg {
    Entries(PathBuf, Vec<engine::Entry>, Option<String>),
    EntriesError(PathBuf, EngineError),
    Preview(Result<String, String>, String),
    PreviewImage {
        key: String,
        name: String,
        #[allow(dead_code)]
        size: u64,
        #[allow(dead_code)]
        method: Option<String>,
        result: Result<Vec<u8>, String>,
    },
    External {
        key: String,
        name: String,
        /// Where the entry was written when it opened successfully.
        result: Result<PathBuf, String>,
    },
    /// An archive entry was extracted for a fresh choab-zip window.
    NestedArchive {
        name: String,
        result: Result<(), String>,
    },
    Done(String, bool),
    /// An extraction finished; partial runs carry what was kept so the user can
    /// decide whether to remove it.
    Extracted {
        report: Box<engine::ExtractReport>,
        dest: PathBuf,
    },
    /// Live progress of a streaming pass (reading or extracting).
    Progress {
        consumed: u64,
        written: u64,
        total: u64,
        entries: usize,
        entries_total: usize,
        label: String,
    },
    Verified(usize, Vec<String>),
    ChecksumResult {
        #[allow(dead_code)]
        path: PathBuf,
        crc32: String,
        sha256: String,
        #[allow(dead_code)]
        size: u64,
        error: Option<String>,
    },
}

/// Wayland app id / X11 `WM_CLASS` — what the desktop shell matches a window
/// against its `.desktop` file. Flatpak exports that file under the reverse-DNS
/// id, so inside the sandbox the id must be the full app id; otherwise the
/// shell cannot resolve the window and falls back to showing the raw sandbox id
/// (`io.github.choab.Zip`) as the application name. A plain install ships
/// `choab-zip.desktop`, so the plain binary name is the right id there.
fn window_app_id() -> &'static str {
    if std::path::Path::new("/.flatpak-info").exists() {
        "io.github.choab.Zip"
    } else {
        "choab-zip"
    }
}

#[derive(Debug, Clone)]
pub struct ThemePalette {
    pub bg_window: egui::Color32,
    pub bg_titlebar: egui::Color32,
    pub bg_toolbar: egui::Color32,
    pub bg_status: egui::Color32,
    pub bg_left_panel: egui::Color32,
    pub bg_right_panel: egui::Color32,
    pub border: egui::Color32,
    pub text: egui::Color32,
    pub text_muted: egui::Color32,
    pub row_hover: egui::Color32,
    pub row_selected: egui::Color32,
    pub row_focus: egui::Color32,
}

pub struct PasswordDialog {
    pub target: PathBuf,
    pub entry: Option<String>,
    pub prompt: String,
    pub password: String,
    pub show_password: bool,
    pub error: Option<String>,
}

pub struct ChecksumDialog {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub crc32: String,
    pub sha256: String,
    pub busy: bool,
    pub error: Option<String>,
}

/// Shown after an extraction that stopped before the end: nothing is deleted
/// automatically, the user decides.
pub struct PartialExtractDialog {
    pub reason: String,
    pub dest: PathBuf,
    pub incomplete: Vec<PathBuf>,
    pub corrupt_copy: Option<PathBuf>,
    pub extracted: usize,
}

pub struct NewFolderDialog {
    pub name: String,
    pub error: Option<String>,
}

/// Which pane has focus. 7-Zip drives both panes with one set of keys.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    /// The filesystem tree on the left.
    Files,
    /// The archive contents on the right.
    Archive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortCol {
    Name,
    Size,
    Packed,
    Modified,
}

struct DirRow {
    name: String,
    is_dir: bool,
    size: u64,
    modified: String,
    is_archive: bool,
}

/// A row shown in the archive pane, already sorted/filtered.
struct Row {
    name: String,
    is_dir: bool,
    size: u64,
    packed: u64,
    modified: String,
}

/// A bookmark or mount point in the sidebar.
pub struct Place {
    pub label: String,
    pub path: PathBuf,
    pub icon: Icon,
}

impl Place {
    fn push(out: &mut Vec<Place>, label: &str, path: PathBuf, icon: Icon) {
        if path.is_dir() {
            out.push(Place {
                label: label.to_string(),
                path,
                icon,
            });
        }
    }

    /// The usual XDG folders, in the order a file manager shows them.
    pub fn bookmarks() -> Vec<Place> {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"));
        let mut out = vec![Place {
            label: "Home".into(),
            path: home.clone(),
            icon: Icon::Home,
        }];
        for (label, dir, icon) in [
            ("Desktop", "Desktop", Icon::Desktop),
            ("Documents", "Documents", Icon::Documents),
            ("Downloads", "Downloads", Icon::Download),
            ("Pictures", "Pictures", Icon::Pictures),
            ("Music", "Music", Icon::Music),
            ("Videos", "Videos", Icon::Video),
        ] {
            Place::push(&mut out, label, home.join(dir), icon);
        }
        out
    }

    /// Root plus whatever is mounted under /media and /run/media.
    pub fn devices() -> Vec<Place> {
        let mut out = vec![Place {
            label: "File System".into(),
            path: PathBuf::from("/"),
            icon: Icon::Disk,
        }];
        let user = std::env::var("USER").unwrap_or_default();
        for base in [format!("/media/{user}"), format!("/run/media/{user}")] {
            let Ok(rd) = std::fs::read_dir(&base) else {
                continue;
            };
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let label = entry.file_name().to_string_lossy().to_string();
                    out.push(Place {
                        label,
                        path,
                        icon: Icon::Disk,
                    });
                }
            }
        }
        out
    }
}

struct ChoabZip {
    cwd: PathBuf,
    cwd_editing: bool,
    cwd_edit_text: String,
    dirs: Vec<DirRow>,
    dir_sel: BTreeSet<usize>,
    dir_focus: Option<usize>,

    archive: Option<PathBuf>,
    entries: Vec<engine::Entry>,
    /// Path inside the archive we have "entered", empty = root.
    inner: String,
    flat_view: bool,
    arc_sel: BTreeSet<usize>,
    arc_focus: Option<usize>,

    filter: String,
    status: String,
    busy: bool,
    /// Fraction + text of the pass in flight, for the status bar.
    progress: Option<(f32, String)>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,

    sort_col: SortCol,
    ascending: bool,

    create_sources: Vec<PathBuf>,
    create_name: String,
    create_opts: CreateOptions,
    create_password: String,
    create_show_password: bool,
    show_create: bool,

    /// Properties / Info dialog state
    show_info: bool,
    info_title: String,
    info_lines: Vec<(String, String)>,

    /// Delete confirmation dialog state: targets and (if single) is_dir
    delete_targets: Vec<(PathBuf, bool)>,

    /// Checksum / Hash dialog state
    checksum_dialog: Option<ChecksumDialog>,

    /// Password prompt dialog state
    password_dialog: Option<PasswordDialog>,
    cached_passwords: HashMap<PathBuf, String>,

    /// New folder dialog state
    new_folder_dialog: Option<NewFolderDialog>,
    partial_extract_dialog: Option<PartialExtractDialog>,

    /// Dark / Light theme.
    theme: design::Mode,

    /// Bookmarks shown in the sidebar.
    places: Vec<Place>,
    devices: Vec<Place>,
    show_sidebar: bool,

    /// Path inside the archive being previewed (None = viewer closed).
    preview: Option<Preview>,
    focus: Pane,
    /// Type-to-find buffer, like Windows list views.
    typeahead: String,
    typeahead_at: Option<std::time::Instant>,

    /// Where the primary button went down on the titlebar (pending move-vs-click).
    titlebar_press: Option<egui::Pos2>,
    /// We already handed the pointer over to the compositor for a window move.
    titlebar_moving: bool,
    /// We already handed the pointer over to the compositor for a window resize.
    resizing: bool,
}

struct Preview {
    /// Identifies the pending load, so stale worker results are ignored.
    key: String,
    name: String,
    method: Option<String>,
    size: u64,
    text: Option<String>,
    image_texture: Option<egui::TextureHandle>,
    image_dim: Option<[usize; 2]>,
    error: Option<String>,
    loading: bool,
    wrap_text: bool,
    search_query: String,
}

/// Progress callback for a worker thread, throttled so the UI is not flooded.
fn progress_sink(tx: Sender<Msg>) -> impl Fn(&engine::Progress) {
    let last = std::cell::Cell::new(std::time::Instant::now());
    move |p: &engine::Progress| {
        let now = std::time::Instant::now();
        if now.duration_since(last.get()) < std::time::Duration::from_millis(80) {
            return;
        }
        last.set(now);
        let _ = tx.send(Msg::Progress {
            consumed: p.consumed,
            written: p.written,
            total: p.total,
            entries: p.entries,
            entries_total: p.entries_total,
            label: p.label.clone(),
        });
    }
}

/// Report an extraction to the UI: a complete run is a success, a partial run is
/// also a success (files were written and kept) but says exactly what happened.
fn report_extract(
    tx: &Sender<Msg>,
    result: Result<engine::ExtractReport, EngineError>,
    dest: &std::path::Path,
) {
    match result {
        Ok(report) => {
            let _ = tx.send(Msg::Extracted {
                report: Box::new(report),
                dest: dest.to_path_buf(),
            });
        }
        Err(e) => {
            // Nothing is cleaned up here either: whatever reached the disk stays.
            let _ = tx.send(Msg::Done(
                format!(
                    "Extract failed: {e} — anything already written was kept in {}",
                    dest.display()
                ),
                false,
            ));
        }
    }
}

impl ChoabZip {
    fn new(cc: &eframe::CreationContext<'_>, start: Option<PathBuf>) -> Self {
        // White by default; a theme the user picked with Ctrl+D wins and is
        // remembered across restarts (`theme=system` in prefs.conf follows the
        // desktop scheme again).
        let mode = design::load_saved_choice()
            .map(|c| c.mode())
            .unwrap_or(design::Mode::Light);
        design::apply(&cc.egui_ctx, &design::Palette::new(mode));

        let (tx, rx) = channel();
        let mut cwd = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"));

        let mut archive = None;
        let mut start_dir = None;
        if let Some(p) = start {
            let p = p.canonicalize().unwrap_or(p);
            if p.is_dir() {
                start_dir = Some(p);
            } else {
                if let Some(parent) = p.parent() {
                    start_dir = Some(parent.to_path_buf());
                }
                archive = Some(p);
            }
        }
        if let Some(d) = start_dir {
            cwd = d;
        }

        let mut me = Self {
            cwd: cwd.clone(),
            cwd_editing: false,
            cwd_edit_text: cwd.display().to_string(),
            dirs: Vec::new(),
            dir_sel: BTreeSet::new(),
            dir_focus: None,
            archive: None,
            entries: Vec::new(),
            inner: String::new(),
            flat_view: false,
            arc_sel: BTreeSet::new(),
            arc_focus: None,
            filter: String::new(),
            status: "Ready".into(),
            busy: false,
            progress: None,
            tx,
            rx,
            sort_col: SortCol::Name,
            ascending: true,
            create_sources: Vec::new(),
            create_name: "archive.7z".into(),
            create_opts: CreateOptions::default(),
            create_password: String::new(),
            create_show_password: false,
            show_create: false,
            show_info: false,
            info_title: String::new(),
            info_lines: Vec::new(),
            delete_targets: Vec::new(),
            checksum_dialog: None,
            password_dialog: None,
            cached_passwords: HashMap::new(),
            new_folder_dialog: None,
            partial_extract_dialog: None,
            theme: mode,
            places: Place::bookmarks(),
            devices: Place::devices(),
            show_sidebar: true,
            preview: None,
            focus: Pane::Files,
            typeahead: String::new(),
            typeahead_at: None,
            titlebar_press: None,
            titlebar_moving: false,
            resizing: false,
        };
        clean_scratch();
        me.refresh_dir();
        if let Some(a) = archive {
            me.open_path(a);
        }
        me
    }

    // ----------------------------------------------------------------- files

    fn refresh_dir(&mut self) {
        self.dirs.clear();
        let Ok(rd) = std::fs::read_dir(&self.cwd) else {
            return;
        };
        self.dirs = rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let md = e.metadata().ok()?;
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    return None;
                }
                let modified = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| {
                        let secs = d.as_secs();
                        let days = (secs / 86_400) as i64;
                        let (y, m, dd) = civil_from_days(days);
                        let (h, mi) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
                        format!("{y:04}-{m:02}-{dd:02} {h:02}:{mi:02}")
                    })
                    .unwrap_or_default();
                let path = self.cwd.join(&name);
                Some(DirRow {
                    name,
                    is_dir: md.is_dir(),
                    size: md.len(),
                    modified,
                    is_archive: !md.is_dir() && looks_like_archive(&path),
                })
            })
            .collect();
        self.sort_dirs();
    }

    fn sort_dirs(&mut self) {
        let (col, asc) = (self.sort_col, self.ascending);
        self.dirs.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                return if a.is_dir {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }
            let res = match col {
                SortCol::Name => cmp_names(&a.name, &b.name),
                SortCol::Size => a.size.cmp(&b.size),
                SortCol::Packed => std::cmp::Ordering::Equal,
                SortCol::Modified => a.modified.cmp(&b.modified),
            };
            let res = if res == std::cmp::Ordering::Equal {
                cmp_names(&a.name, &b.name)
            } else {
                res
            };
            if asc {
                res
            } else {
                res.reverse()
            }
        });
    }

    fn toggle_sort(&mut self, col: SortCol) {
        if self.sort_col == col {
            self.ascending = !self.ascending;
        } else {
            self.sort_col = col;
            self.ascending = true;
        }
        self.sort_dirs();
    }

    fn enter_dir(&mut self, p: PathBuf) {
        // The filesystem and the archive share one pane, so browsing the
        // filesystem means leaving the archive behind.
        if self.archive.is_some() {
            self.archive = None;
            self.entries.clear();
            self.inner.clear();
            self.arc_sel.clear();
            self.arc_focus = None;
            self.preview = None;
        }
        self.cwd = p.clone();
        self.cwd_edit_text = p.display().to_string();
        self.cwd_editing = false;
        self.refresh_dir();
        self.dir_sel.clear();
        self.dir_focus = None;
        self.typeahead.clear();
    }

    fn go_up(&mut self) {
        // One pane: "Up" acts on whatever is on screen. Inside an archive it
        // climbs the inner folders, and at the archive root it leaves.
        if self.archive.is_some() {
            self.archive_go_up();
            return;
        }
        if let Some(p) = self.cwd.parent() {
            let old = self.cwd.clone();
            self.enter_dir(p.to_path_buf());
            if let Some(name) = old.file_name() {
                let n = name.to_string_lossy();
                if let Some(pos) = self.dirs.iter().position(|r| r.name == n) {
                    self.dir_sel.insert(pos);
                    self.dir_focus = Some(pos);
                }
            }
        }
    }

    // --------------------------------------------------------------- archive

    fn open_path(&mut self, path: PathBuf) {
        if looks_like_archive(&path) {
            self.open_archive(path);
            return;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if is_external_name(&name) || size > VIEW_LIMIT {
            self.open_system(&path);
        } else {
            self.start_preview_file(&path);
        }
    }

    /// Hand a real file to the desktop's default application.
    fn open_system(&mut self, path: &Path) {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        self.preview = None;
        match system_open(path) {
            Ok(()) => self.status = format!("Opened {name} with the system default app"),
            Err(e) => self.status = format!("Could not open {name}: {e}"),
        }
    }

    fn open_archive(&mut self, path: PathBuf) {
        let password = self.cached_passwords.get(&path).cloned();
        self.open_archive_with_password(path, password);
    }

    fn open_archive_with_password(&mut self, path: PathBuf, password: Option<String>) {
        if !looks_like_archive(&path) {
            self.open_path(path);
            return;
        }
        self.busy = true;
        self.status = format!("Reading {}…", path.display());
        self.archive = Some(path.clone());
        self.inner.clear();
        self.arc_sel.clear();
        self.arc_focus = None;
        self.focus = Pane::Archive;
        let tx = self.tx.clone();
        let p_clone = path.clone();
        std::thread::spawn(move || {
            let sink = progress_sink(tx.clone());
            match engine::list_with_report_progress(
                &p_clone,
                password.as_deref(),
                Some(&sink as engine::ProgressSink<'_>),
            ) {
                Ok((es, warning)) => {
                    let _ = tx.send(Msg::Entries(p_clone, es, warning));
                }
                Err(e) => {
                    let _ = tx.send(Msg::EntriesError(p_clone, e));
                }
            }
        });
    }

    fn close_archive(&mut self) {
        self.archive = None;
        self.entries.clear();
        self.inner.clear();
        self.arc_sel.clear();
        self.arc_focus = None;
        self.preview = None;
        self.focus = Pane::Files;
        self.status = "No archive open".into();
    }

    /// Rows for the current inner directory, filtered by the search box.
    fn visible_rows(&self) -> Vec<Row> {
        let f = self.filter.to_lowercase();
        let prefix = self.inner.clone();

        let mut rows: Vec<Row> = Vec::new();
        if self.flat_view {
            for e in self.entries.iter() {
                if !f.is_empty() && !e.path.to_lowercase().contains(&f) {
                    continue;
                }
                rows.push(Row {
                    name: e.path.clone(),
                    is_dir: e.is_dir,
                    size: e.size,
                    packed: e.packed,
                    modified: e.modified.clone().unwrap_or_default(),
                });
            }
        } else {
            for e in self.entries.iter() {
                let Some(rest) = e.path.strip_prefix(&prefix) else {
                    continue;
                };
                if rest.is_empty() {
                    continue;
                }
                let (name, is_dir) = match rest.find('/') {
                    Some(i) => (rest[..i].to_string(), true),
                    None => (rest.to_string(), e.is_dir),
                };
                if !f.is_empty() && !name.to_lowercase().contains(&f) {
                    continue;
                }
                if let Some(existing) = rows.iter_mut().find(|r| r.name == name) {
                    existing.size += if is_dir { 0 } else { e.size };
                    continue;
                }
                rows.push(Row {
                    name,
                    is_dir,
                    size: if is_dir { 0 } else { e.size },
                    packed: if is_dir { 0 } else { e.packed },
                    modified: e.modified.clone().unwrap_or_default(),
                });
            }
        }

        let (col, asc) = (self.sort_col, self.ascending);
        rows.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                return if a.is_dir {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }
            let res = match col {
                SortCol::Name => cmp_names(&a.name, &b.name),
                SortCol::Size => a.size.cmp(&b.size),
                SortCol::Packed => a.packed.cmp(&b.packed),
                SortCol::Modified => a.modified.cmp(&b.modified),
            };
            let res = if res == std::cmp::Ordering::Equal {
                cmp_names(&a.name, &b.name)
            } else {
                res
            };
            if asc {
                res
            } else {
                res.reverse()
            }
        });
        rows
    }

    fn enter_archive_item(&mut self, rows: &[Row], i: usize) {
        let Some(r) = rows.get(i) else { return };
        if r.is_dir {
            self.inner = if self.inner.is_empty() {
                format!("{}/", r.name)
            } else {
                format!("{}{}/", self.inner, r.name)
            };
            self.arc_sel.clear();
            self.arc_focus = None;
            self.typeahead.clear();
        } else {
            let full = if self.flat_view || self.inner.is_empty() {
                r.name.clone()
            } else {
                format!("{}{}", self.inner, r.name)
            };
            self.start_preview_entry(full);
        }
    }

    fn archive_go_up(&mut self) {
        if self.flat_view {
            self.flat_view = false;
            return;
        }
        if self.inner.is_empty() {
            self.close_archive();
            return;
        }
        let trimmed = self.inner.trim_end_matches('/');
        let prev_name = trimmed.rsplit('/').next().unwrap_or("").to_string();
        self.inner = match trimmed.rfind('/') {
            Some(i) => self.inner[..=i].to_string(),
            None => String::new(),
        };
        let rows = self.visible_rows();
        self.arc_sel.clear();
        if let Some(pos) = rows.iter().position(|r| r.name == prev_name) {
            self.arc_sel.insert(pos);
            self.arc_focus = Some(pos);
        }
    }

    // --------------------------------------------------------------- preview

    fn start_preview_file(&mut self, path: &Path) {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let key = path.to_string_lossy().to_string();

        if size > VIEW_LIMIT {
            self.open_system(path);
            return;
        }

        if is_image_name(&name) {
            self.preview = Some(Preview {
                key: key.clone(),
                name: name.clone(),
                method: None,
                size,
                text: None,
                image_texture: None,
                image_dim: None,
                error: None,
                loading: true,
                wrap_text: true,
                search_query: String::new(),
            });
            let p_clone = path.to_path_buf();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let res = std::fs::read(&p_clone).map_err(|e| e.to_string());
                let _ = tx.send(Msg::PreviewImage {
                    key,
                    name,
                    size,
                    method: None,
                    result: res,
                });
            });
            return;
        }

        match std::fs::read(path) {
            Ok(bytes) => match as_text(&bytes) {
                Some(text) => {
                    self.preview = Some(Preview {
                        key,
                        name,
                        method: None,
                        size,
                        text: Some(text),
                        image_texture: None,
                        image_dim: None,
                        error: None,
                        loading: false,
                        wrap_text: true,
                        search_query: String::new(),
                    });
                }
                None => self.open_system(path),
            },
            Err(e) => {
                self.preview = Some(Preview {
                    key,
                    name,
                    method: None,
                    size,
                    text: None,
                    image_texture: None,
                    image_dim: None,
                    error: Some(e.to_string()),
                    loading: false,
                    wrap_text: true,
                    search_query: String::new(),
                });
            }
        }
    }

    fn start_preview_entry(&mut self, entry_path: String) {
        let Some(arc) = self.archive.clone() else { return };
        let entry = self.entries.iter().find(|e| e.path == entry_path);
        let size = entry.map(|e| e.size).unwrap_or(0);
        let method = entry.and_then(|e| e.method.clone());
        let name = entry_path.rsplit('/').next().unwrap_or(&entry_path).to_string();
        let key = entry_path.clone();

        // An archive inside an archive has no path a second instance could
        // read, so unpack it to a temp folder and hand that to a new window.
        if is_archive_name(&name) {
            self.preview = None;
            self.busy = true;
            self.status = format!("Extracting {name} for a new window…");
            let tx = self.tx.clone();
            let display = name.clone();
            let password = self.cached_passwords.get(&arc).cloned();
            std::thread::spawn(move || {
                let result = extract_entry_to_scratch(
                    &arc,
                    &entry_path,
                    password.as_deref(),
                    "nested",
                )
                .and_then(|path| spawn_instance(&path));
                let _ = tx.send(Msg::NestedArchive {
                    name: display,
                    result,
                });
            });
            return;
        }

        if is_image_name(&name) && size <= VIEW_LIMIT {
            self.preview = Some(Preview {
                key: key.clone(),
                name: name.clone(),
                method: method.clone(),
                size,
                text: None,
                image_texture: None,
                image_dim: None,
                error: None,
                loading: true,
                wrap_text: true,
                search_query: String::new(),
            });
            let tx = self.tx.clone();
            let password = self.cached_passwords.get(&arc).cloned();
            std::thread::spawn(move || {
                let res = engine::read_entry_with_password(&arc, &entry_path, password.as_deref())
                    .map_err(|e| e.to_string());
                let _ = tx.send(Msg::PreviewImage {
                    key,
                    name,
                    size,
                    method,
                    result: res,
                });
            });
            return;
        }

        if is_external_name(&name) || size > VIEW_LIMIT {
            self.preview = None;
            self.status = format!("Extracting {name} for the system app…");
            let tx = self.tx.clone();
            let display = name.clone();
            let password = self.cached_passwords.get(&arc).cloned();
            std::thread::spawn(move || {
                let result = extract_entry_to_scratch(
                    &arc,
                    &entry_path,
                    password.as_deref(),
                    "opened",
                )
                .and_then(|path| {
                    system_open(&path).map(|()| path)
                });
                let _ = tx.send(Msg::External {
                    key,
                    name: display,
                    result,
                });
            });
            return;
        }

        self.preview = Some(Preview {
            key: key.clone(),
            name: name.clone(),
            method: method.clone(),
            size,
            text: None,
            image_texture: None,
            image_dim: None,
            error: None,
            loading: true,
            wrap_text: true,
            search_query: String::new(),
        });
        let tx = self.tx.clone();
        let key_clone = entry_path.clone();
        let password = self.cached_passwords.get(&arc).cloned();
        std::thread::spawn(move || match engine::read_entry_with_password(&arc, &key_clone, password.as_deref()) {
            Ok(bytes) => match as_text(&bytes) {
                Some(text) => {
                    let _ = tx.send(Msg::Preview(Ok(text), key_clone));
                }
                None => {
                    let result = extract_entry_to_scratch(&arc, &key_clone, password.as_deref(), "opened")
                        .and_then(|path| system_open(&path).map(|()| path));
                    let _ = tx.send(Msg::External {
                        key: key_clone,
                        name,
                        result,
                    });
                }
            },
            Err(e) => {
                let _ = tx.send(Msg::Preview(Err(e.to_string()), key_clone));
            }
        });
    }

    // ------------------------------------------------------------------ jobs

    fn selected_dir_paths(&self) -> Vec<PathBuf> {
        self.dir_sel
            .iter()
            .filter_map(|&i| self.dirs.get(i).map(|r| self.cwd.join(&r.name)))
            .collect()
    }

    fn selected_entry_paths(&self) -> Vec<String> {
        let rows = self.visible_rows();
        self.arc_sel
            .iter()
            .filter_map(|&i| {
                let item = rows.get(i)?;
                Some(if self.flat_view || self.inner.is_empty() {
                    item.name.clone()
                } else {
                    format!("{}{}", self.inner, item.name)
                })
            })
            .collect()
    }

    fn extract_to(&mut self, dest: PathBuf) {
        let Some(archive) = self.archive.clone() else {
            return;
        };
        self.busy = true;
        self.status = format!("Extracting to {}…", dest.display());
        let tx = self.tx.clone();
        let password = self.cached_passwords.get(&archive).cloned();
        std::thread::spawn(move || {
            let sink = progress_sink(tx.clone());
            report_extract(
                &tx,
                engine::extract_with_progress(
                    &archive,
                    &dest,
                    password.as_deref(),
                    Some(&sink as engine::ProgressSink<'_>),
                ),
                &dest,
            );
        });
    }

    /// Extract only the selected entries (or all if none selected).
    fn extract_selected(&mut self) {
        let Some(archive) = self.archive.clone() else {
            return;
        };
        let entries = self.selected_entry_paths();
        let Some(dest) = rfd::FileDialog::new().set_directory(&self.cwd).pick_folder() else {
            return;
        };
        self.busy = true;
        let password = self.cached_passwords.get(&archive).cloned();
        let tx = self.tx.clone();
        let arc_clone = archive.clone();
        if entries.is_empty() {
            self.status = format!("Extracting all to {}…", dest.display());
            std::thread::spawn(move || {
                let sink = progress_sink(tx.clone());
                report_extract(
                    &tx,
                    engine::extract_with_progress(
                        &arc_clone,
                        &dest,
                        password.as_deref(),
                        Some(&sink as engine::ProgressSink<'_>),
                    ),
                    &dest,
                );
            });
        } else {
            self.status = format!("Extracting {} selected items to {}…", entries.len(), dest.display());
            let prefix = self.inner.clone();
            std::thread::spawn(move || {
                let sink = progress_sink(tx.clone());
                report_extract(
                    &tx,
                    engine::extract_entries_with_progress(
                        &arc_clone,
                        &entries,
                        &prefix,
                        &dest,
                        password.as_deref(),
                        Some(&sink as engine::ProgressSink<'_>),
                    ),
                    &dest,
                );
            });
        }
    }

    fn run_verify(&mut self) {
        let Some(archive) = self.archive.clone() else {
            return;
        };
        self.busy = true;
        self.status = "Testing archive integrity…".into();
        let tx = self.tx.clone();
        let password = self.cached_passwords.get(&archive).cloned();
        std::thread::spawn(move || match engine::verify_with_password(&archive, password.as_deref()) {
            Ok((n, errs)) => {
                let _ = tx.send(Msg::Verified(n, errs));
            }
            Err(e) => {
                let _ = tx.send(Msg::Done(format!("Test failed: {e}"), false));
            }
        });
    }

    fn start_create(&mut self) {
        let name = self.create_name.trim().to_string();
        if name.is_empty() || self.create_sources.is_empty() {
            self.status = "Pick a name and at least one source".into();
            return;
        }
        let target = self.cwd.join(&name);
        let mut opts = self.create_opts.clone();
        if !self.create_password.is_empty() {
            opts.password = Some(self.create_password.clone());
        }
        let sources = std::mem::take(&mut self.create_sources);
        self.busy = true;
        self.status = format!("Creating {}…", target.display());
        self.show_create = false;
        let tx = self.tx.clone();
        std::thread::spawn(move || match engine::create_with_options(&opts, &sources, &target) {
            Ok(()) => {
                let _ = tx.send(Msg::Done(format!("Created {}", target.display()), true));
            }
            Err(e) => {
                let _ = tx.send(Msg::Done(format!("Create failed: {e}"), false));
            }
        });
    }

    fn run_checksum(&mut self, path: PathBuf) {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        self.checksum_dialog = Some(ChecksumDialog {
            path: path.clone(),
            name,
            size,
            crc32: String::new(),
            sha256: String::new(),
            busy: true,
            error: None,
        });
        let tx = self.tx.clone();
        std::thread::spawn(move || match engine::hash_file(&path) {
            Ok((crc32, sha256)) => {
                let _ = tx.send(Msg::ChecksumResult {
                    path,
                    crc32,
                    sha256,
                    size,
                    error: None,
                });
            }
            Err(e) => {
                let _ = tx.send(Msg::ChecksumResult {
                    path,
                    crc32: String::new(),
                    sha256: String::new(),
                    size,
                    error: Some(e.to_string()),
                });
            }
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Entries(path, es, warning) => {
                    if self.archive.as_deref() == Some(path.as_path()) {
                        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("archive");
                        self.status = match &warning {
                            Some(reason) => format!(
                                "{name} — {} entries readable, source ends early ({reason}) — \
                                 extract to keep what is there",
                                es.len()
                            ),
                            None => format!(
                                "{name} — {} entries ({})",
                                es.len(),
                                human(engine::container_size(&path))
                            ),
                        };
                        self.entries = es;
                        self.inner.clear();
                        self.arc_sel.clear();
                        self.arc_focus = None;
                    }
                    self.busy = false;
                    self.progress = None;
                }
                Msg::EntriesError(path, err) => {
                    self.busy = false;
                    self.progress = None;
                    if err.is_password_error() {
                        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("archive");
                        self.password_dialog = Some(PasswordDialog {
                            target: path.clone(),
                            entry: None,
                            prompt: format!("The archive '{}' is encrypted. Enter password:", name),
                            password: String::new(),
                            show_password: false,
                            error: None,
                        });
                        self.status = "Password required".into();
                    } else {
                        self.status = format!("Archive error: {err}");
                        // A multi-volume set deserves an explanation, not a
                        // status line elided to nothing.
                        if let Some(report) = engine::rar_volumes(&path)
                            .filter(|r| r.is_set())
                            .filter(|_| engine::is_rar_file(&path))
                        {
                            let mut lines = report.lines();
                            lines.push((
                                "Why".to_string(),
                                "A multi-volume RAR is one file cut into volumes: the names \
                                 live in volume 1 and the data continues across the others. \
                                 Only the complete set can be extracted."
                                    .to_string(),
                            ));
                            lines.push(("Status".to_string(), report.summary()));
                            self.info_title = "Multi-volume archive".to_string();
                            self.info_lines = lines;
                            self.show_info = true;
                        }
                    }
                }
                Msg::Preview(r, key) => {
                    if let Some(p) = &mut self.preview {
                        if p.key == key {
                            match r {
                                Ok(text) => p.text = Some(text),
                                Err(e) => p.error = Some(e),
                            }
                            p.loading = false;
                        }
                    }
                }
                Msg::PreviewImage {
                    key,
                    name,
                    size: _,
                    method: _,
                    result,
                } => {
                    if let Some(p) = &mut self.preview {
                        if p.key == key {
                            p.loading = false;
                            match result {
                                Ok(bytes) => match image::load_from_memory(&bytes) {
                                    Ok(dyn_img) => {
                                        let rgba = dyn_img.to_rgba8();
                                        let (w, h) = rgba.dimensions();
                                        let color_img = egui::ColorImage::from_rgba_unmultiplied(
                                            [w as usize, h as usize],
                                            &rgba.into_raw(),
                                        );
                                        p.image_dim = Some([w as usize, h as usize]);
                                        p.image_texture = Some(ctx.load_texture(
                                            format!("img_preview_{name}"),
                                            color_img,
                                            egui::TextureOptions::LINEAR,
                                        ));
                                    }
                                    Err(e) => {
                                        p.error = Some(format!("Could not decode image: {e}"));
                                    }
                                },
                                Err(e) => p.error = Some(e),
                            }
                        }
                    }
                }
                Msg::NestedArchive { name, result } => {
                    self.busy = false;
                    self.status = match result {
                        Ok(()) => format!("Opened {name} in a new window"),
                        Err(e) => format!("Could not open {name} in a new window: {e}"),
                    };
                }
                Msg::External { key, name, result } => {
                    if self.preview.as_ref().map(|p| p.key == key).unwrap_or(false) {
                        self.preview = None;
                    }
                    self.status = match result {
                        Ok(path) => format!("Opened {name} — extracted copy at {}", path.display()),
                        Err(e) => format!("Could not open {name}: {e}"),
                    };
                    self.busy = false;
                }
                Msg::Verified(n, errs) => {
                    self.busy = false;
                    if errs.is_empty() {
                        self.status = format!("Everything is Ok — {n} files tested");
                    } else {
                        self.status =
                            format!("{n} tested, {} problem(s): {}", errs.len(), errs.join("; "));
                    }
                }
                Msg::Progress {
                    consumed,
                    written,
                    total,
                    entries,
                    entries_total,
                    label,
                } => {
                    // The source can be fully read while decompression is still
                    // running (a 200 MB tar.gz of zeros is a few KB on disk), so
                    // once the input is consumed the readout switches to the
                    // bytes actually written and the bar parks just short of full
                    // until the run really ends.
                    let (frac, detail) = if total > 0 && consumed >= total && written > 0 {
                        (
                            0.99,
                            format!("unpacking · {} written", human(written)),
                        )
                    } else if total > 0 && consumed >= total {
                        // Source fully read, still decompressing: nothing is
                        // written during a listing pass, so say what it is doing.
                        (0.99, "decompressing…".to_string())
                    } else if total > 0 {
                        (
                            (consumed as f32 / total as f32).clamp(0.0, 1.0),
                            format!(
                                "{}% · {} / {}",
                                consumed * 100 / total,
                                human(consumed),
                                human(total)
                            ),
                        )
                    } else if entries_total > 0 {
                        (
                            (entries as f32 / entries_total as f32).clamp(0.0, 1.0),
                            format!("{entries}/{entries_total} · {} written", human(written)),
                        )
                    } else if written > 0 {
                        (0.0, format!("{} written", human(written)))
                    } else {
                        (0.0, format!("{entries} done"))
                    };
                    let detail = if label.is_empty() {
                        detail
                    } else {
                        format!("{detail} · {label}")
                    };
                    self.progress = Some((frac, detail));
                    ctx.request_repaint();
                }
                Msg::Extracted { report, dest } => {
                    self.busy = false;
                    self.progress = None;
                    self.status = if report.is_partial() {
                        format!(
                            "{} — kept in {} (nothing deleted)",
                            report.summary(),
                            dest.display()
                        )
                    } else {
                        report.summary()
                    };
                    self.refresh_dir();
                    if let Some(a) = self.archive.clone() {
                        self.open_archive(a);
                    }
                    if report.is_partial() {
                        let incomplete: Vec<PathBuf> = report
                            .incomplete
                            .iter()
                            .filter_map(|name| engine::dest_for_entry(&dest, name))
                            .filter(|p| p.exists())
                            .collect();
                        self.partial_extract_dialog = Some(PartialExtractDialog {
                            reason: report
                                .stopped
                                .clone()
                                .unwrap_or_else(|| "the source ended early".to_string()),
                            dest: dest.clone(),
                            incomplete,
                            corrupt_copy: report.corrupt_copy.clone(),
                            extracted: report.entries,
                        });
                    }
                }
                Msg::Done(s, ok) => {
                    self.status = s;
                    self.busy = false;
                    self.progress = None;
                    if ok {
                        self.refresh_dir();
                        if let Some(a) = self.archive.clone() {
                            self.open_archive(a);
                        }
                    }
                }
                Msg::ChecksumResult {
                    path: _,
                    crc32,
                    sha256,
                    size: _,
                    error,
                } => {
                    if let Some(dlg) = &mut self.checksum_dialog {
                        dlg.busy = false;
                        dlg.crc32 = crc32;
                        dlg.sha256 = sha256;
                        dlg.error = error;
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------- keyboard

    fn handle_keys(&mut self, ui: &egui::Ui, rows: &[Row]) {
        let (back, enter, up, down, home, end, del, f5, f7, escape) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Backspace),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::End),
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::F5),
                i.key_pressed(egui::Key::F7),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        let ctrl_a = ui.input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::A));
        let ctrl_d = ui.input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::D));
        let ctrl_w = ui.input(|i| {
            (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::W)
        });
        let ctrl_pgup = ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::PageUp));
        let ctrl_pgdn = ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::PageDown));

        if escape {
            if self.show_create {
                self.show_create = false;
            } else if self.show_info {
                self.show_info = false;
            } else if self.password_dialog.is_some() {
                self.password_dialog = None;
            } else if self.checksum_dialog.is_some() {
                self.checksum_dialog = None;
            } else if self.new_folder_dialog.is_some() {
                self.new_folder_dialog = None;
            } else if !self.delete_targets.is_empty() {
                self.delete_targets.clear();
            } else if self.preview.is_some() {
                self.preview = None;
            } else {
                self.dir_sel.clear();
                self.arc_sel.clear();
            }
        }

        if ctrl_d {
            self.toggle_theme(ui.ctx());
        }

        // Ctrl+W closes the archive that is open and leaves the window alone;
        // with nothing open it closes the app.
        if ctrl_w {
            if self.archive.is_some() {
                self.close_archive();
                self.status = "Archive closed".to_string();
            } else {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        if f5 {
            self.refresh_dir();
            if let Some(a) = self.archive.clone() {
                self.open_archive(a);
            }
        }

        if f7 {
            self.new_folder_dialog = Some(NewFolderDialog {
                name: String::new(),
                error: None,
            });
        }

        match self.focus {
            Pane::Files => {
                let n = self.dirs.len();
                if ctrl_a && n > 0 {
                    self.dir_sel = (0..n).collect();
                }
                if del && !self.dir_sel.is_empty() {
                    let targets: Vec<(PathBuf, bool)> = self
                        .dir_sel
                        .iter()
                        .filter_map(|&i| self.dirs.get(i).map(|r| (self.cwd.join(&r.name), r.is_dir)))
                        .collect();
                    if !targets.is_empty() {
                        self.delete_targets = targets;
                    }
                }
                if n > 0 {
                    if down {
                        let next = match self.dir_focus {
                            Some(i) if i + 1 < n => i + 1,
                            Some(_) => 0,
                            None => 0,
                        };
                        self.dir_focus = Some(next);
                        if !ctrl {
                            self.dir_sel.clear();
                            self.dir_sel.insert(next);
                        }
                    }
                    if up {
                        let prev = match self.dir_focus {
                            Some(i) if i > 0 => i - 1,
                            Some(_) => n - 1,
                            None => n - 1,
                        };
                        self.dir_focus = Some(prev);
                        if !ctrl {
                            self.dir_sel.clear();
                            self.dir_sel.insert(prev);
                        }
                    }
                    if home {
                        self.dir_focus = Some(0);
                        if !ctrl {
                            self.dir_sel.clear();
                            self.dir_sel.insert(0);
                        }
                    }
                    if end {
                        self.dir_focus = Some(n - 1);
                        if !ctrl {
                            self.dir_sel.clear();
                            self.dir_sel.insert(n - 1);
                        }
                    }
                }
                if ctrl_pgup || back {
                    self.go_up();
                }
                if enter || ctrl_pgdn {
                    if let Some(i) = self.dir_focus {
                        if let Some(r) = self.dirs.get(i) {
                            let p = self.cwd.join(&r.name);
                            if r.is_dir {
                                self.enter_dir(p);
                            } else {
                                self.open_path(p);
                            }
                        }
                    }
                }
            }
            Pane::Archive => {
                let n = rows.len();
                if ctrl_a && n > 0 {
                    self.arc_sel = (0..n).collect();
                }
                if n > 0 {
                    if down {
                        let next = match self.arc_focus {
                            Some(i) if i + 1 < n => i + 1,
                            Some(_) => 0,
                            None => 0,
                        };
                        self.arc_focus = Some(next);
                        if !ctrl {
                            self.arc_sel.clear();
                            self.arc_sel.insert(next);
                        }
                    }
                    if up {
                        let prev = match self.arc_focus {
                            Some(i) if i > 0 => i - 1,
                            Some(_) => n - 1,
                            None => n - 1,
                        };
                        self.arc_focus = Some(prev);
                        if !ctrl {
                            self.arc_sel.clear();
                            self.arc_sel.insert(prev);
                        }
                    }
                    if home {
                        self.arc_focus = Some(0);
                        if !ctrl {
                            self.arc_sel.clear();
                            self.arc_sel.insert(0);
                        }
                    }
                    if end {
                        self.arc_focus = Some(n - 1);
                        if !ctrl {
                            self.arc_sel.clear();
                            self.arc_sel.insert(n - 1);
                        }
                    }
                }
                if ctrl_pgup || back {
                    self.archive_go_up();
                }
                if enter || ctrl_pgdn {
                    if let Some(i) = self.arc_focus {
                        self.enter_archive_item(rows, i);
                    }
                }
            }
        }
    }

    fn typeahead(&mut self, ui: &egui::Ui, names: &[String]) {
        let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
        let mut changed = false;
        for ev in events {
            if let egui::Event::Text(t) = ev {
                let printable = !t.is_empty()
                    && t.chars().all(|c| !c.is_control())
                    && !t.contains(' ');
                if printable {
                    self.typeahead.push_str(&t);
                    self.typeahead_at = Some(std::time::Instant::now());
                    changed = true;
                }
            }
        }
        if let Some(t) = self.typeahead_at {
            if t.elapsed().as_millis() > 900 {
                self.typeahead.clear();
                self.typeahead_at = None;
            }
        }
        if !changed || self.typeahead.is_empty() {
            return;
        }
        let needle = self.typeahead.to_lowercase();
        if let Some(i) = names
            .iter()
            .position(|n| n.to_lowercase().starts_with(&needle))
        {
            match self.focus {
                Pane::Files => {
                    self.dir_sel.clear();
                    self.dir_sel.insert(i);
                    self.dir_focus = Some(i);
                }
                Pane::Archive => {
                    self.arc_sel.clear();
                    self.arc_sel.insert(i);
                    self.arc_focus = Some(i);
                }
            }
        }
    }
}

// ------------------------------------------------------------------ helpers

/// Extensions that mark a file as an archive, used both for files on disk and
/// for entries inside an archive (where only the name is available).
const ARCHIVE_EXTS: &[&str] = &[
    ".7z", ".cb7", ".zip", ".jar", ".war", ".tar", ".gz", ".tgz", ".xz", ".txz", ".bz2",
    ".tbz", ".tbz2", ".zst", ".tzst", ".rar",
];

fn is_archive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ARCHIVE_EXTS.iter().any(|e| lower.ends_with(e))
}

fn looks_like_archive(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if is_archive_name(&name) {
        return true;
    }
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 8];
    if f.read_exact(&mut head).is_err() {
        return false;
    }
    head.starts_with(b"7z\xBC\xAF\x27\x1C")
        || head.starts_with(b"PK\x03\x04")
        || head.starts_with(&[0x1F, 0x8B])
        || head.starts_with(b"\xFD7zXZ\0")
        || head.starts_with(b"BZh ")
        || head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD])
        || head.starts_with(b"Rar!")
}

/// File types that should be handed to the desktop rather than decoded here.
fn is_external_name(name: &str) -> bool {
    name.to_ascii_lowercase()
        .rsplit_once('.')
        .map(|(_, ext)| EXTERNAL_EXTS.contains(&ext))
        .unwrap_or(false)
}

/// Spawn the desktop's default handler. No shell, no output redirects: this
/// runs only when the user explicitly opens a file.
fn system_open(path: &Path) -> Result<(), String> {
    // `gio open` goes through the OpenURI portal, which is the only thing that
    // reaches the *host* desktop from inside a sandbox. `xdg-open` starts, exits
    // 0 and does nothing there, so it is only a fallback for systems without gio.
    let flatpak = Path::new("/.flatpak-info").exists();
    let mut attempts: Vec<(&str, &[&str])> = vec![("gio", &["open"][..])];
    if !flatpak {
        attempts.push(("kde-open", &[][..]));
        attempts.push(("xdg-open", &[][..]));
    }
    let mut last = String::new();
    for (prog, args) in attempts {
        match std::process::Command::new(prog)
            .args(args)
            .arg(path)
            .status()
        {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => last = format!("{prog}: exit {status}"),
            Err(e) => last = format!("{prog}: {e}"),
        }
    }
    Err(format!(
        "no application could be opened for it ({last}); the file is at {}",
        path.display()
    ))
}

/// Root of the scratch space used for archive entries that need a real path.
///
/// Deliberately *not* `std::env::temp_dir()`: inside a Flatpak `/tmp` is a
/// private tmpfs, so anything written there is invisible to host applications
/// (and to a second instance of this app), which is exactly why "open with the
/// system app" used to do nothing. `$XDG_CACHE_HOME` lives in the home
/// directory the sandbox shares, so the desktop can actually read the file.
fn scratch_root() -> Option<PathBuf> {
    std::env::var("XDG_CACHE_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".cache"))
        })
        .map(|base| base.join("choab-zip"))
}

/// A fresh folder inside the scratch space, e.g. `opened/1234-987654321`.
fn scratch_dir(kind: &str) -> Result<PathBuf, String> {
    let root = scratch_root().ok_or_else(|| "no cache directory".to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = root
        .join(kind)
        .join(format!("{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Drop scratch folders older than three days so the cache does not grow
/// without bound. Silent by design: nothing the user asked for is touched.
fn clean_scratch() {
    let Some(root) = scratch_root() else {
        return;
    };
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(3 * 24 * 60 * 60);
    for kind in ["opened", "nested"] {
        let Ok(entries) = std::fs::read_dir(root.join(kind)) else {
            continue;
        };
        for entry in entries.flatten() {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .map(|m| m < cutoff)
                .unwrap_or(false);
            if old {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

/// Stream one entry out of an archive into a fresh scratch folder and return
/// the path of the file, keeping the entry's own name so the desktop picks the
/// right application for it.
///
/// Streams: a 2 GB video is never held in memory, it is copied straight to disk.
fn extract_entry_to_scratch(
    archive: &Path,
    entry_path: &str,
    password: Option<&str>,
    kind: &str,
) -> Result<PathBuf, String> {
    let dir = scratch_dir(kind)?;
    engine::extract_entries_with_password(archive, &[entry_path.to_string()], "", &dir, password)
        .map_err(|e| e.to_string())?;
    engine::dest_for_entry(&dir, entry_path)
        .filter(|p| p.is_file())
        .ok_or_else(|| format!("could not write {} into {}", entry_path, dir.display()))
}

/// Start another copy of this executable with `path` loaded.
fn spawn_instance(path: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}


fn cmp_names(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn looks_like_text(bytes: &[u8]) -> bool {
    let n = bytes.len().min(64 * 1024);
    !bytes[..n]
        .iter()
        .any(|b| *b == 0 || *b < 0x09 || (*b > 0x0d && *b < 0x20))
}

fn as_text(bytes: &[u8]) -> Option<String> {
    if !looks_like_text(bytes) {
        return None;
    }
    let n = bytes.len().min(RENDER_LIMIT);
    let slice = &bytes[..n];
    let s = String::from_utf8_lossy(slice).to_string();
    Some(if bytes.len() > n {
        format!("{s}\n\n… truncated at {} (of {})", human(n as u64), human(bytes.len() as u64))
    } else {
        s
    })
}

// ----------------------------------------------------------------- UI parts

impl ChoabZip {
    fn show_properties(&mut self) {
        self.info_lines.clear();
        if self.focus == Pane::Archive || self.dir_sel.is_empty() {
            if let Some(arc) = &self.archive {
                self.info_title = arc.file_name().and_then(|s| s.to_str()).unwrap_or("Archive").into();
                let fmt = engine::detect(arc);
                let total_size: u64 = self.entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
                let count_files = self.entries.iter().filter(|e| !e.is_dir).count();
                let count_dirs = self.entries.iter().filter(|e| e.is_dir).count();
                self.info_lines.push(("Path".into(), arc.display().to_string()));
                self.info_lines.push(("Type".into(), fmt.label().to_uppercase()));
                self.info_lines.push(("Physical Size".into(), human(engine::container_size(arc))));
                self.info_lines.push(("Unpacked Size".into(), human(total_size)));
                self.info_lines.push(("Files".into(), count_files.to_string()));
                self.info_lines.push(("Folders".into(), count_dirs.to_string()));
                self.show_info = true;
                return;
            }
        }

        if self.dir_sel.len() > 1 {
            self.info_title = format!("{} items selected", self.dir_sel.len());
            let paths = self.selected_dir_paths();
            let total_size: u64 = paths.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
            self.info_lines.push(("Location".into(), self.cwd.display().to_string()));
            self.info_lines.push(("Selected Count".into(), self.dir_sel.len().to_string()));
            self.info_lines.push(("Total Size".into(), format!("{} ({} bytes)", human(total_size), total_size)));
            self.show_info = true;
            return;
        }

        if let Some(&idx) = self.dir_sel.iter().next() {
            if let Some(r) = self.dirs.get(idx) {
                self.info_title = r.name.clone();
                let p = self.cwd.join(&r.name);
                self.info_lines.push(("Path".into(), p.display().to_string()));
                self.info_lines.push(("Type".into(), if r.is_dir { "Directory".into() } else if r.is_archive { "Archive".into() } else { "File".into() }));
                if !r.is_dir {
                    self.info_lines.push(("Size".into(), format!("{} ({} bytes)", human(r.size), r.size)));
                }
                self.info_lines.push(("Modified".into(), r.modified.clone()));
                self.show_info = true;
            }
        }
    }


    fn ui_dialogs(&mut self, ctx: &egui::Context) {
        // 1. Create / Add to Archive dialog
        if self.show_create {
            let mut open = self.show_create;
            let mut close_dialog = false;
            egui::Window::new("Add to Archive")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(520.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Archive:").strong().size(13.0));
                        ui.add(egui::TextEdit::singleline(&mut self.create_name).desired_width(380.0));
                    });
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Archive format:").strong().size(13.0));
                        for f in [
                            Format::SevenZ,
                            Format::Zip,
                            Format::Tar,
                            Format::TarGz,
                            Format::TarXz,
                            Format::TarBz2,
                            Format::TarZst,
                        ] {
                            let was = self.create_opts.format;
                            if ui.selectable_label(was == f, f.label()).clicked() {
                                self.create_opts.format = f;
                                let stem = Path::new(&self.create_name)
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("archive");
                                let ext = match f {
                                    Format::SevenZ => ".7z",
                                    Format::Zip => ".zip",
                                    Format::Tar => ".tar",
                                    Format::TarGz => ".tar.gz",
                                    Format::TarXz => ".tar.xz",
                                    Format::TarBz2 => ".tar.bz2",
                                    Format::TarZst => ".tar.zst",
                                    _ => ".7z",
                                };
                                let base = if stem.ends_with(".tar") {
                                    &stem[..stem.len() - 4]
                                } else {
                                    stem
                                };
                                self.create_name = format!("{base}{ext}");
                            }
                        }
                    });
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Compression level:").strong().size(13.0));
                        for lvl in [
                            CompressionLevel::Store,
                            CompressionLevel::Fastest,
                            CompressionLevel::Fast,
                            CompressionLevel::Normal,
                            CompressionLevel::Maximum,
                            CompressionLevel::Ultra,
                        ] {
                            ui.selectable_value(&mut self.create_opts.level, lvl, lvl.label());
                        }
                    });
                    ui.add_space(4.0);

                    if self.create_opts.format == Format::SevenZ {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Compression method:").strong().size(13.0));
                            egui::ComboBox::from_id_salt("sevenz_method_combo")
                                .selected_text(self.create_opts.sevenz_method.label())
                                .show_ui(ui, |ui| {
                                    for m in SevenZMethod::ALL {
                                        ui.selectable_value(&mut self.create_opts.sevenz_method, m, m.label());
                                    }
                                });
                            ui.checkbox(&mut self.create_opts.solid, "Solid archive")
                                .on_hover_text("Group files into solid blocks for higher compression ratio");
                        });
                    } else if self.create_opts.format == Format::Zip {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Compression method:").strong().size(13.0));
                            egui::ComboBox::from_id_salt("zip_method_combo")
                                .selected_text(self.create_opts.zip_method.label())
                                .show_ui(ui, |ui| {
                                    for m in ZipMethod::ALL {
                                        ui.selectable_value(&mut self.create_opts.zip_method, m, m.label());
                                    }
                                });
                        });
                    }
                    ui.add_space(4.0);

                    if self.create_opts.format == Format::SevenZ {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Encryption:").strong().size(13.0));
                                ui.label(egui::RichText::new("(AES-256)").italics().small());
                            });
                            ui.horizontal(|ui| {
                                ui.label("Password:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.create_password)
                                        .password(!self.create_show_password)
                                        .desired_width(220.0),
                                );
                                ui.checkbox(&mut self.create_show_password, "Show password");
                            });
                            let has_pw = !self.create_password.is_empty();
                            ui.add_enabled(
                                has_pw,
                                egui::Checkbox::new(&mut self.create_opts.encrypt_header, "Encrypt file names (header encryption)")
                            ).on_hover_text("Encrypts directory listings so entry names are hidden without the password");
                        });
                        ui.add_space(4.0);
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("Source files ({}):", self.create_sources.len())).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Clear").clicked() {
                                self.create_sources.clear();
                            }
                            if ui.button("+ Add folder…").clicked() {
                                if let Some(d) = rfd::FileDialog::new().set_directory(&self.cwd).pick_folder() {
                                    self.create_sources.push(d);
                                }
                            }
                            if ui.button("+ Add files…").clicked() {
                                if let Some(fs) = rfd::FileDialog::new().set_directory(&self.cwd).pick_files() {
                                    self.create_sources.extend(fs);
                                }
                            }
                        });
                    });

                    if self.create_sources.is_empty() {
                        ui.label(
                            egui::RichText::new("No files selected. Add files or folders above.")
                                .italics()
                                .color(egui::Color32::from_gray(130)),
                        );
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("create_sources_scroll")
                            .max_height(100.0)
                            .show(ui, |ui| {
                                for s in &self.create_sources {
                                    ui.label(egui::RichText::new(s.display().to_string()).monospace().small());
                                }
                            });
                    }

                    ui.separator();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Cancel").clicked() {
                            close_dialog = true;
                        }
                        if ui.add_enabled(!self.create_sources.is_empty() && !self.create_name.trim().is_empty(), egui::Button::new("OK").min_size(egui::vec2(70.0, 24.0))).clicked() {
                            self.start_create();
                            close_dialog = true;
                        }
                    });
                });
            if !open || close_dialog {
                self.show_create = false;
            }
        }

        // 2. Properties / Info dialog
        if self.show_info {
            let mut open = self.show_info;
            let mut close_clicked = false;
            egui::Window::new(format!("Properties: {}", self.info_title))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_size([460.0, 260.0])
                .max_width(720.0)
                .show(ctx, |ui| {
                    // Fixed key column, value fitted to what is left: a long
                    // path or archive name keeps its ends instead of widening
                    // the dialog or running over the next column. The full text
                    // is one hover away.
                    const KEY_W: f32 = 108.0;
                    let p = self.palette();
                    for (k, v) in &self.info_lines {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [KEY_W, 16.0],
                                egui::Label::new(
                                    egui::RichText::new(k).strong().size(12.0).color(p.text_muted),
                                ),
                            );
                            let value_max = (ui.available_width() - 8.0).max(120.0);
                            let galley = design::elide_middle(
                                ui,
                                v,
                                egui::FontId::monospace(12.0),
                                p.text,
                                value_max,
                            );
                            ui.add(egui::Label::new(galley)).on_hover_text(v.clone());
                        });
                        ui.add_space(3.0);
                    }
                    ui.separator();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close_clicked = true;
                        }
                    });
                });
            self.show_info = open && !close_clicked;
        }

        // 2b. Partial extraction: nothing was deleted, the user decides.
        if let Some(dlg) = self.partial_extract_dialog.take() {
            let mut open = true;
            let mut keep = false;
            let mut remove = false;
            egui::Window::new("Extraction incomplete")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(520.0)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "{} complete file(s) were extracted to:",
                        dlg.extracted
                    ));
                    ui.label(egui::RichText::new(dlg.dest.display().to_string()).small().monospace());
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Stopped because:").strong());
                    ui.label(egui::RichText::new(&dlg.reason).small());

                    if !dlg.incomplete.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "{} incomplete file(s) are still on disk:",
                                dlg.incomplete.len()
                            ))
                            .strong(),
                        );
                        egui::ScrollArea::vertical()
                            .id_salt("partial_files_scroll")
                            .max_height(110.0)
                            .show(ui, |ui| {
                                for path in &dlg.incomplete {
                                    let name = path
                                        .strip_prefix(&dlg.dest)
                                        .unwrap_or(path)
                                        .display()
                                        .to_string();
                                    ui.label(egui::RichText::new(name).monospace().small());
                                }
                            });
                    }
                    if let Some(copy) = &dlg.corrupt_copy {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "A copy of the damaged archive was saved as:"
                            ))
                            .strong(),
                        );
                        ui.label(
                            egui::RichText::new(
                                copy.file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| copy.display().to_string()),
                            )
                            .monospace()
                            .small(),
                        );
                    }

                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Nothing has been deleted.")
                            .small()
                            .color(self.palette().text_faint),
                    );
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Keep files").clicked() {
                            keep = true;
                        }
                        let can_remove = !dlg.incomplete.is_empty() || dlg.corrupt_copy.is_some();
                        if ui
                            .add_enabled(can_remove, egui::Button::new("Remove incomplete"))
                            .clicked()
                        {
                            remove = true;
                        }
                    });
                });

            if remove {
                let mut removed = 0usize;
                for path in &dlg.incomplete {
                    if std::fs::remove_file(path).is_ok() {
                        removed += 1;
                    }
                }
                let copy_removed = dlg
                    .corrupt_copy
                    .as_ref()
                    .map(|c| std::fs::remove_file(c).is_ok())
                    .unwrap_or(false);
                let mut note = format!("Removed {removed} incomplete file(s)");
                if copy_removed {
                    note.push_str(" and the damaged copy");
                }
                self.status = note;
                self.refresh_dir();
            } else if keep {
                self.status = format!(
                    "Kept {} incomplete file(s) in {}",
                    dlg.incomplete.len(),
                    dlg.dest.display()
                );
            } else if !open {
                // Closed with the window button: same as keeping.
                self.status = format!(
                    "Kept {} incomplete file(s) in {}",
                    dlg.incomplete.len(),
                    dlg.dest.display()
                );
            }
        }

        // 3. Delete confirmation dialog (single or multiple)
        if !self.delete_targets.is_empty() {
            let mut open = true;
            let mut do_delete = false;
            let count = self.delete_targets.len();
            egui::Window::new("Confirm Delete")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    if count == 1 {
                        let (target, is_dir) = &self.delete_targets[0];
                        let file_name = target.file_name().and_then(|s| s.to_str()).unwrap_or("item");
                        let kind = if *is_dir { "folder and all its contents" } else { "file" };
                        ui.label(format!("Are you sure you want to delete this {kind}?"));
                        ui.label(egui::RichText::new(file_name).strong());
                    } else {
                        ui.label(format!("Are you sure you want to delete these {count} items?"));
                        egui::ScrollArea::vertical()
                            .id_salt("del_targets_scroll")
                            .max_height(100.0)
                            .show(ui, |ui| {
                                for (target, _) in &self.delete_targets {
                                    let name = target.file_name().and_then(|s| s.to_str()).unwrap_or("item");
                                    ui.label(egui::RichText::new(name).monospace().small());
                                }
                            });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            do_delete = true;
                        }
                        if ui.button("Cancel").clicked() {
                            self.delete_targets.clear();
                        }
                    });
                });
            if do_delete {
                let mut deleted_count = 0;
                let mut last_err = None;
                for (target, is_dir) in self.delete_targets.drain(..) {
                    let res = if is_dir {
                        std::fs::remove_dir_all(&target)
                    } else {
                        std::fs::remove_file(&target)
                    };
                    match res {
                        Ok(()) => deleted_count += 1,
                        Err(e) => last_err = Some(e.to_string()),
                    }
                }
                if let Some(err) = last_err {
                    self.status = format!("Deleted {deleted_count} items (error: {err})");
                } else {
                    self.status = format!("Deleted {deleted_count} items");
                }
                self.refresh_dir();
            } else if !open {
                self.delete_targets.clear();
            }
        }

        // 4. Checksum / Hash dialog
        if let Some(dlg) = self.checksum_dialog.take() {
            let mut open = true;
            let mut close = false;
            egui::Window::new("Checksums / Hashes")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(460.0)
                .show(ctx, |ui| {
                    egui::Grid::new("checksum_grid")
                        .num_columns(3)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("File:").strong());
                            ui.label(egui::RichText::new(dlg.path.display().to_string()).small());
                            ui.label("");
                            ui.end_row();

                            ui.label(egui::RichText::new("Size:").strong());
                            ui.label(format!("{} ({} bytes)", human(dlg.size), dlg.size));
                            ui.label("");
                            ui.end_row();

                            ui.label(egui::RichText::new("CRC-32:").strong());
                            if dlg.busy && dlg.crc32.is_empty() {
                                ui.spinner();
                                ui.label("");
                            } else {
                                ui.label(egui::RichText::new(&dlg.crc32).monospace().strong());
                                if ui.button("Copy").clicked() {
                                    ui.ctx().copy_text(dlg.crc32.clone());
                                }
                            }
                            ui.end_row();

                            ui.label(egui::RichText::new("SHA-256:").strong());
                            if dlg.busy && dlg.sha256.is_empty() {
                                ui.spinner();
                                ui.label("");
                            } else {
                                ui.label(egui::RichText::new(&dlg.sha256).monospace().small());
                                if ui.button("Copy").clicked() {
                                    ui.ctx().copy_text(dlg.sha256.clone());
                                }
                            }
                            ui.end_row();
                        });

                    if let Some(err) = &dlg.error {
                        ui.colored_label(egui::Color32::from_rgb(220, 38, 38), err);
                    }

                    ui.separator();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                        if !dlg.busy && !dlg.crc32.is_empty() && !dlg.sha256.is_empty() {
                            if ui.button("Copy All").clicked() {
                                let all = format!("Name: {}\nSize: {}\nCRC32: {}\nSHA256: {}\n", dlg.name, dlg.size, dlg.crc32, dlg.sha256);
                                ui.ctx().copy_text(all);
                            }
                        }
                    });
                });

            if !close && open {
                self.checksum_dialog = Some(dlg);
            }
        }

        // 5. Password prompt dialog
        if let Some(mut dlg) = self.password_dialog.take() {
            let mut open = true;
            let mut submit = false;
            let mut cancel = false;
            egui::Window::new("Password Required")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(380.0)
                .show(ctx, |ui| {
                    ui.label(&dlg.prompt);
                    ui.add_space(4.0);
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut dlg.password)
                            .password(!dlg.show_password)
                            .desired_width(320.0),
                    );
                    if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                    ui.checkbox(&mut dlg.show_password, "Show password");
                    if let Some(err) = &dlg.error {
                        ui.colored_label(egui::Color32::from_rgb(220, 38, 38), err);
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            submit = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });

            if submit {
                let target = dlg.target.clone();
                let pw = dlg.password.clone();
                self.cached_passwords.insert(target.clone(), pw.clone());
                if let Some(entry_path) = dlg.entry {
                    self.status = format!("Opening {}…", entry_path);
                    self.start_preview_entry(entry_path);
                } else {
                    self.open_archive_with_password(target, Some(pw));
                }
            } else if !cancel && open {
                self.password_dialog = Some(dlg);
            }
        }

        // 6. New Folder dialog
        if let Some(mut dlg) = self.new_folder_dialog.take() {
            let mut open = true;
            let mut submit = false;
            let mut cancel = false;
            egui::Window::new("Create New Folder")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(340.0)
                .show(ctx, |ui| {
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut dlg.name).desired_width(300.0),
                    );
                    if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                    if let Some(err) = &dlg.error {
                        ui.colored_label(egui::Color32::from_rgb(220, 38, 38), err);
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            submit = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });

            if submit {
                let name = dlg.name.trim().to_string();
                if name.is_empty() {
                    dlg.error = Some("Folder name cannot be empty".into());
                    self.new_folder_dialog = Some(dlg);
                } else {
                    let path = self.cwd.join(&name);
                    match std::fs::create_dir(&path) {
                        Ok(()) => {
                            self.status = format!("Created folder '{}'", name);
                            self.refresh_dir();
                        }
                        Err(e) => {
                            dlg.error = Some(format!("Failed to create folder: {e}"));
                            self.new_folder_dialog = Some(dlg);
                        }
                    }
                }
            } else if !cancel && open {
                self.new_folder_dialog = Some(dlg);
            }
        }
    }

}

/// Window/taskbar icon, embedded at compile time.
fn window_icon() -> egui::IconData {
    const PNG: &[u8] = include_bytes!("../assets/icon.png");
    match eframe::icon_data::from_png_bytes(PNG) {
        Ok(icon) => icon,
        Err(_) => egui::IconData {
            rgba: vec![0; 4 * 64 * 64],
            width: 64,
            height: 64,
        },
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Headless engine check: list + extract, for CI and codec verification.
    if args.len() >= 3 && args[1] == "--selftest" {
        let path = Path::new(&args[2]);
        let fmt = engine::detect(path);
        println!("format: {}", fmt.label());
        // Same streaming path the UI uses, with progress echoed to stderr so a
        // headless run shows it is actually being reported.
        let sink = |p: &engine::Progress| {
            let pct = if p.total > 0 {
                format!("{}%", p.consumed * 100 / p.total)
            } else {
                format!("{} done", p.entries)
            };
            eprintln!("  progress {pct} · {}", p.label);
        };
        match engine::list_with_report_progress(
            path,
            None,
            Some(&sink as engine::ProgressSink<'_>),
        )
        .map(|(es, warning)| {
            if let Some(w) = &warning {
                eprintln!("  warning: source ends early ({w})");
            }
            es
        }) {
            Ok(es) => {
                println!("entries: {}", es.len());
                for e in es.iter().take(10) {
                    println!("  {:>10}  {}", e.size, e.path);
                }
            }
            Err(e) => {
                eprintln!("list error: {e}");
                std::process::exit(1);
            }
        }
        let tmp = std::env::temp_dir().join(format!("choab-zip-selftest-{}", std::process::id()));
        match engine::extract_with_progress(
            path,
            &tmp,
            None,
            Some(&sink as engine::ProgressSink<'_>),
        ) {
            Ok(report) => println!("extract: {} -> {}", report.summary(), tmp.display()),
            Err(e) => {
                eprintln!("extract error: {e}");
                std::process::exit(1);
            }
        }
        // Keep the output for inspection when asked (CHOAB_KEEP_SELFTEST=1).
        if std::env::var_os("CHOAB_KEEP_SELFTEST").is_some() {
            println!("kept: {}", tmp.display());
        } else {
            let _ = std::fs::remove_dir_all(&tmp);
        }
        return Ok(());
    }

    let start = args.get(1).map(PathBuf::from);

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // One content pane instead of two: no need for a wide window.
            .with_inner_size([1040.0, 720.0])
            .with_min_inner_size([720.0, 460.0])
            .with_title("CHOAB Zip")
            .with_app_id(window_app_id())
            .with_icon(window_icon())
            .with_decorations(false)
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "choab-zip",
        opts,
        Box::new(move |cc| Ok(Box::new(ChoabZip::new(cc, start)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_known_desktop_types() {
        assert!(is_external_name("manual.PDF"));
        assert!(is_external_name("clip.mp4"));
        assert!(!is_external_name("photo.jpeg"));
        assert!(is_image_name("photo.jpeg"));
        assert!(is_image_name("banner.PNG"));
        assert!(is_image_name("graphic.webp"));
        assert!(!is_external_name("notes.txt"));
        assert!(!is_external_name("Makefile"));
    }

    #[test]
    fn detects_text_and_binary() {
        assert!(looks_like_text(b"hello\nworld\n"));
        assert!(!looks_like_text(b"\x7fELF\x02\x01\x01\x00"));
        assert!(!looks_like_text(&[0, 1, 2, 3]));
        assert_eq!(as_text(b"hello").as_deref(), Some("hello"));
        assert!(as_text(&[0, 0, 0]).is_none());
    }
}
