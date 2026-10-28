//! 7rust-zip — a native, 7-Zip-style archiver for Linux.
//!
//! Pure Rust: sevenz-rust2 for 7z, `zip` for zip, `tar` + codecs for tarballs,
//! egui for the UI. No Wine, no Qt, no `7z` subprocess, and no external
//! launcher: archives can be browsed and files previewed in-app.

mod engine;
mod icons;

use eframe::egui;
use engine::{human, Format};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Upper bound on what the built-in viewer will load, to stay responsive.
const VIEW_LIMIT: u64 = 8 * 1024 * 1024;
/// Upper bound on bytes rendered as text (egui chokes on huge strings).
const RENDER_LIMIT: usize = 400_000;

enum Msg {
    Entries(PathBuf, Vec<engine::Entry>),
    Preview(Result<Vec<u8>, String>, String),
    Done(String, bool),
    Verified(usize, Vec<String>),
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

struct SevenRustZip {
    icons: Arc<icons::Icons>,

    cwd: PathBuf,
    dirs: Vec<DirRow>,
    dir_sel: Option<usize>,

    archive: Option<PathBuf>,
    entries: Vec<engine::Entry>,
    /// Path inside the archive we have "entered", empty = root.
    inner: String,
    arc_sel: Option<usize>,

    filter: String,
    status: String,
    busy: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,

    sort_col: SortCol,
    ascending: bool,

    create_sources: Vec<PathBuf>,
    create_name: String,
    create_fmt: Format,
    show_create: bool,

    /// Properties / Info dialog state
    show_info: bool,
    info_title: String,
    info_lines: Vec<(String, String)>,

    /// Delete confirmation dialog state
    delete_target: Option<(PathBuf, bool)>,

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
    name: String,
    method: Option<String>,
    bytes: Option<Vec<u8>>,
    error: Option<String>,
    loading: bool,
}

impl SevenRustZip {
    fn new(cc: &eframe::CreationContext<'_>, start: Option<PathBuf>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Light);
        cc.egui_ctx.style_mut_of(egui::Theme::Light, |style| {
            style.spacing.item_spacing = egui::vec2(6.0, 3.0);
            style.spacing.button_padding = egui::vec2(8.0, 4.0);
            style.visuals = light_visuals();
        });

        let icons = icons::Icons::new(&cc.egui_ctx);
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
            icons,
            cwd,
            dirs: Vec::new(),
            dir_sel: None,
            archive: None,
            entries: Vec::new(),
            inner: String::new(),
            arc_sel: None,
            filter: String::new(),
            status: "Ready".into(),
            busy: false,
            tx,
            rx,
            sort_col: SortCol::Name,
            ascending: true,
            create_sources: Vec::new(),
            create_name: String::new(),
            create_fmt: Format::SevenZ,
            show_create: false,
            show_info: false,
            info_title: String::new(),
            info_lines: Vec::new(),
            delete_target: None,
            preview: None,
            focus: Pane::Files,
            typeahead: String::new(),
            typeahead_at: None,
            titlebar_press: None,
            titlebar_moving: false,
            resizing: false,
        };
        me.refresh_dir();
        if let Some(a) = archive {
            me.open_archive(a);
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
        self.cwd = p;
        self.refresh_dir();
        self.dir_sel = None;
        self.typeahead.clear();
    }

    fn go_up(&mut self) {
        if let Some(p) = self.cwd.parent() {
            let old = self.cwd.clone();
            self.enter_dir(p.to_path_buf());
            if let Some(name) = old.file_name() {
                let n = name.to_string_lossy();
                self.dir_sel = self.dirs.iter().position(|r| r.name == n);
            }
        }
    }

    // --------------------------------------------------------------- archive

    fn open_archive(&mut self, path: PathBuf) {
        if !looks_like_archive(&path) {
            self.start_preview_file(&path);
            return;
        }
        self.busy = true;
        self.status = format!("Reading {}…", path.display());
        self.archive = Some(path.clone());
        self.inner.clear();
        self.arc_sel = None;
        self.focus = Pane::Archive;
        let tx = self.tx.clone();
        std::thread::spawn(move || match engine::list(&path) {
            Ok(es) => {
                let _ = tx.send(Msg::Entries(path, es));
            }
            Err(e) => {
                let _ = tx.send(Msg::Done(format!("Not an archive: {e}"), false));
            }
        });
    }

    fn close_archive(&mut self) {
        self.archive = None;
        self.entries.clear();
        self.inner.clear();
        self.arc_sel = None;
        self.preview = None;
        self.focus = Pane::Files;
        self.status = "No archive open".into();
    }

    /// Rows for the current inner directory, filtered by the search box.
    fn visible_rows(&self) -> Vec<Row> {
        let f = self.filter.to_lowercase();
        let prefix = self.inner.clone();

        let mut rows: Vec<Row> = Vec::new();
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
            self.arc_sel = None;
            self.typeahead.clear();
        } else {
            let full = if self.inner.is_empty() {
                r.name.clone()
            } else {
                format!("{}{}", self.inner, r.name)
            };
            self.start_preview_entry(full);
        }
    }

    fn archive_go_up(&mut self) {
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
        self.arc_sel = rows.iter().position(|r| r.name == prev_name);
    }

    // --------------------------------------------------------------- preview

    fn start_preview_file(&mut self, path: &Path) {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if size > VIEW_LIMIT {
            self.preview = Some(Preview {
                name,
                method: None,
                bytes: None,
                error: Some(format!(
                    "File is {} — too large to preview (limit {})",
                    human(size),
                    human(VIEW_LIMIT)
                )),
                loading: false,
            });
            return;
        }
        self.preview = Some(Preview {
            name,
            method: None,
            bytes: None,
            error: None,
            loading: true,
        });
        match std::fs::read(path) {
            Ok(b) => {
                if let Some(p) = &mut self.preview {
                    p.bytes = Some(b);
                    p.loading = false;
                }
            }
            Err(e) => {
                if let Some(p) = &mut self.preview {
                    p.error = Some(e.to_string());
                    p.loading = false;
                }
            }
        }
    }

    fn start_preview_entry(&mut self, entry_path: String) {
        let Some(arc) = self.archive.clone() else { return };
        let size = self
            .entries
            .iter()
            .find(|e| e.path == entry_path)
            .map(|e| e.size)
            .unwrap_or(0);
        let name = entry_path.rsplit('/').next().unwrap_or(&entry_path).to_string();
        let method = self
            .entries
            .iter()
            .find(|e| e.path == entry_path)
            .and_then(|e| e.method.clone());

        if size > VIEW_LIMIT {
            self.preview = Some(Preview {
                name,
                method: method.clone(),
                bytes: None,
                error: Some(format!(
                    "Entry is {} — too large to preview (limit {})",
                    human(size),
                    human(VIEW_LIMIT)
                )),
                loading: false,
            });
            return;
        }

        self.preview = Some(Preview {
            name,
            method,
            bytes: None,
            error: None,
            loading: true,
        });
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = engine::read_entry(&arc, &entry_path).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Preview(r, entry_path));
        });
    }

    // ------------------------------------------------------------------ jobs

    fn extract_to(&mut self, dest: PathBuf) {
        let Some(archive) = self.archive.clone() else {
            return;
        };
        self.busy = true;
        self.status = format!("Extracting to {}…", dest.display());
        let tx = self.tx.clone();
        std::thread::spawn(move || match engine::extract(&archive, &dest) {
            Ok(n) => {
                let _ = tx.send(Msg::Done(format!("Extracted {n} entries"), true));
            }
            Err(e) => {
                let _ = tx.send(Msg::Done(format!("Extract failed: {e}"), false));
            }
        });
    }

    /// Archive-relative path of the row selected in the archive pane.
    fn selected_entry_path(&self) -> Option<String> {
        let rows = self.visible_rows();
        let i = self.arc_sel?;
        let item = rows.get(i)?;
        Some(if self.inner.is_empty() {
            item.name.clone()
        } else {
            format!("{}{}", self.inner, item.name)
        })
    }

    /// Extract only the selected entry (or the selected directory subtree).
    fn extract_selected(&mut self) {
        let Some(archive) = self.archive.clone() else {
            return;
        };
        let Some(entry) = self.selected_entry_path() else {
            self.status = "Select an item in the archive to extract".into();
            return;
        };
        let Some(dest) = rfd::FileDialog::new().set_directory(&self.cwd).pick_folder() else {
            return;
        };
        self.busy = true;
        self.status = format!("Extracting {entry}…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            match engine::extract_entry(&archive, &entry, &dest) {
                Ok(n) => {
                    let _ = tx.send(Msg::Done(format!("Extracted {n} item(s) from {entry}"), true));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Done(format!("Extract failed: {e}"), false));
                }
            }
        });
    }

    fn run_verify(&mut self) {
        let Some(archive) = self.archive.clone() else {
            return;
        };
        self.busy = true;
        self.status = "Testing archive integrity…".into();
        let tx = self.tx.clone();
        std::thread::spawn(move || match engine::verify(&archive) {
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
        let fmt = self.create_fmt;
        let sources = std::mem::take(&mut self.create_sources);
        self.busy = true;
        self.status = format!("Creating {}…", target.display());
        self.show_create = false;
        let tx = self.tx.clone();
        std::thread::spawn(move || match engine::create(fmt, &sources, &target) {
            Ok(()) => {
                let _ = tx.send(Msg::Done(format!("Created {}", target.display()), true));
            }
            Err(e) => {
                let _ = tx.send(Msg::Done(format!("Create failed: {e}"), false));
            }
        });
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Entries(path, es) => {
                    if self.archive.as_deref() == Some(path.as_path()) {
                        self.status = format!(
                            "{} — {} entries ({})",
                            path.file_name().and_then(|s| s.to_str()).unwrap_or("archive"),
                            es.len(),
                            human(engine::container_size(&path))
                        );
                        self.entries = es;
                        self.inner.clear();
                        self.arc_sel = None;
                    }
                    self.busy = false;
                }
                Msg::Preview(r, entry_path) => {
                    if let Some(p) = &mut self.preview {
                        let still = entry_path
                            .rsplit('/')
                            .next()
                            .map(|n| n == p.name)
                            .unwrap_or(false);
                        if still {
                            match r {
                                Ok(b) => p.bytes = Some(b),
                                Err(e) => p.error = Some(e),
                            }
                            p.loading = false;
                        }
                    }
                }
                Msg::Verified(n, errs) => {
                    self.busy = false;
                    if errs.is_empty() {
                        self.status = format!("Everything is Ok — {n} files tested");
                    } else {
                        self.status =
                            format!("{n} tested, {} problem(s): {}", errs.len(), errs[0]);
                    }
                }
                Msg::Done(s, ok) => {
                    self.status = s;
                    self.busy = false;
                    if ok {
                        self.refresh_dir();
                        if let Some(a) = self.archive.clone() {
                            self.open_archive(a);
                        }
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------- keyboard

    fn handle_keys(&mut self, ui: &egui::Ui, rows: &[Row]) {
        let (back, enter, up, down, home, end) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Backspace),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::End),
            )
        });
        let ctrl_pgup = ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::PageUp));
        let ctrl_pgdn = ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::PageDown));

        match self.focus {
            Pane::Files => {
                let n = self.dirs.len();
                if n > 0 {
                    if down {
                        self.dir_sel = Some(match self.dir_sel {
                            Some(i) if i + 1 < n => i + 1,
                            Some(_) => 0,
                            None => 0,
                        });
                    }
                    if up {
                        self.dir_sel = Some(match self.dir_sel {
                            Some(i) if i > 0 => i - 1,
                            Some(_) => n - 1,
                            None => n - 1,
                        });
                    }
                    if home {
                        self.dir_sel = Some(0);
                    }
                    if end {
                        self.dir_sel = Some(n - 1);
                    }
                }
                if ctrl_pgup || back {
                    self.go_up();
                }
                if enter || ctrl_pgdn {
                    if let Some(i) = self.dir_sel {
                        if let Some(r) = self.dirs.get(i) {
                            let p = self.cwd.join(&r.name);
                            if r.is_dir {
                                self.enter_dir(p);
                            } else {
                                self.open_archive(p);
                            }
                        }
                    }
                }
            }
            Pane::Archive => {
                let n = rows.len();
                if n > 0 {
                    if down {
                        self.arc_sel = Some(match self.arc_sel {
                            Some(i) if i + 1 < n => i + 1,
                            Some(_) => 0,
                            None => 0,
                        });
                    }
                    if up {
                        self.arc_sel = Some(match self.arc_sel {
                            Some(i) if i > 0 => i - 1,
                            Some(_) => n - 1,
                            None => n - 1,
                        });
                    }
                    if home {
                        self.arc_sel = Some(0);
                    }
                    if end {
                        self.arc_sel = Some(n - 1);
                    }
                }
                if ctrl_pgup || back {
                    self.archive_go_up();
                }
                if enter || ctrl_pgdn {
                    if let Some(i) = self.arc_sel {
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
                Pane::Files => self.dir_sel = Some(i),
                Pane::Archive => self.arc_sel = Some(i),
            }
        }
    }
}

// ------------------------------------------------------------------ helpers

fn looks_like_archive(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    const EXTS: &[&str] = &[
        ".7z", ".zip", ".tar", ".gz", ".tgz", ".xz", ".txz", ".bz2", ".tbz2", ".zst", ".tzst",
        ".rar",
    ];
    if EXTS.iter().any(|e| name.ends_with(e)) {
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

// -- list geometry -----------------------------------------------------------
// Rows are painted by hand rather than placed in an egui::Grid so that the
// selection highlight covers the whole row, and so every click across the row
// hits the same item. A Grid would only make the name cell clickable.

/// Width reserved on the right of the titlebar for the window buttons.
const BUTTON_BAR_W: f32 = 108.0;

const ROW_H: f32 = 22.0;
const ROW_ICON: f32 = 17.0;
/// The list text is set one point above the theme default so rows read comfortably.
const ROW_TEXT_BOOST: f32 = 1.0;
const ROW_PAD: f32 = 4.0;
const COL_NUM: f32 = 84.0;
const COL_DATE: f32 = 116.0;

const ROW_SELECTED: egui::Color32 = egui::Color32::from_rgb(204, 226, 247);
const ROW_HOVER: egui::Color32 = egui::Color32::from_rgb(232, 240, 250);
const ROW_FOCUS: egui::Color32 = egui::Color32::from_rgb(120, 170, 220);
const ROW_TEXT: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
const ROW_DIM: egui::Color32 = egui::Color32::from_rgb(110, 110, 110);

/// Horizontal layout for one list row.
struct RowCols {
    icon: egui::Rect,
    name_left: f32,
    name_right: f32,
    size_right: f32,
    packed_right: Option<f32>,
    date_right: f32,
}

impl RowCols {
    fn new(rect: egui::Rect, show_packed: bool) -> Self {
        let icon = egui::Rect::from_min_size(
            egui::pos2(rect.left() + ROW_PAD, rect.center().y - ROW_ICON / 2.0),
            egui::vec2(ROW_ICON, ROW_ICON),
        );
        let date_right = rect.right() - ROW_PAD;
        let date_left = date_right - COL_DATE;

        let (packed_right, size_right) = if show_packed {
            let packed_right = date_left - 4.0;
            (Some(packed_right), packed_right - COL_NUM)
        } else {
            (None, date_left)
        };

        Self {
            icon,
            name_left: icon.right() + 6.0,
            name_right: size_right - COL_NUM - 6.0,
            size_right: size_right - 4.0,
            packed_right,
            date_right,
        }
    }
}

/// Truncate `text` with an ellipsis so it fits `max_w`.
fn elide(ui: &egui::Ui, text: &str, font: egui::FontId, color: egui::Color32, max_w: f32) -> std::sync::Arc<egui::Galley> {
    let measure = |s: &str| {
        ui.painter().layout_no_wrap(s.to_owned(), font.clone(), color)
    };
    let full = measure(text);
    if full.size().x <= max_w || max_w <= 0.0 {
        return full;
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let trial: String = chars[..mid].iter().collect::<String>() + "…";
        if measure(&trial).size().x <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let cut: String = chars[..lo].iter().collect::<String>() + "…";
    measure(&cut)
}

/// Paint one list row and return its full-width click response.
#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    icon: egui::TextureId,
    name: &str,
    size: &str,
    packed: &str,
    date: &str,
    selected: bool,
    focused: bool,
    show_packed: bool,
) -> egui::Response {
    let width = ui.available_width().max(240.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::click());
    let cols = RowCols::new(rect, show_packed);

    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 2.0, ROW_SELECTED);
        if focused {
            painter.rect_stroke(
                rect.shrink(0.5),
                2.0,
                egui::Stroke::new(1.0, ROW_FOCUS),
                egui::StrokeKind::Inside,
            );
        }
    } else if resp.hovered() {
        painter.rect_filled(rect, 2.0, ROW_HOVER);
    }

    painter.image(
        icon,
        cols.icon,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    let name_font =
        egui::FontId::proportional(egui::TextStyle::Body.resolve(ui.style()).size + ROW_TEXT_BOOST);
    let num_font =
        egui::FontId::monospace(egui::TextStyle::Monospace.resolve(ui.style()).size + ROW_TEXT_BOOST);
    let cy = rect.center().y;

    let galley = elide(ui, name, name_font, ROW_TEXT, cols.name_right - cols.name_left);
    let painter = ui.painter();
    painter.galley(
        egui::pos2(cols.name_left, cy - galley.size().y / 2.0),
        galley,
        ROW_TEXT,
    );

    if !size.is_empty() {
        painter.text(
            egui::pos2(cols.size_right, cy),
            egui::Align2::RIGHT_CENTER,
            size,
            num_font.clone(),
            ROW_DIM,
        );
    }
    if let Some(pr) = cols.packed_right {
        if !packed.is_empty() {
            painter.text(
                egui::pos2(pr, cy),
                egui::Align2::RIGHT_CENTER,
                packed,
                num_font,
                ROW_DIM,
            );
        }
    }
    if !date.is_empty() {
        painter.text(
            egui::pos2(cols.date_right, cy),
            egui::Align2::RIGHT_CENTER,
            date,
            egui::FontId::proportional(11.5 + ROW_TEXT_BOOST),
            ROW_DIM,
        );
    }

    resp
}

/// Column header whose segments are clickable to change the sort.
fn header(
    ui: &mut egui::Ui,
    labels: [(&str, SortCol); 4],
    current: SortCol,
    ascending: bool,
    show_packed: bool,
) -> Option<SortCol> {
    let width = ui.available_width().max(240.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::click());
    let cols = RowCols::new(rect, show_packed);

    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(238, 238, 238));
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, egui::Color32::from_rgb(212, 212, 212)),
    );

    let font =
        egui::FontId::proportional(egui::TextStyle::Body.resolve(ui.style()).size + ROW_TEXT_BOOST);
    let cy = rect.center().y;
    let arrow = |c: SortCol| {
        if current == c {
            if ascending {
                " ▲"
            } else {
                " ▼"
            }
        } else {
            ""
        }
    };

    // Name occupies everything from the icon to the numeric columns.
    painter.text(
        egui::pos2(cols.name_left, cy),
        egui::Align2::LEFT_CENTER,
        format!("{}{}", labels[0].0, arrow(labels[0].1)),
        font.clone(),
        ROW_TEXT,
    );

    let mut regions: Vec<(egui::Rect, SortCol)> = Vec::with_capacity(4);
    regions.push((
        egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top()), egui::pos2(cols.size_right - COL_NUM, rect.bottom())),
        labels[0].1,
    ));

    painter.text(
        egui::pos2(cols.size_right, cy),
        egui::Align2::RIGHT_CENTER,
        format!("{}{}", labels[1].0, arrow(labels[1].1)),
        font.clone(),
        ROW_TEXT,
    );
    regions.push((
        egui::Rect::from_min_max(
            egui::pos2(cols.size_right - COL_NUM, rect.top()),
            egui::pos2(cols.size_right + 4.0, rect.bottom()),
        ),
        labels[1].1,
    ));

    let mut next = 2usize;
    if let Some(pr) = cols.packed_right {
        painter.text(
            egui::pos2(pr, cy),
            egui::Align2::RIGHT_CENTER,
            format!("{}{}", labels[2].0, arrow(labels[2].1)),
            font.clone(),
            ROW_TEXT,
        );
        regions.push((
            egui::Rect::from_min_max(
                egui::pos2(pr - COL_NUM, rect.top()),
                egui::pos2(pr + 4.0, rect.bottom()),
            ),
            labels[2].1,
        ));
        next = 3;
    }

    painter.text(
        egui::pos2(cols.date_right, cy),
        egui::Align2::RIGHT_CENTER,
        format!("{}{}", labels[next].0, arrow(labels[next].1)),
        font,
        ROW_TEXT,
    );
    regions.push((
        egui::Rect::from_min_max(
            egui::pos2(cols.date_right - COL_DATE, rect.top()),
            egui::pos2(rect.right(), rect.bottom()),
        ),
        labels[next].1,
    ));

    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            return regions
                .into_iter()
                .find(|(r, _)| r.contains(pos))
                .map(|(_, c)| c);
        }
    }
    None
}

fn cmp_names(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

fn light_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    v.panel_fill = egui::Color32::from_rgb(240, 240, 240);
    v.window_fill = egui::Color32::from_rgb(248, 248, 248);
    v.extreme_bg_color = egui::Color32::WHITE;
    v.faint_bg_color = egui::Color32::from_rgb(250, 250, 250);
    v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(240, 240, 240);
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(232, 232, 232);
    v.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(232, 232, 232);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(204, 226, 247);
    v.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(204, 226, 247);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(0, 120, 215);
    v.selection.bg_fill = egui::Color32::from_rgb(204, 226, 247);
    v.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 120, 215));
    v.override_text_color = Some(egui::Color32::from_rgb(24, 24, 24));
    v
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

fn as_text(bytes: &[u8]) -> Option<String> {
    let n = bytes.len().min(RENDER_LIMIT);
    let slice = &bytes[..n];
    let bad = slice
        .iter()
        .take(4096)
        .filter(|b| **b == 0 || (**b < 0x09))
        .count();
    if bad > 0 {
        return None;
    }
    let s = String::from_utf8_lossy(slice).to_string();
    Some(if bytes.len() > n {
        format!("{s}\n\n… truncated at {} (of {})", human(n as u64), human(bytes.len() as u64))
    } else {
        s
    })
}

fn hex_dump(bytes: &[u8]) -> String {
    let n = bytes.len().min(RENDER_LIMIT / 4);
    let mut out = String::with_capacity(n * 4);
    for (i, chunk) in bytes[..n].chunks(16).enumerate() {
        out.push_str(&format!("{:08X}  ", i * 16));
        for b in chunk {
            out.push_str(&format!("{b:02X} "));
        }
        for _ in chunk.len()..16 {
            out.push_str("   ");
        }
        out.push(' ');
        for b in chunk {
            out.push(if b.is_ascii_graphic() || *b == b' ' {
                *b as char
            } else {
                '.'
            });
        }
        out.push('\n');
    }
    if bytes.len() > n {
        out.push_str(&format!("… truncated at {}\n", human(n as u64)));
    }
    out
}

// ----------------------------------------------------------------- UI parts

impl SevenRustZip {
    fn ui_toolbar(&mut self, ui: &mut egui::Ui) {
        let icons = Arc::clone(&self.icons);

        ui.horizontal(|ui| {
            // 7-Zip Add Button
            if ui
                .add(egui::Button::image_and_text(
                    egui::Image::new(&icons.add).fit_to_exact_size(egui::vec2(22.0, 22.0)),
                    "Add",
                ))
                .clicked()
            {
                self.show_create = true;
                self.create_name = "archive.7z".into();
                self.create_sources = self
                    .dir_sel
                    .and_then(|i| self.dirs.get(i))
                    .map(|r| vec![self.cwd.join(&r.name)])
                    .unwrap_or_default();
            }

            // 7-Zip Extract Button
            if ui
                .add_enabled(
                    self.archive.is_some(),
                    egui::Button::image_and_text(
                        egui::Image::new(&icons.extract).fit_to_exact_size(egui::vec2(22.0, 22.0)),
                        "Extract",
                    ),
                )
                .clicked()
            {
                if let Some(d) = rfd::FileDialog::new()
                    .set_directory(&self.cwd)
                    .pick_folder()
                {
                    self.extract_to(d);
                }
            }

            // Extract only the selected entry inside the archive.
            let sel_entry = self.selected_entry_path();
            if ui
                .add_enabled(
                    sel_entry.is_some(),
                    egui::Button::image_and_text(
                        egui::Image::new(&icons.extract).fit_to_exact_size(egui::vec2(22.0, 22.0)),
                        "Extract Sel.",
                    ),
                )
                .on_hover_text("Extract only the selected item")
                .clicked()
            {
                self.extract_selected();
            }

            // 7-Zip Test Button
            if ui
                .add_enabled(
                    self.archive.is_some(),
                    egui::Button::image_and_text(
                        egui::Image::new(&icons.test).fit_to_exact_size(egui::vec2(22.0, 22.0)),
                        "Test",
                    ),
                )
                .clicked()
            {
                self.run_verify();
            }

            // 7-Zip Copy Button
            if ui
                .add_enabled(
                    self.dir_sel.is_some(),
                    egui::Button::image_and_text(
                        egui::Image::new(&icons.copy).fit_to_exact_size(egui::vec2(22.0, 22.0)),
                        "Copy",
                    ),
                )
                .clicked()
            {
                if let Some(idx) = self.dir_sel {
                    if let Some(r) = self.dirs.get(idx) {
                        let src = self.cwd.join(&r.name);
                        if let Some(dest_dir) = rfd::FileDialog::new().set_directory(&self.cwd).pick_folder() {
                            let dest = dest_dir.join(&r.name);
                            if let Err(e) = std::fs::copy(&src, &dest) {
                                self.status = format!("Copy failed: {e}");
                            } else {
                                self.status = format!("Copied to {}", dest.display());
                                self.refresh_dir();
                            }
                        }
                    }
                }
            }

            // 7-Zip Move Button
            if ui
                .add_enabled(
                    self.dir_sel.is_some(),
                    egui::Button::image_and_text(
                        egui::Image::new(&icons.move_item).fit_to_exact_size(egui::vec2(22.0, 22.0)),
                        "Move",
                    ),
                )
                .clicked()
            {
                if let Some(idx) = self.dir_sel {
                    if let Some(r) = self.dirs.get(idx) {
                        let src = self.cwd.join(&r.name);
                        if let Some(dest_dir) = rfd::FileDialog::new().set_directory(&self.cwd).pick_folder() {
                            let dest = dest_dir.join(&r.name);
                            if let Err(e) = std::fs::rename(&src, &dest) {
                                self.status = format!("Move failed: {e}");
                            } else {
                                self.status = format!("Moved to {}", dest.display());
                                self.refresh_dir();
                            }
                        }
                    }
                }
            }

            // 7-Zip Delete Button
            if ui
                .add_enabled(
                    self.dir_sel.is_some(),
                    egui::Button::image_and_text(
                        egui::Image::new(&icons.delete).fit_to_exact_size(egui::vec2(22.0, 22.0)),
                        "Delete",
                    ),
                )
                .clicked()
            {
                if let Some(idx) = self.dir_sel {
                    if let Some(r) = self.dirs.get(idx) {
                        self.delete_target = Some((self.cwd.join(&r.name), r.is_dir));
                    }
                }
            }

            // 7-Zip Info / Properties Button
            if ui
                .add_enabled(
                    self.dir_sel.is_some() || self.archive.is_some(),
                    egui::Button::image_and_text(
                        egui::Image::new(&icons.info).fit_to_exact_size(egui::vec2(22.0, 22.0)),
                        "Info",
                    ),
                )
                .clicked()
            {
                self.show_properties();
            }

            ui.separator();

            // Up Button
            if ui
                .add(egui::Button::image_and_text(
                    egui::Image::new(&icons.up).fit_to_exact_size(egui::vec2(18.0, 18.0)),
                    "Up",
                ))
                .clicked()
            {
                match self.focus {
                    Pane::Files => self.go_up(),
                    Pane::Archive => self.archive_go_up(),
                }
            }

            // Refresh Button
            if ui
                .add(egui::Button::image_and_text(
                    egui::Image::new(&icons.refresh).fit_to_exact_size(egui::vec2(18.0, 18.0)),
                    "Refresh",
                ))
                .clicked()
            {
                self.refresh_dir();
                if let Some(a) = self.archive.clone() {
                    self.open_archive(a);
                }
            }

            ui.separator();

            // Filter search box
            ui.label("Filter:");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .desired_width(140.0)
                    .hint_text("Search name"),
            );
            if resp.changed() {
                self.arc_sel = None;
                self.dir_sel = None;
            }
            if !self.filter.is_empty() {
                let clear_btn = egui::Button::image(
                    egui::Image::new(&icons.window_close).fit_to_exact_size(egui::vec2(13.0, 13.0)),
                )
                .frame(false);
                if ui.add(clear_btn).on_hover_text("Clear filter").clicked() {
                    self.filter.clear();
                }
            }
        });
    }

    fn show_properties(&mut self) {
        self.info_lines.clear();
        if self.focus == Pane::Archive || self.dir_sel.is_none() {
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

        if let Some(idx) = self.dir_sel {
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

    fn ui_files(&mut self, ui: &mut egui::Ui) {
        let icons = Arc::clone(&self.icons);

        ui.horizontal(|ui| {
            ui.add(egui::Image::new(&icons.folder).fit_to_exact_size(egui::vec2(16.0, 16.0)));
            ui.label(egui::RichText::new("Folder:").strong());
            ui.label(
                egui::RichText::new(self.cwd.display().to_string())
                    .small()
                    .color(egui::Color32::from_gray(80)),
            );
        });

        let focused_pane = self.focus == Pane::Files;
        if let Some(c) = header(
            ui,
            [
                ("Name", SortCol::Name),
                ("Size", SortCol::Size),
                ("Packed", SortCol::Packed),
                ("Modified", SortCol::Modified),
            ],
            self.sort_col,
            self.ascending,
            false,
        ) {
            self.toggle_sort(c);
        }

        egui::ScrollArea::both()
            .id_salt("files_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut clicked = None;
                let mut dbl = None;
                let mut go_parent = false;

                // 7-Zip pins a ".." row at the top of every listing.
                if self.cwd.parent().is_some() {
                    let r = row(
                        ui,
                        icons.folder_up.id(),
                        "..",
                        "",
                        "",
                        "",
                        false,
                        false,
                        false,
                    );
                    if r.double_clicked() {
                        go_parent = true;
                    }
                }

                for (i, entry) in self.dirs.iter().enumerate() {
                    let sel = self.dir_sel == Some(i);
                    let icon = if entry.is_dir {
                        icons.folder.id()
                    } else if entry.is_archive {
                        icons.archive.id()
                    } else {
                        icons.file.id()
                    };
                    let size_text = if entry.is_dir {
                        String::new()
                    } else {
                        human(entry.size)
                    };
                    let r = row(
                        ui,
                        icon,
                        &entry.name,
                        &size_text,
                        "",
                        &entry.modified,
                        sel,
                        focused_pane,
                        false,
                    );
                    if r.clicked() {
                        clicked = Some(i);
                        self.focus = Pane::Files;
                    }
                    if r.double_clicked() {
                        dbl = Some(i);
                    }
                }

                if let Some(i) = clicked {
                    self.dir_sel = Some(i);
                }
                if go_parent {
                    self.go_up();
                }
                if let Some(i) = dbl {
                    if let Some(r) = self.dirs.get(i) {
                        let p = self.cwd.join(&r.name);
                        if r.is_dir {
                            self.enter_dir(p);
                        } else {
                            self.open_archive(p);
                        }
                    }
                }
            });
    }

    fn ui_archive(&mut self, ui: &mut egui::Ui) {
        let icons = Arc::clone(&self.icons);

        let Some(arc) = self.archive.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(
                        "Double-click an archive on the left to open it.\n\
                         Everything opens natively in-app.",
                    )
                    .size(14.0)
                    .color(egui::Color32::from_gray(120)),
                );
            });
            return;
        };

        let fmt = engine::detect(&arc);
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(&icons.archive).fit_to_exact_size(egui::vec2(16.0, 16.0)));
            ui.label(egui::RichText::new("Archive:").strong());
            ui.label(
                egui::RichText::new(
                    arc.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("archive"),
                )
                .color(egui::Color32::from_rgb(0, 90, 160)),
            );
            ui.label(
                egui::RichText::new(format!("[{}]", fmt.label()))
                    .small()
                    .color(egui::Color32::from_gray(110)),
            );
            if !self.inner.is_empty() {
                ui.label(egui::RichText::new("›").color(egui::Color32::from_gray(120)));
                ui.label(
                    egui::RichText::new(self.inner.trim_end_matches('/'))
                        .monospace()
                        .color(egui::Color32::from_gray(80)),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close_btn = egui::Button::image(
                    egui::Image::new(&icons.window_close).fit_to_exact_size(egui::vec2(14.0, 14.0)),
                )
                .frame(false);
                if ui
                    .add(close_btn)
                    .on_hover_text("Close archive")
                    .clicked()
                {
                    self.close_archive();
                }
            });
        });
        ui.separator();

        let rows = self.visible_rows();
        let focused_pane = self.focus == Pane::Archive;

        if let Some(c) = header(
            ui,
            [
                ("Name", SortCol::Name),
                ("Size", SortCol::Size),
                ("Packed", SortCol::Packed),
                ("Modified", SortCol::Modified),
            ],
            self.sort_col,
            self.ascending,
            true,
        ) {
            self.toggle_sort(c);
        }

        egui::ScrollArea::both()
            .id_salt("archive_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(
                        egui::RichText::new("(empty)")
                            .italics()
                            .color(egui::Color32::from_gray(140)),
                    );
                    return;
                }
                let mut clicked = None;
                let mut dbl = None;
                let mut go_parent = false;

                if !self.inner.is_empty() {
                    let r = row(
                        ui,
                        icons.folder_up.id(),
                        "..",
                        "",
                        "",
                        "",
                        false,
                        false,
                        true,
                    );
                    if r.double_clicked() {
                        go_parent = true;
                    }
                }

                for (i, item) in rows.iter().enumerate() {
                    let sel = self.arc_sel == Some(i);
                    let icon = if item.is_dir {
                        icons.folder.id()
                    } else {
                        icons.file.id()
                    };
                    let size_text = if item.is_dir {
                        String::new()
                    } else {
                        human(item.size)
                    };
                    let pack_text = if item.packed > 0 {
                        human(item.packed)
                    } else {
                        String::new()
                    };
                    let r = row(
                        ui,
                        icon,
                        &item.name,
                        &size_text,
                        &pack_text,
                        &item.modified,
                        sel,
                        focused_pane,
                        true,
                    );
                    if r.clicked() {
                        clicked = Some(i);
                        self.focus = Pane::Archive;
                    }
                    if r.double_clicked() {
                        dbl = Some(i);
                    }
                }

                if let Some(i) = clicked {
                    self.arc_sel = Some(i);
                }
                if go_parent {
                    self.archive_go_up();
                }
                if let Some(i) = dbl {
                    self.enter_archive_item(&rows, i);
                }
            });
    }

    fn ui_preview(&mut self, ui: &mut egui::Ui) {
        let Some(p) = &self.preview else { return };
        let name = p.name.clone();
        let loading = p.loading;
        let error = p.error.clone();
        let bytes = p.bytes.clone();
        let method = p.method.clone();

        let mut close = false;
        egui::Panel::right("preview")
            .resizable(true)
            .default_size(460.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Viewer:").strong());
                    ui.label(
                        egui::RichText::new(&name)
                            .monospace()
                            .color(egui::Color32::from_rgb(0, 90, 160)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = egui::Button::image(
                            egui::Image::new(&self.icons.window_close)
                                .fit_to_exact_size(egui::vec2(14.0, 14.0)),
                        )
                        .frame(false);
                        if ui.add(btn).on_hover_text("Close viewer").clicked() {
                            close = true;
                        }
                    });
                });
                if let Some(m) = &method {
                    ui.label(
                        egui::RichText::new(format!("codec: {m}"))
                            .small()
                            .color(egui::Color32::from_gray(120)),
                    );
                }
                ui.separator();

                if loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Decoding…");
                    });
                    return;
                }
                if let Some(e) = error {
                    ui.label(
                        egui::RichText::new(e).color(egui::Color32::from_rgb(170, 40, 40)),
                    );
                    return;
                }
                let Some(bytes) = bytes else { return };
                ui.label(
                    egui::RichText::new(human(bytes.len() as u64))
                        .small()
                        .color(egui::Color32::from_gray(120)),
                );
                match as_text(&bytes) {
                    Some(text) => {
                        egui::ScrollArea::both()
                            .id_salt("preview_text")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(text)
                                            .monospace()
                                            .size(12.0),
                                    )
                                    .selectable(true),
                                );
                            });
                    }
                    None => {
                        ui.label(
                            egui::RichText::new("Binary — showing hex")
                                .small()
                                .italics()
                                .color(egui::Color32::from_gray(120)),
                        );
                        egui::ScrollArea::both()
                            .id_salt("preview_hex")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(hex_dump(&bytes))
                                            .monospace()
                                            .size(12.0),
                                    )
                                    .selectable(true),
                                );
                            });
                    }
                }
            });

        if close {
            self.preview = None;
        }
    }

    fn ui_status(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.busy {
                ui.spinner();
            }
            ui.label(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let pane = match self.focus {
                    Pane::Files => "files",
                    Pane::Archive => "archive",
                };
                ui.label(
                    egui::RichText::new(format!("active: {pane}"))
                        .small()
                        .color(egui::Color32::from_gray(120)),
                );
                // Mirror 7-Zip, which lists the selected item in the status bar.
                let selected = match self.focus {
                    Pane::Files => self
                        .dir_sel
                        .and_then(|i| self.dirs.get(i))
                        .map(|r| (r.name.clone(), if r.is_dir { String::new() } else { human(r.size) })),
                    Pane::Archive => self.arc_sel.and_then(|i| {
                        self.visible_rows()
                            .get(i)
                            .map(|r| (r.name.clone(), if r.is_dir { String::new() } else { human(r.size) }))
                    }),
                };
                if let Some((name, size)) = selected {
                    ui.separator();
                    if size.is_empty() {
                        ui.label(egui::RichText::new(name).small());
                    } else {
                        ui.label(
                            egui::RichText::new(format!("{name}  ({size})")).small(),
                        );
                    }
                }
            });
        });
    }

    fn ui_dialogs(&mut self, ctx: &egui::Context) {
        // Create archive dialog
        if self.show_create {
            let mut open = self.show_create;
            let icons = Arc::clone(&self.icons);
            egui::Window::new("Add to Archive")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Image::new(&icons.add).fit_to_exact_size(egui::vec2(22.0, 22.0)));
                        ui.label(egui::RichText::new("Create New Archive").strong().size(14.0));
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Archive name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.create_name).desired_width(280.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Archive format:");
                        for f in [
                            Format::SevenZ,
                            Format::Zip,
                            Format::Tar,
                            Format::TarGz,
                            Format::TarXz,
                            Format::TarBz2,
                            Format::TarZst,
                        ] {
                            ui.selectable_value(&mut self.create_fmt, f, f.label());
                        }
                    });
                    ui.separator();
                    if self.create_sources.is_empty() {
                        ui.label(
                            egui::RichText::new("No files selected")
                                .italics()
                                .color(egui::Color32::from_gray(130)),
                        );
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("create_sources")
                        .max_height(120.0)
                        .show(ui, |ui| {
                            for s in &self.create_sources {
                                ui.label(egui::RichText::new(s.display().to_string()).monospace().small());
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui.button("Add files…").clicked() {
                            if let Some(fs) = rfd::FileDialog::new()
                                .set_directory(&self.cwd)
                                .pick_files()
                            {
                                self.create_sources.extend(fs);
                            }
                        }
                        if ui.button("Add folder…").clicked() {
                            if let Some(d) = rfd::FileDialog::new()
                                .set_directory(&self.cwd)
                                .pick_folder()
                            {
                                self.create_sources.push(d);
                            }
                        }
                        if ui.button("Clear").clicked() {
                            self.create_sources.clear();
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.create_sources.is_empty(),
                                egui::Button::new("OK"),
                            )
                            .clicked()
                        {
                            self.start_create();
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_create = false;
                        }
                    });
                });
            self.show_create = open;
        }

        // Properties / Info dialog
        if self.show_info {
            let mut open = self.show_info;
            let mut close_clicked = false;
            let icons = Arc::clone(&self.icons);
            egui::Window::new(format!("Properties: {}", self.info_title))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_size([380.0, 240.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Image::new(&icons.info).fit_to_exact_size(egui::vec2(22.0, 22.0)));
                        ui.label(egui::RichText::new(&self.info_title).strong().size(14.0));
                    });
                    ui.separator();
                    egui::Grid::new("props_grid")
                        .num_columns(2)
                        .spacing([18.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for (k, v) in &self.info_lines {
                                ui.label(egui::RichText::new(k).strong());
                                ui.label(egui::RichText::new(v).monospace());
                                ui.end_row();
                            }
                        });
                    ui.separator();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close_clicked = true;
                        }
                    });
                });
            self.show_info = open && !close_clicked;
        }

        // Delete confirmation dialog
        if let Some((target, is_dir)) = self.delete_target.clone() {
            let mut open = true;
            let file_name = target.file_name().and_then(|s| s.to_str()).unwrap_or("item");
            let icons = Arc::clone(&self.icons);
            egui::Window::new("Confirm Delete")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let kind = if is_dir { "folder and all its contents" } else { "file" };
                    ui.horizontal(|ui| {
                        ui.add(egui::Image::new(&icons.delete).fit_to_exact_size(egui::vec2(24.0, 24.0)));
                        ui.label(egui::RichText::new("Confirm Deletion").strong().size(14.0));
                    });
                    ui.separator();
                    ui.label(format!("Are you sure you want to delete this {kind}?"));
                    ui.label(egui::RichText::new(file_name).strong());
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            let res = if is_dir {
                                std::fs::remove_dir_all(&target)
                            } else {
                                std::fs::remove_file(&target)
                            };
                            match res {
                                Ok(()) => {
                                    self.status = format!("Deleted {}", target.display());
                                    self.refresh_dir();
                                }
                                Err(e) => self.status = format!("Delete failed: {e}"),
                            }
                            self.delete_target = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.delete_target = None;
                        }
                    });
                });
            if !open {
                self.delete_target = None;
            }
        }
    }

    fn ui_titlebar(&mut self, ui: &mut egui::Ui) {
        let is_maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        let icons = Arc::clone(&self.icons);

        // Note: the titlebar deliberately registers no click/drag-sensing widget.
        // An interactive window move hands the pointer to the compositor
        // (xdg_toplevel.move), which swallows the button release; egui would then
        // believe the button is still held and ignore every later click until an
        // Escape. So we detect press/travel/double-click ourselves and only use
        // raw pointer state.
        let bar = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

            // Left: App icon and title
            ui.add(egui::Image::new(&icons.app_mark).fit_to_exact_size(egui::vec2(22.0, 22.0)));
            ui.label(
                egui::RichText::new("7rust-zip")
                    .strong()
                    .size(14.0)
                    .color(egui::Color32::from_rgb(30, 41, 59)),
            );

            if let Some(arc) = &self.archive {
                let fname = arc.file_name().and_then(|s| s.to_str()).unwrap_or("archive");
                ui.label(
                    egui::RichText::new(format!("— {fname}"))
                        .size(13.0)
                        .color(egui::Color32::from_rgb(2, 132, 199)),
                );
            }

            // Spacer pushing the window buttons to the right.
            let avail = ui.available_size_before_wrap();
            ui.add_space((avail.x - BUTTON_BAR_W).max(12.0));

            // Right: Window controls (reduit, maximise, close) — enlarged to 20x20
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

                let btn_size = egui::vec2(20.0, 20.0);

                // Close button (red)
                let close_btn = egui::Button::image(
                    egui::Image::new(&icons.window_close).fit_to_exact_size(btn_size),
                )
                .frame(false);
                if ui.add(close_btn).on_hover_text("Close (Fermer)").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }

                // Maximize / Restore button (green/teal)
                let max_icon = if is_maximized {
                    &icons.window_restore
                } else {
                    &icons.window_maximize
                };
                let max_tip = if is_maximized {
                    "Restore (Restaurer)"
                } else {
                    "Maximize (Maximiser)"
                };
                let max_btn = egui::Button::image(
                    egui::Image::new(max_icon).fit_to_exact_size(btn_size),
                )
                .frame(false);
                if ui.add(max_btn).on_hover_text(max_tip).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                }

                // Minimize / Reduit button (amber)
                let min_btn = egui::Button::image(
                    egui::Image::new(&icons.window_minimize).fit_to_exact_size(btn_size),
                )
                .frame(false);
                if ui.add(min_btn).on_hover_text("Minimize (Réduire)").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            });
        });

        let row = bar.response.rect;
        let drag_rect = egui::Rect::from_min_max(
            row.left_top(),
            egui::pos2((row.right() - BUTTON_BAR_W).max(row.left() + 40.0), row.bottom()),
        );

        self.titlebar_drag(ui.ctx(), drag_rect, is_maximized);
    }

    /// Own implementation of "drag the window by its titlebar".
    ///
    /// Started only after the pointer actually travels, so a double-click (which
    /// toggles maximize) never degenerates into a pointer-grabbing move.
    fn titlebar_drag(&mut self, ctx: &egui::Context, drag_rect: egui::Rect, is_maximized: bool) {
        let (pointer_pos, primary_down, primary_pressed, primary_released, double_clicked) = ctx
            .input(|i| {
                (
                    i.pointer.interact_pos(),
                    i.pointer.primary_down(),
                    i.pointer.primary_pressed(),
                    i.pointer.primary_released(),
                    i.pointer.button_double_clicked(egui::PointerButton::Primary),
                )
            });
        let inside = pointer_pos.is_some_and(|p| drag_rect.contains(p));

        if primary_pressed && inside {
            self.titlebar_press = pointer_pos;
            self.titlebar_moving = false;
        }

        if let (Some(start), Some(pos)) = (self.titlebar_press, pointer_pos) {
            if primary_down && !self.titlebar_moving && pos.distance(start) > 5.0 {
                self.titlebar_moving = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                release_pointer_to_compositor(ctx);
            }
        }

        if primary_released || !primary_down {
            self.titlebar_press = None;
            self.titlebar_moving = false;
        }

        if double_clicked && inside && !self.titlebar_moving {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
        }
    }
}

/// The compositor takes ownership of the pointer for an interactive move or
/// resize and never reports the button release back to us. Drop egui's stale
/// pointer/drag bookkeeping, otherwise every subsequent click is swallowed
/// until the user presses Escape (which aborts the phantom drag).
fn release_pointer_to_compositor(ctx: &egui::Context) {
    ctx.stop_dragging();
    ctx.input_mut(|i| i.pointer = egui::PointerState::default());
    // Repaint so the UI immediately drops hover/drag visuals instead of waiting
    // for the next pointer motion event.
    ctx.request_repaint();
}

fn handle_window_resize(ui: &mut egui::Ui, app_rect: egui::Rect, resizing: &mut bool) {
    let border_hit = 8.0;
    let outer_rect = app_rect.expand(border_hit);
    let inner_rect = app_rect.shrink(border_hit);

    let Some(pointer_pos) = ui.ctx().pointer_latest_pos() else {
        *resizing = false;
        return;
    };

    if ui.input(|i| !i.pointer.any_down()) {
        *resizing = false;
    }

    if !outer_rect.contains(pointer_pos) || inner_rect.contains(pointer_pos) {
        return;
    }

    let left = pointer_pos.x < app_rect.left() + border_hit;
    let right = pointer_pos.x > app_rect.right() - border_hit;
    let top = pointer_pos.y < app_rect.top() + border_hit;
    let bottom = pointer_pos.y > app_rect.bottom() - border_hit;

    let (dir, cursor) = match (left, right, top, bottom) {
        (true, false, true, false) => (Some(egui::ResizeDirection::NorthWest), egui::CursorIcon::ResizeNorthWest),
        (false, true, true, false) => (Some(egui::ResizeDirection::NorthEast), egui::CursorIcon::ResizeNorthEast),
        (true, false, false, true) => (Some(egui::ResizeDirection::SouthWest), egui::CursorIcon::ResizeSouthWest),
        (false, true, false, true) => (Some(egui::ResizeDirection::SouthEast), egui::CursorIcon::ResizeSouthEast),
        (true, false, false, false) => (Some(egui::ResizeDirection::West), egui::CursorIcon::ResizeWest),
        (false, true, false, false) => (Some(egui::ResizeDirection::East), egui::CursorIcon::ResizeEast),
        (false, false, true, false) => (Some(egui::ResizeDirection::North), egui::CursorIcon::ResizeNorth),
        (false, false, false, true) => (Some(egui::ResizeDirection::South), egui::CursorIcon::ResizeSouth),
        _ => (None, egui::CursorIcon::Default),
    };

    if let Some(dir) = dir {
        ui.ctx().set_cursor_icon(cursor);
        // Only *start* a resize on a fresh press in the border, never by dragging
        // into the border with the button already down.
        if !*resizing && ui.input(|i| i.pointer.primary_pressed()) {
            *resizing = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
            release_pointer_to_compositor(ui.ctx());
        }
    }
}

impl eframe::App for SevenRustZip {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if self.busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let is_maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        let shadow_margin: f32 = if is_maximized { 0.0 } else { 14.0 };
        let corner_radius: u8 = if is_maximized { 0 } else { 10 };

        let outer_rect = ui.available_rect_before_wrap();
        let app_rect = outer_rect.shrink(shadow_margin);

        if !is_maximized {
            // Drop shadow behind the main app window to make it stand out against white backgrounds
            let ambient_shadow = egui::Shadow {
                offset: [0, 2],
                blur: 16,
                spread: 2,
                color: egui::Color32::from_black_alpha(45),
            };
            ui.painter().add(ambient_shadow.as_shape(app_rect, corner_radius));

            let key_shadow = egui::Shadow {
                offset: [0, 6],
                blur: 14,
                spread: 1,
                color: egui::Color32::from_black_alpha(65),
            };
            ui.painter().add(key_shadow.as_shape(app_rect, corner_radius));

            // Edge resize interaction for borderless window
            handle_window_resize(ui, app_rect, &mut self.resizing);
        }

        // Fill background of main app window
        ui.painter().rect_filled(app_rect, corner_radius, egui::Color32::from_rgb(240, 240, 243));

        // Child UI hosting all app panels within the rounded window rect
        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(app_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );

        let rows = self.visible_rows();
        let names: Vec<String> = match self.focus {
            Pane::Files => self.dirs.iter().map(|r| r.name.clone()).collect(),
            Pane::Archive => rows.iter().map(|r| r.name.clone()).collect(),
        };
        let typing = child_ui.ctx().egui_wants_keyboard_input();
        if !typing {
            self.typeahead(&mut child_ui, &names);
            self.handle_keys(&mut child_ui, &rows);
        }

        let titlebar_frame = egui::Frame::NONE
            .fill(egui::Color32::from_rgb(235, 237, 241))
            .corner_radius(egui::CornerRadius {
                nw: corner_radius,
                ne: corner_radius,
                sw: 0,
                se: 0,
            })
            .inner_margin(egui::Margin::symmetric(10, 6));
        egui::Panel::top("window_titlebar")
            .frame(titlebar_frame)
            .show(&mut child_ui, |ui| {
                self.ui_titlebar(ui);
            });

        let toolbar_frame = egui::Frame::NONE
            .fill(egui::Color32::from_rgb(246, 247, 249))
            .inner_margin(egui::Margin::symmetric(8, 4));
        egui::Panel::top("toolbar")
            .frame(toolbar_frame)
            .show(&mut child_ui, |ui| {
                self.ui_toolbar(ui);
            });

        let status_frame = egui::Frame::NONE
            .fill(egui::Color32::from_rgb(238, 240, 244))
            .corner_radius(egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: corner_radius,
                se: corner_radius,
            })
            .inner_margin(egui::Margin::symmetric(10, 4));
        egui::Panel::bottom("status")
            .frame(status_frame)
            .show(&mut child_ui, |ui| {
                self.ui_status(ui);
            });

        self.ui_preview(&mut child_ui);
        self.ui_dialogs(child_ui.ctx());

        egui::Panel::left("files")
            .resizable(true)
            .default_size(440.0)
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(248, 249, 251)))
            .show(&mut child_ui, |ui| {
                self.ui_files(ui);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::WHITE))
            .show(&mut child_ui, |ui| {
                self.ui_archive(ui);
            });

        // Crisp border outline around the window to make it stand out against white backgrounds
        let border_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(148, 163, 184)); // Slate-400
        ui.painter().rect_stroke(app_rect, corner_radius, border_stroke, egui::StrokeKind::Inside);
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
        match engine::list(path) {
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
        let tmp = std::env::temp_dir().join(format!("7rust-zip-selftest-{}", std::process::id()));
        match engine::extract(path, &tmp) {
            Ok(n) => println!("extract OK: {n} files -> {}", tmp.display()),
            Err(e) => {
                eprintln!("extract error: {e}");
                std::process::exit(1);
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
        return Ok(());
    }

    let start = args.get(1).map(PathBuf::from);

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 720.0])
            .with_min_inner_size([760.0, 460.0])
            .with_title("7rust-zip")
            .with_icon(window_icon())
            .with_decorations(false)
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "7rust-zip",
        opts,
        Box::new(move |cc| Ok(Box::new(SevenRustZip::new(cc, start)))),
    )
}
