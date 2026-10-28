//! 7rust-zip — a native, 7-Zip-style archiver for Linux.
//!
//! Pure Rust: sevenz-rust2 for 7z, `zip` for zip, `tar` + codecs for tarballs,
//! egui for the UI. No Wine, no Qt, no `7z` subprocess, and no external
//! launcher: archives can be browsed and files previewed in-app.

mod engine;

use eframe::egui;
use engine::{human, Format};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

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

    /// Path inside the archive being previewed (None = viewer closed).
    preview: Option<Preview>,
    focus: Pane,
    /// Type-to-find buffer, like Windows list views.
    typeahead: String,
    typeahead_at: Option<std::time::Instant>,
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
            preview: None,
            focus: Pane::Files,
            typeahead: String::new(),
            typeahead_at: None,
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
            // 7-Zip: folders first in BOTH directions (PanelSort.cpp does the
            // dir check before inverting for ascending/descending).
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
            // Keep the folder we came from selected, as explorers do.
            if let Some(name) = old.file_name() {
                let n = name.to_string_lossy();
                self.dir_sel = self.dirs.iter().position(|r| r.name == n);
            }
        }
    }

    // --------------------------------------------------------------- archive

    fn open_archive(&mut self, path: PathBuf) {
        if !looks_like_archive(&path) {
            // Never hand off to another program: show a preview instead.
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

        // Collect immediate children of `inner`.
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
            // Fold nested paths into one row per immediate child.
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

        // Sort: folders first in both directions (7-Zip semantics).
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
                        // Ignore stale results if the user moved on.
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

    /// Windows-style type-to-find: jump to the first item with this prefix.
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
        // Reset the buffer after a short idle, like Windows.
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
    // Fall back to magic bytes for extensionless files.
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

fn cmp_names(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

fn light_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    v.panel_fill = egui::Color32::from_rgb(243, 243, 243);
    v.window_fill = egui::Color32::from_rgb(248, 248, 248);
    v.extreme_bg_color = egui::Color32::WHITE;
    v.faint_bg_color = egui::Color32::from_rgb(250, 250, 250);
    v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(243, 243, 243);
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(227, 227, 227);
    v.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(227, 227, 227);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(204, 226, 247);
    v.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(204, 226, 247);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(0, 120, 215);
    v.selection.bg_fill = egui::Color32::from_rgb(0, 120, 215);
    v.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
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

/// Render a byte blob as text if it plausibly is text.
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
        ui.horizontal(|ui| {
            if ui.button("Open").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .set_directory(&self.cwd)
                    .add_filter(
                        "Archives",
                        &["7z", "zip", "tar", "gz", "tgz", "xz", "txz", "bz2", "zst", "rar"],
                    )
                    .pick_file()
                {
                    self.open_archive(p);
                }
            }
            if ui
                .add_enabled(self.archive.is_some(), egui::Button::new("Extract"))
                .clicked()
            {
                if let Some(d) = rfd::FileDialog::new()
                    .set_directory(&self.cwd)
                    .pick_folder()
                {
                    self.extract_to(d);
                }
            }
            if ui
                .add_enabled(self.archive.is_some(), egui::Button::new("Test"))
                .clicked()
            {
                self.run_verify();
            }
            if ui.button("Add").clicked() {
                self.show_create = true;
                self.create_name = "archive.7z".into();
                self.create_sources = self
                    .dir_sel
                    .and_then(|i| self.dirs.get(i))
                    .map(|r| vec![self.cwd.join(&r.name)])
                    .unwrap_or_default();
            }
            ui.separator();
            if ui.button("Up").clicked() {
                match self.focus {
                    Pane::Files => self.go_up(),
                    Pane::Archive => self.archive_go_up(),
                }
            }
            if ui.button("Refresh").clicked() {
                self.refresh_dir();
                if let Some(a) = self.archive.clone() {
                    self.open_archive(a);
                }
            }
            ui.separator();
            ui.label("Find:");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .desired_width(160.0)
                    .hint_text("name"),
            );
            if resp.changed() {
                self.arc_sel = None;
                self.dir_sel = None;
            }
            if ui.button("✖").clicked() {
                self.filter.clear();
            }
        });
    }

    fn ui_files(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Folder").strong());
            ui.label(
                egui::RichText::new(self.cwd.display().to_string())
                    .small()
                    .color(egui::Color32::from_gray(90)),
            );
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("files_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut clicked = None;
                let mut dbl = None;
                let mut go_parent = false;

                // 7-Zip pins a ".." row at the top of every listing.
                if self.cwd.parent().is_some() {
                    let r = ui.add(
                        egui::Button::new(
                            egui::RichText::new("..").monospace().color(
                                egui::Color32::from_rgb(150, 90, 0),
                            ),
                        )
                        .frame(false)
                        .min_size(egui::vec2(ui.available_width(), 18.0)),
                    );
                    if r.double_clicked() || r.clicked() && false {
                        go_parent = true;
                    }
                }

                for (i, row) in self.dirs.iter().enumerate() {
                    let sel = self.dir_sel == Some(i);
                    let (mark, color) = if row.is_dir {
                        ("[D]", egui::Color32::from_rgb(140, 90, 0))
                    } else if row.is_archive {
                        ("[A]", egui::Color32::from_rgb(0, 110, 60))
                    } else {
                        ("   ", egui::Color32::from_gray(60))
                    };
                    let size = if row.is_dir {
                        format!("{:>9}", "")
                    } else {
                        format!("{:>9}", human(row.size))
                    };
                    let text = format!(
                        "{mark} {:<34} {size}  {}",
                        truncate(&row.name, 34),
                        row.modified
                    );
                    let resp = ui.add(
                        egui::Button::new(
                            egui::RichText::new(text).monospace().color(color),
                        )
                        .frame(false)
                        .fill(if sel {
                            egui::Color32::from_rgb(204, 226, 247)
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .min_size(egui::vec2(ui.available_width(), 18.0)),
                    );
                    if resp.clicked() {
                        clicked = Some(i);
                        self.focus = Pane::Files;
                    }
                    if resp.double_clicked() {
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
        let Some(arc) = self.archive.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(
                        "Double-click an archive to open it.\n\
                         Archives open here; nothing is handed to another app.",
                    )
                    .size(14.0)
                    .color(egui::Color32::from_gray(120)),
                );
            });
            return;
        };

        let fmt = engine::detect(&arc);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Archive").strong());
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
                if ui.button("Close").clicked() {
                    self.close_archive();
                }
            });
        });
        ui.separator();

        let rows = self.visible_rows();

        // Column headers are buttons, like 7-Zip's list view.
        ui.horizontal(|ui| {
            let mut clicked = None;
            for (label, col) in [
                ("Name", SortCol::Name),
                ("Size", SortCol::Size),
                ("Packed", SortCol::Packed),
                ("Modified", SortCol::Modified),
            ] {
                let marker = if self.sort_col == col {
                    if self.ascending {
                        " +"
                    } else {
                        " -"
                    }
                } else {
                    ""
                };
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(format!("{label}{marker}")).strong().small(),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    clicked = Some(col);
                }
            }
            if let Some(c) = clicked {
                self.toggle_sort(c);
            }
        });
        ui.separator();

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
                    let r = ui.add(
                        egui::Button::new(
                            egui::RichText::new("..")
                                .monospace()
                                .color(egui::Color32::from_rgb(150, 90, 0)),
                        )
                        .frame(false)
                        .min_size(egui::vec2(ui.available_width(), 18.0)),
                    );
                    if r.double_clicked() {
                        go_parent = true;
                    }
                }

                for (i, row) in rows.iter().enumerate() {
                    let sel = self.arc_sel == Some(i);
                    let (mark, color) = if row.is_dir {
                        ("[D]", egui::Color32::from_rgb(140, 90, 0))
                    } else {
                        ("   ", egui::Color32::from_gray(60))
                    };
                    let text = format!(
                        "{mark} {:<34} {:>9} {:>9}  {}",
                        truncate(&row.name, 34),
                        if row.is_dir {
                            String::new()
                        } else {
                            human(row.size)
                        },
                        if row.packed > 0 {
                            human(row.packed)
                        } else {
                            String::new()
                        },
                        row.modified
                    );
                    let resp = ui.add(
                        egui::Button::new(egui::RichText::new(text).monospace().color(color))
                            .frame(false)
                            .fill(if sel {
                                egui::Color32::from_rgb(204, 226, 247)
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .min_size(egui::vec2(ui.available_width(), 18.0)),
                    );
                    if resp.clicked() {
                        clicked = Some(i);
                        self.focus = Pane::Archive;
                    }
                    if resp.double_clicked() {
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
                    ui.label(egui::RichText::new("Viewer").strong());
                    ui.label(
                        egui::RichText::new(&name)
                            .monospace()
                            .color(egui::Color32::from_rgb(0, 90, 160)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✖").clicked() {
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
                    egui::RichText::new(format!("pane: {pane}"))
                        .small()
                        .color(egui::Color32::from_gray(120)),
                );
            });
        });
    }

    fn ui_create_dialog(&mut self, ui: &egui::Ui) {
        if !self.show_create {
            return;
        }
        let mut open = self.show_create;
        egui::Window::new("Create archive")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.create_name).desired_width(280.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Format:");
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
                        egui::RichText::new("No sources selected")
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
                            egui::Button::new("Create"),
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
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

impl eframe::App for SevenRustZip {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if self.busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Keyboard handling needs the current row set; compute once.
        let rows = self.visible_rows();
        let names: Vec<String> = match self.focus {
            Pane::Files => self.dirs.iter().map(|r| r.name.clone()).collect(),
            Pane::Archive => rows.iter().map(|r| r.name.clone()).collect(),
        };
        // Ignore keys while typing in the search box.
        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing {
            self.typeahead(ui, &names);
            self.handle_keys(ui, &rows);
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(2.0);
            self.ui_toolbar(ui);
            ui.add_space(2.0);
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            self.ui_status(ui);
        });

        self.ui_preview(ui);
        self.ui_create_dialog(ui);

        egui::Panel::left("files")
            .resizable(true)
            .default_size(430.0)
            .show(ui, |ui| {
                self.ui_files(ui);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            self.ui_archive(ui);
        });
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

    // Allow opening an archive (or folder) straight from the command line.
    let start = args.get(1).map(PathBuf::from);

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 720.0])
            .with_min_inner_size([760.0, 460.0])
            .with_title("7rust-zip")
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "7rust-zip",
        opts,
        Box::new(move |cc| Ok(Box::new(SevenRustZip::new(cc, start)))),
    )
}
