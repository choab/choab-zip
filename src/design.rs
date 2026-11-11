//! Visual language for choab-zip: strictly greyscale, one type scale, one
//! icon set drawn as vectors.
//!
//! The palette never introduces hue: every surface, border and glyph is a
//! shade of black, white or grey. Light and dark variants follow the desktop
//! colour scheme and keep the text at high contrast.

use eframe::egui::{self, Color32, FontId, Rect, Response, Sense, Shape, Stroke, Vec2};

// ---------------------------------------------------------------- metrics

pub const HEADER_H: f32 = 46.0;
pub const COMMAND_H: f32 = 44.0;
pub const STATUS_H: f32 = 26.0;
pub const ROW_H: f32 = 28.0;
pub const SIDEBAR_W: f32 = 198.0;

// ---------------------------------------------------------------- palette

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    pub fn flipped(self) -> Self {
        match self {
            Mode::Light => Mode::Dark,
            Mode::Dark => Mode::Light,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub mode: Mode,

    pub bg_header: Color32,
    pub bg_command: Color32,
    pub bg_sidebar: Color32,
    pub bg_content: Color32,
    pub bg_status: Color32,
    pub bg_field: Color32,

    pub border: Color32,

    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,

    /// Near-black in light mode, near-white in dark mode.
    pub ink: Color32,
    pub ink_fg: Color32,
    /// The seam that separates the two plates of the app mark.
    pub seam: Color32,
    pub danger: Color32,

    pub row_hover: Color32,
    pub row_selected: Color32,
    pub row_focus: Color32,

    pub shadow: egui::Shadow,
}

/// Linear blend, `t = 0` returns `a`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn grey(v: u8) -> Color32 {
    Color32::from_rgb(v, v, v)
}

fn luminance(c: Color32) -> f32 {
    0.2126 * c.r() as f32 / 255.0 + 0.7152 * c.g() as f32 / 255.0 + 0.0722 * c.b() as f32 / 255.0
}

impl Palette {
    pub fn new(mode: Mode) -> Self {
        match mode {
            Mode::Light => Self {
                mode,
                bg_header: grey(0xFF),
                bg_command: grey(0xFF),
                bg_sidebar: grey(0xF2),
                bg_content: grey(0xFF),
                bg_status: grey(0xFA),
                bg_field: grey(0xEA),
                border: grey(0xD2),
                text: grey(0x0A),
                text_muted: grey(0x48),
                text_faint: grey(0x70),
                ink: grey(0x0A),
                ink_fg: grey(0xFF),
                seam: grey(0x8C),
                danger: grey(0x0A),
                row_hover: grey(0xEC),
                row_selected: grey(0xD4),
                row_focus: grey(0x0A),
                shadow: egui::Shadow {
                    offset: [0, 6],
                    blur: 22,
                    spread: 0,
                    color: Color32::from_black_alpha(55),
                },
            },
            Mode::Dark => Self {
                mode,
                bg_header: grey(0x15),
                bg_command: grey(0x15),
                bg_sidebar: grey(0x0F),
                bg_content: grey(0x1A),
                bg_status: grey(0x13),
                bg_field: grey(0x27),
                border: grey(0x3A),
                text: grey(0xF7),
                text_muted: grey(0xBE),
                text_faint: grey(0x8E),
                ink: grey(0xF7),
                ink_fg: grey(0x0A),
                seam: grey(0x8C),
                danger: grey(0xF7),
                row_hover: grey(0x26),
                row_selected: grey(0x3D),
                row_focus: grey(0xF7),
                shadow: egui::Shadow {
                    offset: [0, 8],
                    blur: 26,
                    spread: 0,
                    color: Color32::from_black_alpha(110),
                },
            },
        }
    }
}

// ------------------------------------------------------- system settings

/// Read a minimal ini subset: `key = value` inside `[section]`.
fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            current = line[1..line.len() - 1].to_string();
            continue;
        }
        if current != section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn parse_rgb(v: &str) -> Option<Color32> {
    let parts: Vec<u8> = v
        .trim()
        .trim_start_matches('#')
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    if parts.len() >= 3 {
        return Some(Color32::from_rgb(parts[0], parts[1], parts[2]));
    }
    None
}

fn kde_config() -> Option<String> {
    let home = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    std::fs::read_to_string(home.join("kdeglobals")).ok()
}

/// Best-effort read of the Plasma colour scheme. The accent colour is
/// deliberately ignored: this app is greyscale by design.
pub fn system_mode() -> Mode {
    let Some(cfg) = kde_config() else {
        // No desktop hint: white is the default look.
        return Mode::Light;
    };
    if let Some(scheme) = ini_value(&cfg, "General", "ColorScheme") {
        if scheme.to_lowercase().contains("dark") {
            return Mode::Dark;
        }
        return Mode::Light;
    }
    if let Some(bg) =
        ini_value(&cfg, "Colors:Window", "BackgroundNormal").and_then(|v| parse_rgb(&v))
    {
        if luminance(bg) < 0.45 {
            return Mode::Dark;
        }
        return Mode::Light;
    }
    Mode::Light
}

/// Where the one user preference this app has is stored.
fn prefs_file() -> Option<std::path::PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("choab-zip").join("prefs.conf"))
}

/// What the user asked for. `System` is the opt-in way back to following the
/// desktop scheme; the default (nothing saved) is white.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    Light,
    Dark,
    System,
}

/// Everything this app remembers between runs. One small ini-ish file, written
/// on change; unknown keys are ignored so a hand-edited file stays harmless.
#[derive(Debug, Default, Clone)]
pub struct Prefs {
    /// Theme the user picked, if they ever picked one.
    pub theme: Option<ThemeChoice>,
    /// Directory that was open when the app last closed.
    pub last_dir: Option<std::path::PathBuf>,
}

impl Prefs {
    pub fn load() -> Self {
        match prefs_file().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => Self::parse(&text),
            None => Self::default(),
        }
    }

    /// Parse the file format. Unknown or malformed lines are ignored, so a
    /// hand-edited or older file never breaks startup.
    pub fn parse(text: &str) -> Self {
        let mut prefs = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "theme" => {
                    prefs.theme = match value {
                        "light" => Some(ThemeChoice::Light),
                        "dark" => Some(ThemeChoice::Dark),
                        "system" => Some(ThemeChoice::System),
                        _ => None,
                    }
                }
                "last_dir" if !value.is_empty() => {
                    prefs.last_dir = Some(std::path::PathBuf::from(value))
                }
                _ => {}
            }
        }
        prefs
    }

    /// The palette to start with: the saved choice, or white.
    pub fn mode(&self) -> Mode {
        self.theme
            .map(ThemeChoice::mode)
            .unwrap_or(Mode::Light)
    }

    pub fn save(&self) {
        let Some(path) = prefs_file() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut out = String::new();
        if let Some(theme) = self.theme {
            let value = match theme {
                ThemeChoice::Light => "light",
                ThemeChoice::Dark => "dark",
                ThemeChoice::System => "system",
            };
            out.push_str(&format!("theme={value}\n"));
        }
        if let Some(dir) = &self.last_dir {
            out.push_str(&format!("last_dir={}\n", dir.display()));
        }
        let _ = std::fs::write(path, out);
    }

    /// Record the theme the user just chose, keeping everything else.
    pub fn remember_mode(mode: Mode) {
        let mut prefs = Self::load();
        prefs.theme = Some(match mode {
            Mode::Light => ThemeChoice::Light,
            Mode::Dark => ThemeChoice::Dark,
        });
        prefs.save();
    }

    /// Record the directory being browsed, keeping everything else.
    pub fn remember_dir(dir: &std::path::Path) {
        let mut prefs = Self::load();
        if prefs.last_dir.as_deref() == Some(dir) {
            return;
        }
        prefs.last_dir = Some(dir.to_path_buf());
        prefs.save();
    }
}

impl ThemeChoice {
    pub fn mode(self) -> Mode {
        match self {
            ThemeChoice::Light => Mode::Light,
            ThemeChoice::Dark => Mode::Dark,
            ThemeChoice::System => system_mode(),
        }
    }
}

// ------------------------------------------------------------- egui style

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let theme = match p.mode {
        Mode::Light => egui::Theme::Light,
        Mode::Dark => egui::Theme::Dark,
    };
    let mut style = (*ctx.style_of(theme)).clone();

    let mut v = match p.mode {
        Mode::Light => egui::Visuals::light(),
        Mode::Dark => egui::Visuals::dark(),
    };

    v.dark_mode = p.mode == Mode::Dark;
    v.panel_fill = p.bg_content;
    v.window_fill = p.bg_content;
    v.faint_bg_color = p.row_hover;
    v.extreme_bg_color = p.bg_field;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.text;
    v.window_corner_radius = egui::CornerRadius::same(12);
    v.menu_corner_radius = egui::CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_shadow = egui::Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: Color32::from_black_alpha(80),
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(65),
    };

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = Color32::TRANSPARENT;
    w.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    w.noninteractive.bg_stroke = Stroke::NONE;
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text_muted);
    w.noninteractive.corner_radius = egui::CornerRadius::same(6);

    w.inactive.bg_fill = mix(p.bg_field, p.text, 0.32);
    w.inactive.weak_bg_fill = p.bg_field;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = egui::CornerRadius::same(6);

    w.hovered.bg_fill = mix(p.bg_field, p.text, 0.45);
    w.hovered.weak_bg_fill = mix(p.bg_field, p.text, 0.12);
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = egui::CornerRadius::same(6);

    w.active.bg_fill = p.ink;
    w.active.weak_bg_fill = p.row_selected;
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.0, p.ink_fg);
    w.active.corner_radius = egui::CornerRadius::same(6);

    w.open.bg_fill = p.row_hover;
    w.open.weak_bg_fill = p.row_hover;
    w.open.bg_stroke = Stroke::new(1.0, p.border);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = egui::CornerRadius::same(6);

    v.selection.bg_fill = p.row_selected;
    v.selection.stroke = Stroke::new(1.0, p.ink);
    v.striped = false;

    style.visuals = v;
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(14);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.spacing.scroll.floating = true;
    style.spacing.scroll.foreground_color = false;
    style.spacing.scroll.floating_width = 6.0;
    style.spacing.scroll.bar_width = 10.0;
    style.spacing.scroll.bar_inner_margin = 3.0;
    style.spacing.scroll.bar_outer_margin = 2.0;
    style.spacing.interact_size.y = 26.0;
    style.interaction.tooltip_delay = 0.35;

    style.text_styles = [
        (egui::TextStyle::Heading, FontId::proportional(17.0)),
        (egui::TextStyle::Body, FontId::proportional(13.0)),
        (egui::TextStyle::Button, FontId::proportional(12.5)),
        (egui::TextStyle::Small, FontId::proportional(11.0)),
        (egui::TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();

    ctx.set_style_of(theme, style);
    ctx.set_theme(theme);
}

// ------------------------------------------------------------------ icons

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Add,
    Extract,
    ExtractSelected,
    Check,
    Hash,
    FolderPlus,
    Copy,
    Move,
    Delete,
    Info,
    Up,
    Refresh,
    Search,
    Close,
    ChevronRight,
    ChevronDown,
    Minimize,
    Maximize,
    Restore,
    Sun,
    Moon,
    Home,
    Download,
    Documents,
    Pictures,
    Music,
    Video,
    Desktop,
    Disk,
    Folder,
    FolderUp,
    Archive,
    File,
    /// Page with a play mark: video.
    FileVideo,
    /// Page with an equalizer: audio.
    FileAudio,
    /// Page with a mountain: image.
    FileImage,
    /// Page with ruled lines: document / text.
    FileDocument,
    External,
    Lock,
    Sidebar,
    Rows,
    Pencil,
}

/// Draw one icon into `rect`. Stroke weight scales with the icon so 16 px
/// rows and 44 px empty states look equally deliberate.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let w = (rect.width() * 0.115).clamp(1.6, 3.0);
    let s = Stroke::new(w, color);
    let p = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    let line = |a: f32, b: f32, c: f32, d: f32| {
        painter.line_segment([p(a, b), p(c, d)], s);
    };
    let poly = |pts: &[(f32, f32)]| {
        painter.add(Shape::line(pts.iter().map(|&(x, y)| p(x, y)).collect(), s));
    };
    let closed = |pts: &[(f32, f32)]| {
        painter.add(Shape::closed_line(
            pts.iter().map(|&(x, y)| p(x, y)).collect(),
            s,
        ));
    };
    let dot = |x: f32, y: f32, r: f32| {
        painter.circle_filled(p(x, y), r * rect.width(), color);
    };
    let ring = |x: f32, y: f32, r: f32| {
        painter.circle_stroke(p(x, y), r * rect.width(), s);
    };
    let rrect = |x0: f32, y0: f32, x1: f32, y1: f32, frac: f32| {
        painter.rect_stroke(
            Rect::from_min_max(p(x0, y0), p(x1, y1)),
            rect.width() * frac,
            s,
            egui::StrokeKind::Inside,
        );
    };
    // The plain page outline every file-family glyph is built on.
    let page = || {
        closed(&[
            (0.28, 0.12),
            (0.62, 0.12),
            (0.77, 0.29),
            (0.77, 0.88),
            (0.28, 0.88),
        ]);
        line(0.62, 0.12, 0.62, 0.29);
        line(0.62, 0.29, 0.77, 0.29);
    };
    let arc = |cx: f32, cy: f32, rx: f32, ry: f32, from: f32, to: f32, n: usize| {
        let mut pts = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let t = from + (to - from) * (i as f32 / n as f32);
            pts.push(p(cx + rx * t.cos(), cy + ry * t.sin()));
        }
        painter.add(Shape::line(pts, s));
    };

    match icon {
        // ---------------------------------------------------- actions
        Icon::Add => {
            line(0.5, 0.16, 0.5, 0.84);
            line(0.16, 0.5, 0.84, 0.5);
        }
        Icon::Extract => {
            line(0.5, 0.10, 0.5, 0.52);
            poly(&[(0.33, 0.37), (0.5, 0.55), (0.67, 0.37)]);
            poly(&[(0.17, 0.62), (0.17, 0.86), (0.83, 0.86), (0.83, 0.62)]);
        }
        Icon::ExtractSelected => {
            line(0.38, 0.10, 0.38, 0.48);
            poly(&[(0.24, 0.35), (0.38, 0.50), (0.52, 0.35)]);
            poly(&[(0.14, 0.62), (0.14, 0.86), (0.80, 0.86), (0.80, 0.62)]);
            poly(&[(0.60, 0.34), (0.72, 0.46), (0.92, 0.18)]);
        }
        Icon::Check => poly(&[(0.14, 0.54), (0.40, 0.80), (0.86, 0.24)]),
        Icon::Hash => {
            line(0.38, 0.12, 0.32, 0.88);
            line(0.68, 0.12, 0.62, 0.88);
            line(0.14, 0.36, 0.86, 0.36);
            line(0.12, 0.64, 0.84, 0.64);
        }
        Icon::FolderPlus => {
            closed(&[
                (0.10, 0.26),
                (0.42, 0.26),
                (0.50, 0.38),
                (0.90, 0.38),
                (0.90, 0.84),
                (0.10, 0.84),
            ]);
            line(0.50, 0.50, 0.50, 0.70);
            line(0.40, 0.60, 0.60, 0.60);
        }
        Icon::Copy => {
            rrect(0.34, 0.12, 0.86, 0.64, 0.06);
            poly(&[(0.14, 0.30), (0.14, 0.86), (0.66, 0.86), (0.66, 0.66)]);
        }
        Icon::Move => {
            line(0.10, 0.5, 0.64, 0.5);
            poly(&[(0.50, 0.33), (0.68, 0.5), (0.50, 0.67)]);
            line(0.87, 0.18, 0.87, 0.82);
        }
        Icon::Delete => {
            line(0.16, 0.28, 0.84, 0.28);
            poly(&[(0.38, 0.28), (0.38, 0.16), (0.62, 0.16), (0.62, 0.28)]);
            poly(&[(0.25, 0.28), (0.30, 0.87), (0.70, 0.87), (0.75, 0.28)]);
            line(0.43, 0.42, 0.44, 0.74);
            line(0.57, 0.42, 0.56, 0.74);
        }
        Icon::Info => {
            ring(0.5, 0.5, 0.34);
            dot(0.5, 0.31, 0.045);
            line(0.5, 0.44, 0.5, 0.70);
        }
        Icon::Up => {
            line(0.5, 0.86, 0.5, 0.24);
            poly(&[(0.29, 0.43), (0.5, 0.20), (0.71, 0.43)]);
        }
        Icon::Refresh => {
            arc(0.5, 0.5, 0.32, 0.32, 0.75, 5.45, 24);
            poly(&[(0.70, 0.10), (0.84, 0.31), (0.61, 0.32)]);
        }
        Icon::Search => {
            ring(0.45, 0.44, 0.26);
            line(0.64, 0.63, 0.87, 0.87);
        }
        Icon::Close => {
            line(0.24, 0.24, 0.76, 0.76);
            line(0.76, 0.24, 0.24, 0.76);
        }
        Icon::ChevronRight => poly(&[(0.38, 0.20), (0.62, 0.5), (0.38, 0.80)]),
        Icon::ChevronDown => poly(&[(0.22, 0.40), (0.5, 0.66), (0.78, 0.40)]),
        Icon::Minimize => line(0.26, 0.56, 0.74, 0.56),
        Icon::Maximize => rrect(0.28, 0.28, 0.72, 0.72, 0.05),
        Icon::Restore => {
            rrect(0.36, 0.16, 0.84, 0.62, 0.05);
            rrect(0.16, 0.38, 0.64, 0.84, 0.05);
        }
        Icon::Sun => {
            ring(0.5, 0.5, 0.15);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let (dx, dy) = (a.cos(), a.sin());
                line(
                    0.5 + dx * 0.24,
                    0.5 + dy * 0.24,
                    0.5 + dx * 0.37,
                    0.5 + dy * 0.37,
                );
            }
        }
        Icon::Moon => {
            let mut pts: Vec<egui::Pos2> = Vec::with_capacity(42);
            for i in 0..=20 {
                let t = 0.95 + (5.30 - 0.95) * (i as f32 / 20.0);
                pts.push(p(0.47 + 0.35 * t.cos(), 0.5 + 0.35 * t.sin()));
            }
            for i in 0..=20 {
                let t = 5.30 - (5.30 - 0.95) * (i as f32 / 20.0);
                pts.push(p(0.64 + 0.29 * t.cos(), 0.5 + 0.29 * t.sin()));
            }
            painter.add(Shape::line(pts, s));
        }
        Icon::Home => {
            poly(&[(0.10, 0.47), (0.5, 0.14), (0.90, 0.47)]);
            poly(&[(0.23, 0.42), (0.23, 0.87), (0.77, 0.87), (0.77, 0.42)]);
            poly(&[(0.42, 0.87), (0.42, 0.64), (0.58, 0.64), (0.58, 0.87)]);
        }
        Icon::Download => {
            line(0.5, 0.12, 0.5, 0.58);
            poly(&[(0.32, 0.42), (0.5, 0.60), (0.68, 0.42)]);
            line(0.18, 0.86, 0.82, 0.86);
        }
        Icon::Documents => {
            closed(&[
                (0.22, 0.12),
                (0.60, 0.12),
                (0.78, 0.30),
                (0.78, 0.88),
                (0.22, 0.88),
            ]);
            line(0.58, 0.12, 0.58, 0.32);
            line(0.58, 0.32, 0.78, 0.32);
            line(0.32, 0.52, 0.68, 0.52);
            line(0.32, 0.66, 0.68, 0.66);
            line(0.32, 0.80, 0.54, 0.80);
        }
        Icon::Pictures => {
            rrect(0.10, 0.18, 0.90, 0.82, 0.06);
            ring(0.33, 0.37, 0.075);
            poly(&[
                (0.16, 0.74),
                (0.40, 0.48),
                (0.56, 0.66),
                (0.66, 0.55),
                (0.84, 0.75),
            ]);
        }
        Icon::Music => {
            dot(0.32, 0.74, 0.105);
            line(0.42, 0.74, 0.42, 0.16);
            poly(&[(0.42, 0.16), (0.74, 0.24), (0.74, 0.42)]);
        }
        Icon::Video => {
            rrect(0.10, 0.24, 0.90, 0.76, 0.08);
            painter.add(Shape::convex_polygon(
                vec![p(0.42, 0.38), p(0.42, 0.62), p(0.63, 0.5)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Desktop => {
            rrect(0.10, 0.16, 0.90, 0.68, 0.06);
            line(0.5, 0.68, 0.5, 0.82);
            line(0.30, 0.84, 0.70, 0.84);
        }
        Icon::Disk => {
            rrect(0.12, 0.24, 0.88, 0.76, 0.10);
            line(0.28, 0.58, 0.62, 0.58);
            dot(0.75, 0.36, 0.035);
        }

        // ------------------------------------------------ file types
        Icon::Folder => {
            closed(&[
                (0.08, 0.24),
                (0.40, 0.24),
                (0.49, 0.36),
                (0.92, 0.36),
                (0.92, 0.86),
                (0.08, 0.86),
            ]);
        }
        Icon::FolderUp => {
            closed(&[
                (0.08, 0.24),
                (0.40, 0.24),
                (0.49, 0.36),
                (0.92, 0.36),
                (0.92, 0.86),
                (0.08, 0.86),
            ]);
            line(0.5, 0.78, 0.5, 0.48);
            poly(&[(0.37, 0.61), (0.5, 0.46), (0.63, 0.61)]);
        }
        Icon::Archive => {
            // A box with a zip fastener down the middle: the shape itself says
            // "archive", so no label is needed to recognise one.
            rrect(0.14, 0.12, 0.86, 0.88, 0.05);
            line(0.50, 0.12, 0.50, 0.60);
            line(0.41, 0.21, 0.59, 0.21);
            line(0.41, 0.30, 0.59, 0.30);
            line(0.41, 0.39, 0.59, 0.39);
            line(0.41, 0.48, 0.59, 0.48);
            rrect(0.43, 0.57, 0.57, 0.71, 0.03);
            line(0.50, 0.71, 0.50, 0.82);
        }
        Icon::File => {
            closed(&[
                (0.28, 0.12),
                (0.62, 0.12),
                (0.77, 0.29),
                (0.77, 0.88),
                (0.28, 0.88),
            ]);
            line(0.62, 0.12, 0.62, 0.29);
            line(0.62, 0.29, 0.77, 0.29);
        }

        Icon::FileVideo => {
            page();
            closed(&[(0.40, 0.44), (0.64, 0.57), (0.40, 0.70)]);
        }
        Icon::FileAudio => {
            page();
            line(0.39, 0.48, 0.39, 0.72);
            line(0.50, 0.42, 0.50, 0.72);
            line(0.61, 0.52, 0.61, 0.72);
        }
        Icon::FileImage => {
            page();
            poly(&[(0.34, 0.74), (0.45, 0.58), (0.53, 0.68), (0.60, 0.60), (0.72, 0.74)]);
            dot(0.39, 0.44, 0.035);
        }
        Icon::FileDocument => {
            page();
            line(0.37, 0.46, 0.68, 0.46);
            line(0.37, 0.57, 0.68, 0.57);
            line(0.37, 0.68, 0.56, 0.68);
        }

        // -------------------------------------------------- extras
        Icon::External => {
            poly(&[(0.42, 0.58), (0.42, 0.86), (0.86, 0.86), (0.86, 0.58)]);
            line(0.56, 0.48, 0.88, 0.14);
            poly(&[(0.66, 0.14), (0.88, 0.14), (0.88, 0.38)]);
        }
        Icon::Lock => {
            rrect(0.26, 0.46, 0.74, 0.86, 0.08);
            arc(0.5, 0.46, 0.16, 0.18, std::f32::consts::PI, std::f32::consts::TAU, 16);
            line(0.34, 0.46, 0.34, 0.30);
            line(0.66, 0.46, 0.66, 0.30);
            line(0.5, 0.60, 0.5, 0.72);
        }
        Icon::Sidebar => {
            rrect(0.12, 0.18, 0.88, 0.82, 0.06);
            line(0.38, 0.18, 0.38, 0.82);
            line(0.20, 0.34, 0.30, 0.34);
            line(0.20, 0.48, 0.30, 0.48);
        }
        Icon::Rows => {
            for y in [0.26, 0.5, 0.74] {
                line(0.32, y, 0.88, y);
                dot(0.18, y, 0.04);
            }
        }
        Icon::Pencil => {
            poly(&[
                (0.18, 0.82),
                (0.22, 0.62),
                (0.64, 0.18),
                (0.80, 0.34),
                (0.38, 0.78),
                (0.18, 0.82),
            ]);
            line(0.58, 0.24, 0.74, 0.40);
        }
    }
}

/// The app mark: two plates split by a grey seam, with a "7" that stays whole
/// and bridges the seam.
pub fn paint_mark(painter: &egui::Painter, rect: Rect, p: &Palette) {
    let outer = (rect.width() * 0.22).round() as u8;
    let inner = (rect.width() * 0.09).round() as u8;
    let gap = rect.height() * 0.06;
    let mid = rect.center().y;

    // The seam shows through the gap between the plates.
    painter.rect_filled(rect, outer, p.seam);

    let top = Rect::from_min_max(rect.min, egui::pos2(rect.right(), mid - gap / 2.0));
    let bottom = Rect::from_min_max(egui::pos2(rect.left(), mid + gap / 2.0), rect.max);
    painter.rect_filled(
        top,
        egui::CornerRadius {
            nw: outer,
            ne: outer,
            sw: inner,
            se: inner,
        },
        p.ink,
    );
    painter.rect_filled(
        bottom,
        egui::CornerRadius {
            nw: inner,
            ne: inner,
            sw: outer,
            se: outer,
        },
        p.ink,
    );

    let s = Stroke::new(rect.width() * 0.135, p.ink_fg);
    let at = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    painter.line_segment([at(0.24, 0.28), at(0.78, 0.28)], s);
    painter.line_segment([at(0.74, 0.30), at(0.42, 0.80)], s);
}

// --------------------------------------------------------------- widgets

/// Flat toolbar button: 16 px icon, label, 30 px tall.
pub fn cmd_button(
    ui: &mut egui::Ui,
    p: &Palette,
    icon: Icon,
    label: &str,
    enabled: bool,
) -> Response {
    let font = FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), p.text);
    let w = 10.0 + 16.0 + 6.0 + galley.size().x + 10.0;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 30.0), sense);

    let painter = ui.painter();
    let hovered = enabled && resp.hovered();
    if hovered {
        painter.rect_filled(rect, 7, p.row_hover);
    }

    let (fg, ic) = if !enabled {
        (p.text_faint, p.text_faint)
    } else if hovered {
        (p.text, p.ink)
    } else {
        (p.text_muted, p.text_muted)
    };

    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 10.0 + 8.0, rect.center().y),
        Vec2::splat(16.0),
    );
    paint_icon(painter, icon_rect, icon, ic);
    painter.galley(
        egui::pos2(
            icon_rect.right() + 6.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        fg,
    );
    resp
}

/// Square icon-only button.
pub fn icon_button(
    ui: &mut egui::Ui,
    p: &Palette,
    icon: Icon,
    side: f32,
    enabled: bool,
    active: bool,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), sense);
    let painter = ui.painter();
    let hovered = enabled && resp.hovered();
    if active {
        painter.rect_filled(rect, 6, p.row_selected);
    } else if hovered {
        painter.rect_filled(rect, 6, p.row_hover);
    }
    let color = if !enabled {
        p.text_faint
    } else if active {
        p.ink
    } else if hovered {
        p.text
    } else {
        p.text_muted
    };
    let ic = Rect::from_center_size(rect.center(), Vec2::splat(15.0));
    paint_icon(painter, ic, icon, color);
    resp
}

/// 1 px horizontal rule spanning the available width.
pub fn hairline(ui: &mut egui::Ui, p: &Palette) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, p.border);
}

/// Small uppercase section label used in the sidebar.
pub fn section_label(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text.to_uppercase(),
        FontId::proportional(10.0),
        p.text_faint,
    );
    ui.add_space(2.0);
}

/// Draw a rounded tag such as the archive format badge.
pub fn chip(painter: &egui::Painter, rect: Rect, p: &Palette, text: &str) {
    painter.rect_filled(rect, 4, p.bg_field);
    painter.rect_stroke(
        rect,
        4,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::proportional(10.5),
        p.text_muted,
    );
}

pub fn chip_width(ui: &egui::Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            FontId::proportional(10.5),
            Color32::WHITE,
        )
        .size()
        .x
        + 12.0
}

/// Truncate `text` with an ellipsis so it fits `max_w`.
/// Keep the *end* of the text, dropping the front: for file names, where the
/// extension and the distinguishing tail matter more than the prefix.
pub fn elide_left(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let measure = |s: String| ui.painter().layout_no_wrap(s, font.clone(), color);
    let full = measure(text.to_owned());
    if full.size().x <= max_w || max_w <= 0.0 {
        return full;
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let trial = "…".to_string() + &chars[chars.len() - mid..].iter().collect::<String>();
        if measure(trial).size().x <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    measure("…".to_string() + &chars[chars.len() - lo..].iter().collect::<String>())
}

/// Keep both ends and drop the middle. A long path or a long file name stays
/// recognisable, and nothing is drawn past `max_w`.
pub fn elide_middle(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let measure =
        |s: String| ui.painter().layout_no_wrap(s, font.clone(), color);
    let full = measure(text.to_owned());
    if full.size().x <= max_w || max_w <= 0.0 || text.is_empty() {
        return full;
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 4 {
        return elide(ui, text, font, color, max_w);
    }

    // Estimate how many characters fit, then shrink until it really fits. One
    // layout per step, and a handful of steps, keeps this cheap per frame.
    let per_char = (full.size().x / chars.len() as f32).max(1.0);
    let budget = (max_w / per_char).floor() as usize;
    let mut keep = budget.saturating_sub(1).min(chars.len());
    loop {
        let n = keep.min(chars.len());
        if n < 4 {
            return measure("…".to_string());
        }
        // Give the tail the larger share: file names end in what identifies them.
        let head = (n / 3).max(1);
        let tail = n - head;
        let trial = format!(
            "{}…{}",
            chars[..head].iter().collect::<String>(),
            chars[chars.len() - tail..].iter().collect::<String>()
        );
        let galley = measure(trial);
        if galley.size().x <= max_w || n < 6 {
            return galley;
        }
        keep = n - 1;
    }
}

pub fn elide(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let measure = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font.clone(), color);
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

#[cfg(test)]
mod prefs_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn prefs_parse_reads_theme_and_last_dir() {
        let prefs = Prefs::parse("theme=dark\nlast_dir=/home/aub/Videos\n");
        assert_eq!(prefs.theme, Some(ThemeChoice::Dark));
        assert_eq!(prefs.last_dir, Some(PathBuf::from("/home/aub/Videos")));

        let prefs = Prefs::parse("theme=system\n");
        assert_eq!(prefs.theme, Some(ThemeChoice::System));
        assert!(prefs.last_dir.is_none());
    }

    #[test]
    fn prefs_parse_ignores_junk_and_keeps_partial_files_useful() {
        let prefs = Prefs::parse(
            "# comment\nnot a pair\ntheme=nonsense\nlast_dir=\nlast_dir=/tmp/x\n",
        );
        assert_eq!(prefs.theme, None);
        assert_eq!(prefs.last_dir, Some(PathBuf::from("/tmp/x")));

        // An empty file means defaults, not an error.
        let prefs = Prefs::parse("");
        assert!(prefs.theme.is_none());
        assert!(prefs.last_dir.is_none());
        assert_eq!(prefs.mode(), Mode::Light, "white is the default");
    }
}
