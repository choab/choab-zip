//! The interface: one header bar, one command bar, a places sidebar and two
//! list panes. All drawing goes through `design` so the look stays coherent.
//!
//! This module contains only presentation; navigation, archive and file
//! operations live in `main.rs`.

use crate::design::{
    self, chip, chip_width, cmd_button, elide, hairline, icon_button, paint_icon, section_label,
    Icon, Palette, COMMAND_H, HEADER_H, ROW_H, SIDEBAR_W, STATUS_H,
};
use crate::{engine, human, Pane, Place, ChoabZip, SortCol};
use eframe::egui::{
    self, Align2, Color32, FontId, Margin, Rect, Response, Sense, Stroke, Vec2,
};
use std::path::PathBuf;
use std::sync::Arc;

const WIN_BTN_W: f32 = 36.0;
const WIN_BTN_H: f32 = 28.0;

/// Column geometry shared by the header and every row of a pane.
///
/// Columns are dropped from the right as the pane narrows so a squeezed pane
/// never draws overlapping labels.
struct Cols {
    name_l: f32,
    name_r: f32,
    size_r: Option<f32>,
    packed_r: Option<f32>,
    date_r: Option<f32>,
}

impl Cols {
    fn new(rect: Rect, packed: bool) -> Self {
        let w = rect.width();
        let show_date = w > 320.0;
        let show_packed = packed && w > 500.0;
        let show_size = w > 260.0;

        let mut right = rect.right() - 10.0;
        let date_r = if show_date {
            let r = right;
            right -= 92.0;
            Some(r)
        } else {
            None
        };
        let packed_r = if show_packed {
            let r = right - 6.0;
            right = r - 74.0;
            Some(r)
        } else {
            None
        };
        let size_r = if show_size {
            let r = right - 6.0;
            right = r - 66.0;
            Some(r)
        } else {
            None
        };

        Self {
            name_l: rect.left() + 10.0 + 16.0 + 8.0,
            name_r: (right - 6.0).max(rect.left() + 40.0),
            size_r,
            packed_r,
            date_r,
        }
    }
}

impl ChoabZip {
    pub fn palette(&self) -> Palette {
        Palette::new(self.theme)
    }

    pub fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.theme = self.theme.flipped();
        design::Prefs::remember_mode(self.theme);
        design::apply(ctx, &self.palette());
    }

    // ------------------------------------------------------------- header

    fn ui_header(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        let is_maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));

        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;

            let (mark, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
            design::paint_mark(ui.painter(), mark, &p);
            ui.label(
                egui::RichText::new("CHOAB Zip")
                    .size(13.5)
                    .strong()
                    .color(p.text),
            );

            if let Some(arc) = self.archive.clone() {
                let name = arc.file_name().and_then(|s| s.to_str()).unwrap_or("archive");
                let font = FontId::proportional(12.0);
                // Reserve the window buttons (laid out right-to-left later) and
                // the app mark, then fit the name into what is left. Long names
                // keep their tail — the extension is the useful part.
                let tab_max = (ui.available_width() - WIN_BTN_W * 3.0 - 30.0 - 42.0).max(70.0);
                let label = design::elide_left(ui, name, font.clone(), p.text, tab_max);
                let tw = label.size().x;
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(tw + 42.0, 26.0), Sense::click());
                let bg = if resp.hovered() { p.row_hover } else { p.bg_field };
                ui.painter().rect_filled(rect, 6, bg);
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        egui::pos2(rect.left() + 14.0, rect.center().y),
                        Vec2::splat(14.0),
                    ),
                    Icon::Archive,
                    p.ink,
);
                ui.painter().galley(
                    egui::pos2(rect.left() + 25.0, rect.center().y - label.size().y / 2.0),
                    label,
                    p.text,
                );
                if resp
                    .on_hover_text(arc.display().to_string())
                    .clicked()
                {
                    self.focus = Pane::Archive;
                }
            } else {
                ui.label(
                    egui::RichText::new("a quiet home for your archives")
                        .size(11.5)
                        .color(p.text_faint),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;

                if window_button(ui, &p, Icon::Close, true).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                let (icon, tip) = if is_maximized {
                    (Icon::Restore, "Restore")
                } else {
                    (Icon::Maximize, "Maximize")
                };
                if window_button(ui, &p, icon, false)
                    .on_hover_text(tip)
                    .clicked()
                {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                }
                if window_button(ui, &p, Icon::Minimize, false)
                    .on_hover_text("Minimize")
                    .clicked()
                {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            });
        });

        let row = ui.max_rect();
        let drag_rect = Rect::from_min_max(
            row.left_top(),
            egui::pos2((row.right() - WIN_BTN_W * 3.0 - 12.0).max(row.left() + 40.0), row.bottom()),
        );
        self.titlebar_drag(ui.ctx(), drag_rect, is_maximized);
    }

    // -------------------------------------------------------- command bar

    fn ui_commandbar(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        let has_archive = self.archive.is_some();
        let has_dir_sel = !self.dir_sel.is_empty();
        let has_entry_sel = !self.arc_sel.is_empty();

        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;

            if cmd_button(ui, &p, Icon::Add, "Add", true).clicked() {
                let sel_paths = self.selected_dir_paths();
                self.create_sources = if !sel_paths.is_empty() {
                    sel_paths
                } else {
                    vec![self.cwd.clone()]
                };
                self.create_name = "archive.7z".into();
                self.show_create = true;
            }
            if cmd_button(ui, &p, Icon::Extract, "Extract", has_archive).clicked() {
                if let Some(d) = rfd::FileDialog::new().set_directory(&self.cwd).pick_folder() {
                    self.extract_to(d);
                }
            }
            if cmd_button(ui, &p, Icon::ExtractSelected, "Selected", has_archive && has_entry_sel)
                .on_hover_text("Extract only the selected entries")
                .clicked()
            {
                self.extract_selected();
            }
            if cmd_button(ui, &p, Icon::Check, "Test", has_archive).clicked() {
                self.run_verify();
            }
            if cmd_button(ui, &p, Icon::Hash, "Hashes", has_dir_sel)
                .on_hover_text("CRC-32 and SHA-256 of the selected file")
                .clicked()
            {
                if let Some(&idx) = self.dir_sel.iter().next() {
                    if let Some(r) = self.dirs.get(idx) {
                        let path = self.cwd.join(&r.name);
                        self.run_checksum(path);
                    }
                }
            }

            vsep(ui, &p);

            if cmd_button(ui, &p, Icon::FolderPlus, "New Folder", true)
                .on_hover_text("F7")
                .clicked()
            {
                self.new_folder_dialog = Some(crate::NewFolderDialog {
                    name: String::new(),
                    error: None,
                });
            }
            if cmd_button(ui, &p, Icon::Copy, "Copy", has_dir_sel).clicked() {
                let paths = self.selected_dir_paths();
                if let Some(dest_dir) =
                    rfd::FileDialog::new().set_directory(&self.cwd).pick_folder()
                {
                    let mut copied = 0;
                    for src in paths {
                        if let Some(name) = src.file_name() {
                            if std::fs::copy(&src, dest_dir.join(name)).is_ok() {
                                copied += 1;
                            }
                        }
                    }
                    self.status = format!("Copied {copied} item(s) to {}", dest_dir.display());
                    self.refresh_dir();
                }
            }
            if cmd_button(ui, &p, Icon::Move, "Move", has_dir_sel).clicked() {
                let paths = self.selected_dir_paths();
                if let Some(dest_dir) =
                    rfd::FileDialog::new().set_directory(&self.cwd).pick_folder()
                {
                    let mut moved = 0;
                    for src in paths {
                        if let Some(name) = src.file_name() {
                            if std::fs::rename(&src, dest_dir.join(name)).is_ok() {
                                moved += 1;
                            }
                        }
                    }
                    self.status = format!("Moved {moved} item(s) to {}", dest_dir.display());
                    self.refresh_dir();
                }
            }
            if cmd_button(ui, &p, Icon::Delete, "Delete", has_dir_sel).clicked() {
                let targets: Vec<(PathBuf, bool)> = self
                    .dir_sel
                    .iter()
                    .filter_map(|&i| {
                        self.dirs
                            .get(i)
                            .map(|r| (self.cwd.join(&r.name), r.is_dir))
                    })
                    .collect();
                if !targets.is_empty() {
                    self.delete_targets = targets;
                }
            }
            if cmd_button(ui, &p, Icon::Info, "Info", has_dir_sel || has_archive).clicked() {
                self.show_properties();
            }

            vsep(ui, &p);

            if cmd_button(ui, &p, Icon::Up, "Up", true).clicked() {
                match self.focus {
                    Pane::Files => self.go_up(),
                    Pane::Archive => self.archive_go_up(),
                }
            }
            if cmd_button(ui, &p, Icon::Refresh, "Refresh", true).clicked() {
                self.refresh_dir();
                if let Some(a) = self.archive.clone() {
                    self.open_archive(a);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (icon, tip) = match self.theme {
                    design::Mode::Dark => (Icon::Sun, "Light theme (Ctrl+D)"),
                    design::Mode::Light => (Icon::Moon, "Dark theme (Ctrl+D)"),
                };
                if icon_button(ui, &p, icon, 28.0, true, false)
                    .on_hover_text(tip)
                    .clicked()
                {
                    self.toggle_theme(ui.ctx());
                }
                if icon_button(ui, &p, Icon::Sidebar, 28.0, true, self.show_sidebar)
                    .on_hover_text("Toggle sidebar")
                    .clicked()
                {
                    self.show_sidebar = !self.show_sidebar;
                }
            });
        });
    }

    // ------------------------------------------------------------ sidebar

    fn ui_sidebar(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        let mut target: Option<PathBuf> = None;

        egui::ScrollArea::vertical()
            .id_salt("sidebar_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
                ui.add_space(8.0);
                section_label(ui, &p, "Places");
                for place in self.places.iter() {
                    if sidebar_item(ui, &p, place, self.cwd == place.path) {
                        target = Some(place.path.clone());
                    }
                }
                if !self.devices.is_empty() {
                    section_label(ui, &p, "Devices");
                    for place in self.devices.iter() {
                        if sidebar_item(ui, &p, place, self.cwd == place.path) {
                            target = Some(place.path.clone());
                        }
                    }
                }
                ui.add_space(12.0);
            });

        if let Some(path) = target {
            self.enter_dir(path);
            self.focus = Pane::Files;
        }

        // A hairline on the right edge separates the sidebar from the panes.
        let r = ui.max_rect();
        ui.painter().line_segment(
            [r.right_top(), r.right_bottom()],
            Stroke::new(1.0, p.border),
        );
    }

    // -------------------------------------------------------- files pane

    fn ui_files(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();

        // Pane header: home, breadcrumbs / editable path, edit button.
        egui::Frame::NONE
            .inner_margin(Margin::symmetric(10, 0))
            .show(ui, |ui| {
                ui.set_height(34.0);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;

                    if icon_button(ui, &p, Icon::Home, 24.0, true, false)
                        .on_hover_text("Home")
                        .clicked()
                    {
                        self.enter_dir(home_dir());
                    }

                    if self.cwd_editing {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.cwd_edit_text)
                                .desired_width(ui.available_width() - 36.0)
                                .font(FontId::proportional(12.5)),
                        );
                        if (resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                            || icon_button(ui, &p, Icon::Check, 24.0, true, false).clicked()
                        {
                            let path = PathBuf::from(&self.cwd_edit_text);
                            if path.is_dir() {
                                self.enter_dir(path);
                            } else {
                                self.cwd_editing = false;
                            }
                        }
                    } else {
                        let mut targets: Vec<(String, PathBuf)> = Vec::new();
                        let mut accum = PathBuf::new();
                        for comp in self.cwd.components() {
                            accum.push(comp.as_os_str());
                            let label = comp.as_os_str().to_str().unwrap_or("/");
                            targets.push((
                                if label == "/" { "/".to_string() } else { label.to_string() },
                                accum.clone(),
                            ));
                        }
                        let avail = ui.available_width() - 32.0;
                        if let Some(i) = breadcrumbs(ui, &p, &targets, avail) {
                            self.enter_dir(targets[i].1.clone());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, &p, Icon::Pencil, 24.0, true, false)
                                .on_hover_text("Edit path")
                                .clicked()
                            {
                                self.cwd_editing = true;
                                self.cwd_edit_text = self.cwd.display().to_string();
                            }
                        });
                    }
                });
            });
        hairline(ui, &p);
        ui.spacing_mut().item_spacing.y = 0.0;

        if let Some(c) = column_header(ui, &p, false, self.sort_col, self.ascending) {
            self.toggle_sort(c);
        }

        let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        let shift = ui.input(|i| i.modifiers.shift);
        let focused_pane = self.focus == Pane::Files;

        egui::ScrollArea::both()
            .id_salt("files_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;

                if self.dirs.is_empty() {
                    empty_state(ui, &p, Icon::Folder, "This folder is empty", "");
                    return;
                }

                let mut clicked_idx = None;
                let mut dbl_idx = None;
                let mut go_parent = false;

                if self.cwd.parent().is_some() {
                    let r = list_row(
                        ui,
                        &p,
                        Icon::FolderUp,
                        p.text_muted,
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

                enum DirAction {
                    Open(PathBuf, bool),
                    AddArchive(PathBuf),
                    Checksum(PathBuf),
                    Delete(PathBuf, bool),
                    Properties(usize),
                }
                let mut dir_action: Option<DirAction> = None;

                for (i, entry) in self.dirs.iter().enumerate() {
                    let sel = self.dir_sel.contains(&i);
                    let (icon, tint) = if entry.is_dir {
                        (Icon::Folder, p.ink)
                    } else if entry.is_archive {
                        (Icon::Archive, p.ink)
                    } else {
                        (Icon::File, p.text_faint)
                    };
                    let size_text = if entry.is_dir {
                        String::new()
                    } else {
                        human(entry.size)
                    };
                    let r = list_row(
                        ui,
                        &p,
                        icon,
                        tint,
                        &entry.name,
                        &size_text,
                        "",
                        &entry.modified,
                        sel,
                        focused_pane && self.dir_focus == Some(i),
                        false,
                    );

                    let entry_path = self.cwd.join(&entry.name);
                    let entry_is_dir = entry.is_dir;

                    r.clone().context_menu(|ui| {
                        if ui.button("Open").clicked() {
                            dir_action = Some(DirAction::Open(entry_path.clone(), entry_is_dir));
                            ui.close();
                        }
                        if ui.button("Add to Archive…").clicked() {
                            dir_action = Some(DirAction::AddArchive(entry_path.clone()));
                            ui.close();
                        }
                        if !entry_is_dir && ui.button("Calculate Hashes…").clicked() {
                            dir_action = Some(DirAction::Checksum(entry_path.clone()));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Delete").clicked() {
                            dir_action = Some(DirAction::Delete(entry_path.clone(), entry_is_dir));
                            ui.close();
                        }
                        if ui.button("Properties").clicked() {
                            dir_action = Some(DirAction::Properties(i));
                            ui.close();
                        }
                    });

                    if r.clicked() {
                        clicked_idx = Some(i);
                    }
                    if r.double_clicked() {
                        dbl_idx = Some(i);
                    }
                }

                if let Some(act) = dir_action {
                    match act {
                        DirAction::Open(path, is_dir) => {
                            if is_dir {
                                self.enter_dir(path);
                            } else {
                                self.open_path(path);
                            }
                        }
                        DirAction::AddArchive(path) => {
                            self.create_sources = vec![path];
                            self.create_name = "archive.7z".into();
                            self.show_create = true;
                        }
                        DirAction::Checksum(path) => self.run_checksum(path),
                        DirAction::Delete(path, is_dir) => {
                            self.delete_targets = vec![(path, is_dir)]
                        }
                        DirAction::Properties(idx) => {
                            self.dir_sel.clear();
                            self.dir_sel.insert(idx);
                            self.dir_focus = Some(idx);
                            self.show_properties();
                        }
                    }
                }

                if let Some(i) = clicked_idx {
                    self.focus = Pane::Files;
                    if ctrl {
                        if !self.dir_sel.remove(&i) {
                            self.dir_sel.insert(i);
                        }
                        self.dir_focus = Some(i);
                    } else if shift && self.dir_focus.is_some() {
                        let start = self.dir_focus.unwrap().min(i);
                        let end = self.dir_focus.unwrap().max(i);
                        self.dir_sel.clear();
                        for idx in start..=end {
                            self.dir_sel.insert(idx);
                        }
                    } else {
                        self.dir_sel.clear();
                        self.dir_sel.insert(i);
                        self.dir_focus = Some(i);
                    }
                }
                if go_parent {
                    self.go_up();
                }
                if let Some(i) = dbl_idx {
                    if let Some(r) = self.dirs.get(i) {
                        let path = self.cwd.join(&r.name);
                        if r.is_dir {
                            self.enter_dir(path);
                        } else {
                            self.open_path(path);
                        }
                    }
                }
            });

        let r = ui.max_rect();
        ui.painter().line_segment(
            [r.right_top(), r.right_bottom()],
            Stroke::new(1.0, p.border),
        );
    }

    // ------------------------------------------------------ archive pane

    fn ui_archive(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        let Some(arc) = self.archive.clone() else {
            empty_state(
                ui,
                &p,
                Icon::Archive,
                "No archive open",
                "Double-click an archive on the left, or drop one on the window.",
            );
            return;
        };

        let fmt = engine::detect(&arc);
        let arc_name = arc.file_name().and_then(|s| s.to_str()).unwrap_or("archive");

        egui::Frame::NONE
            .inner_margin(Margin::symmetric(10, 0))
            .show(ui, |ui| {
                ui.set_height(34.0);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;

                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            egui::pos2(ui.cursor().left() + 8.0, ui.cursor().center().y),
                            Vec2::splat(16.0),
                        ),
                        Icon::Archive,
                        p.ink,
);
                    ui.add_space(20.0);

                    let mut targets: Vec<(String, String)> = Vec::new();
                    targets.push((arc_name.to_string(), String::new()));
                    if !self.flat_view && !self.inner.is_empty() {
                        let mut accum = String::new();
                        for part in self.inner.trim_end_matches('/').split('/') {
                            accum.push_str(part);
                            accum.push('/');
                            targets.push((part.to_string(), accum.clone()));
                        }
                    }
                    let avail = ui.available_width() - 150.0;
                    if let Some(i) = archive_crumbs(ui, &p, &targets, avail) {
                        if i == 0 {
                            self.inner.clear();
                            self.flat_view = false;
                            self.arc_sel.clear();
                            self.arc_focus = None;
                        } else {
                            self.inner = targets[i].1.clone();
                            self.arc_sel.clear();
                            self.arc_focus = None;
                        }
                    }

                    ui.add_space(4.0);
                    let w = chip_width(ui, fmt.label());
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 16.0), Sense::hover());
                    chip(ui.painter(), rect, &p, fmt.label());

                    let room = ui.available_width();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if icon_button(ui, &p, Icon::Close, 26.0, true, false)
                            .on_hover_text("Close archive")
                            .clicked()
                        {
                            self.close_archive();
                        }
                        if icon_button(ui, &p, Icon::Rows, 26.0, true, self.flat_view)
                            .on_hover_text("Flat list of every entry")
                            .clicked()
                        {
                            self.flat_view = !self.flat_view;
                            self.arc_sel.clear();
                            self.arc_focus = None;
                        }
                        if !self.flat_view && room > 240.0 {
                            let changed = search_box(ui, &p, &mut self.filter, 130.0, "Filter");
                            if changed {
                                self.arc_sel.clear();
                                self.arc_focus = None;
                                self.dir_sel.clear();
                                self.dir_focus = None;
                            }
                        }
                    });
                });
            });
        hairline(ui, &p);
        ui.spacing_mut().item_spacing.y = 0.0;

        if let Some(c) = column_header(ui, &p, true, self.sort_col, self.ascending) {
            self.toggle_sort(c);
        }

        let rows = self.visible_rows();
        let focused_pane = self.focus == Pane::Archive;
        let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        let shift = ui.input(|i| i.modifiers.shift);

        egui::ScrollArea::both()
            .id_salt("archive_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;

                if rows.is_empty() {
                    empty_state(
                        ui,
                        &p,
                        Icon::Folder,
                        if self.filter.is_empty() { "Nothing here" } else { "No matches" },
                        if self.filter.is_empty() {
                            "This folder inside the archive is empty."
                        } else {
                            "No entry matches the filter."
                        },
                    );
                    return;
                }

                let mut clicked_idx = None;
                let mut dbl_idx = None;
                let mut go_parent = false;

                if !self.flat_view && !self.inner.is_empty() {
                    let r = list_row(
                        ui,
                        &p,
                        Icon::FolderUp,
                        p.text_muted,
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

                for (i, row_item) in rows.iter().enumerate() {
                    let sel = self.arc_sel.contains(&i);
                    let (icon, tint) = if row_item.is_dir {
                        (Icon::Folder, p.ink)
                    } else {
                        (Icon::File, p.text_faint)
                    };
                    let size_text = if row_item.is_dir {
                        String::new()
                    } else {
                        human(row_item.size)
                    };
                    let packed_text = if row_item.is_dir || row_item.packed == 0 {
                        String::new()
                    } else {
                        human(row_item.packed)
                    };
                    let r = list_row(
                        ui,
                        &p,
                        icon,
                        tint,
                        &row_item.name,
                        &size_text,
                        &packed_text,
                        &row_item.modified,
                        sel,
                        focused_pane && self.arc_focus == Some(i),
                        true,
                    );

                    let entry_full_path = if self.flat_view || self.inner.is_empty() {
                        row_item.name.clone()
                    } else {
                        format!("{}{}", self.inner, row_item.name)
                    };

                    r.clone().context_menu(|ui| {
                        if ui.button("Open / View").clicked() {
                            self.enter_archive_item(&rows, i);
                            ui.close();
                        }
                        if ui.button("Extract Selected…").clicked() {
                            self.arc_sel.clear();
                            self.arc_sel.insert(i);
                            self.arc_focus = Some(i);
                            self.extract_selected();
                            ui.close();
                        }
                        if ui.button("Test Integrity").clicked() {
                            self.run_verify();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Copy Path").clicked() {
                            ui.ctx().copy_text(entry_full_path.clone());
                            ui.close();
                        }
                    });

                    if r.clicked() {
                        clicked_idx = Some(i);
                    }
                    if r.double_clicked() {
                        dbl_idx = Some(i);
                    }
                }

                if let Some(i) = clicked_idx {
                    self.focus = Pane::Archive;
                    if ctrl {
                        if !self.arc_sel.remove(&i) {
                            self.arc_sel.insert(i);
                        }
                        self.arc_focus = Some(i);
                    } else if shift && self.arc_focus.is_some() {
                        let start = self.arc_focus.unwrap().min(i);
                        let end = self.arc_focus.unwrap().max(i);
                        self.arc_sel.clear();
                        for idx in start..=end {
                            self.arc_sel.insert(idx);
                        }
                    } else {
                        self.arc_sel.clear();
                        self.arc_sel.insert(i);
                        self.arc_focus = Some(i);
                    }
                }
                if go_parent {
                    self.archive_go_up();
                }
                if let Some(i) = dbl_idx {
                    self.enter_archive_item(&rows, i);
                }
            });
    }

    // ------------------------------------------------------------ viewer

    fn ui_preview(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        let Some(preview) = &mut self.preview else {
            return;
        };

        let mut close = false;
        let mut open_externally = false;
        let key_for_external = preview.key.clone();
        let name_for_external = preview.name.clone();

        // The viewer is a dock across the bottom: it never squeezes the panes.
        let panel = egui::Panel::bottom("preview_panel")
            .resizable(true)
            .default_size(300.0)
            .min_size(140.0)
            .frame(
                egui::Frame::NONE
                    .fill(p.bg_content)
                    .inner_margin(Margin::symmetric(14, 10)),
            )
            .show(ui, |ui| {
                // One compact row: icon, name, meta, then the actions.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            egui::pos2(ui.cursor().left() + 8.0, ui.cursor().center().y),
                            Vec2::splat(16.0),
                        ),
                        if preview.image_texture.is_some() {
                            Icon::Pictures
                        } else {
                            Icon::File
                        },
                        p.ink,
);
                    ui.add_space(20.0);

                    let title = elide(
                        ui,
                        &preview.name,
                        FontId::proportional(13.0),
                        p.text,
                        (ui.available_width() - 400.0).max(80.0),
                    );
                    let title_w = title.size().x;
                    ui.painter().galley(
                        egui::pos2(
                            ui.cursor().left(),
                            ui.cursor().center().y - title.size().y / 2.0,
                        ),
                        title,
                        p.text,
                    );
                    ui.add_space(title_w + 12.0);

                    let mut meta: Vec<String> = Vec::new();
                    if let Some(dim) = preview.image_dim {
                        meta.push(format!("{} × {} px", dim[0], dim[1]));
                    }
                    meta.push(human(preview.size));
                    if let Some(m) = &preview.method {
                        meta.push(m.clone());
                    }
                    ui.label(
                        egui::RichText::new(meta.join("  ·  "))
                            .size(11.0)
                            .color(p.text_faint),
                    );

                    let has_text = preview.text.is_some();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if icon_button(ui, &p, Icon::Close, 24.0, true, false)
                            .on_hover_text("Close viewer")
                            .clicked()
                        {
                            close = true;
                        }
                        if has_text {
                            search_box(ui, &p, &mut preview.search_query, 96.0, "Find");
                            if icon_button(ui, &p, Icon::External, 24.0, true, false)
                                .on_hover_text("Open with the system app")
                                .clicked()
                            {
                                open_externally = true;
                            }
                            if icon_button(ui, &p, Icon::Copy, 24.0, true, false)
                                .on_hover_text("Copy all text")
                                .clicked()
                            {
                                if let Some(text) = &preview.text {
                                    ui.ctx().copy_text(text.clone());
                                }
                            }
                            if icon_button(ui, &p, Icon::Rows, 24.0, true, preview.wrap_text)
                                .on_hover_text("Wrap long lines")
                                .clicked()
                            {
                                preview.wrap_text = !preview.wrap_text;
                            }
                        } else if icon_button(ui, &p, Icon::External, 24.0, true, false)
                            .on_hover_text("Open with the system app")
                            .clicked()
                        {
                            open_externally = true;
                        }
                    });
                });

                hairline(ui, &p);

                if preview.loading {
                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new("Decoding…")
                                .size(12.0)
                                .color(p.text_muted),
                        );
                    });
                    return;
                }

                if let Some(e) = &preview.error {
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new(e).size(12.0).color(p.danger));
                    return;
                }

                if let Some(tex) = &preview.image_texture {
                    egui::ScrollArea::both()
                        .id_salt("preview_image_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let max_w = ui.available_width().max(80.0);
                            ui.add(egui::Image::new(tex).max_width(max_w));
                        });
                    return;
                }

                if let Some(text) = &preview.text {
                    let query = preview.search_query.to_lowercase();
                    egui::ScrollArea::both()
                        .id_salt("preview_text_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            if preview.wrap_text {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(text).monospace().size(12.0),
                                    )
                                    .wrap()
                                    .selectable(true),
                                );
                            } else {
                                for (li, line) in text.lines().enumerate() {
                                    let hit =
                                        !query.is_empty() && line.to_lowercase().contains(&query);
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        ui.label(
                                            egui::RichText::new(format!("{:>4}", li + 1))
                                                .monospace()
                                                .size(10.5)
                                                .color(p.text_faint),
                                        );
                                        let mut rt =
                                            egui::RichText::new(line).monospace().size(12.0);
                                        if hit {
                                            rt = rt
                                                .background_color(design::with_alpha(p.text_muted, 45))
                                                .color(p.text);
                                        }
                                        ui.label(rt);
                                    });
                                }
                            }
                        });
                }
            });

        let panel_rect = panel.response.rect;
        ui.painter().line_segment(
            [panel_rect.left_top(), panel_rect.right_top()],
            Stroke::new(1.0, p.border),
        );

        if open_externally {
            let preview_path = PathBuf::from(&key_for_external);
            if preview_path.exists() {
                self.open_system(&preview_path);
            } else if let Some(arc) = self.archive.clone() {
                let tx = self.tx.clone();
                let password = self.cached_passwords.get(&arc).cloned();
                std::thread::spawn(move || {
                    let result = crate::extract_entry_to_scratch(
                        &arc,
                        &key_for_external,
                        password.as_deref(),
                        "opened",
                    )
                    .and_then(|path| crate::system_open(&path).map(|()| path));
                    let _ = tx.send(crate::Msg::External {
                        key: key_for_external,
                        name: name_for_external,
                        result,
                    });
                });
            }
        }

        if close {
            self.preview = None;
        }
    }

    // ------------------------------------------------------------ status

    fn ui_status(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_space(10.0);
            if self.busy {
                ui.spinner();
            }
            // Progress rides along with the status text, so a long pass is
            // visible instead of the bar looking frozen.
            let mut text = self.status.clone();
            if let Some((_, detail)) = self.progress.as_ref().filter(|_| self.busy) {
                if !detail.is_empty() {
                    text.push_str("  ·  ");
                    text.push_str(detail);
                }
            }
            let status = design::elide_middle(
                ui,
                &text,
                FontId::proportional(11.5),
                p.text_muted,
                ui.available_width() - 240.0,
            );
            ui.painter().galley(
                egui::pos2(ui.cursor().left(), ui.cursor().center().y - status.size().y / 2.0),
                status,
                p.text_muted,
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space(10.0);

                // Thin determinate bar along the bottom edge: greyscale, like
                // everything else, and it never competes with the text.
                if let Some((frac, _)) = self.progress.as_ref().filter(|_| self.busy) {
                    let frac = *frac;
                    let rect = ui.max_rect();
                    let track = Rect::from_min_max(
                        egui::pos2(rect.left(), rect.bottom() - 2.0),
                        egui::pos2(rect.right(), rect.bottom()),
                    );
                    ui.painter().rect_filled(track, 0, p.border);
                    let width = rect.width() * frac.clamp(0.0, 1.0);
                    if width > 0.5 {
                        ui.painter().rect_filled(
                            Rect::from_min_max(
                                track.min,
                                egui::pos2(track.left() + width, track.bottom()),
                            ),
                            0,
                            p.ink,
                        );
                    }
                }

                let pane = match self.focus {
                    Pane::Files => "files",
                    Pane::Archive => "archive",
                };
                let (count, sel_text) = match self.focus {
                    Pane::Files => {
                        let mut sel = String::new();
                        if self.dir_sel.len() == 1 {
                            if let Some(&i) = self.dir_sel.iter().next() {
                                if let Some(r) = self.dirs.get(i) {
                                    sel = if r.is_dir {
                                        r.name.clone()
                                    } else {
                                        format!("{} · {}", r.name, human(r.size))
                                    };
                                }
                            }
                        } else if self.dir_sel.len() > 1 {
                            let bytes: u64 = self
                                .dir_sel
                                .iter()
                                .filter_map(|&i| self.dirs.get(i))
                                .map(|r| r.size)
                                .sum();
                            sel = format!("{} selected · {}", self.dir_sel.len(), human(bytes));
                        }
                        (self.dirs.len(), sel)
                    }
                    Pane::Archive => {
                        let rows = self.visible_rows();
                        let mut sel = String::new();
                        if self.arc_sel.len() == 1 {
                            if let Some(&i) = self.arc_sel.iter().next() {
                                if let Some(r) = rows.get(i) {
                                    sel = if r.is_dir {
                                        r.name.clone()
                                    } else {
                                        format!("{} · {}", r.name, human(r.size))
                                    };
                                }
                            }
                        } else if self.arc_sel.len() > 1 {
                            let bytes: u64 = self
                                .arc_sel
                                .iter()
                                .filter_map(|&i| rows.get(i))
                                .map(|r| r.size)
                                .sum();
                            sel = format!("{} selected · {}", self.arc_sel.len(), human(bytes));
                        }
                        (rows.len(), sel)
                    }
                };

                status_seg(ui, &p, pane);
                dot(ui, &p);
                status_seg(ui, &p, &format!("{count} items"));
                if !sel_text.is_empty() {
                    dot(ui, &p);
                    status_seg(ui, &p, &sel_text);
                }
            });
        });
    }
}

// ----------------------------------------------------------------- pieces

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

fn vsep(ui: &mut egui::Ui, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(13.0, 20.0), Sense::hover());
    ui.painter().line_segment(
        [
            egui::pos2(rect.center().x, rect.top() + 2.0),
            egui::pos2(rect.center().x, rect.bottom() - 2.0),
        ],
        Stroke::new(1.0, p.border),
    );
}

fn dot(ui: &mut egui::Ui, p: &Palette) {
    ui.label(egui::RichText::new("·").size(11.0).color(p.text_faint));
}

fn status_seg(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).color(p.text_muted));
}

fn window_button(ui: &mut egui::Ui, p: &Palette, icon: Icon, danger: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(WIN_BTN_W, WIN_BTN_H), Sense::click());
    let hovered = resp.hovered();
    if hovered {
        let fill = if danger { p.danger } else { p.row_hover };
        ui.painter().rect_filled(rect, 6, fill);
    }
    let color = if hovered && danger {
        Color32::WHITE
    } else if hovered {
        p.text
    } else {
        p.text_muted
    };
    paint_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(14.0)),
        icon,
        color,
);
    resp
}

fn sidebar_item(ui: &mut egui::Ui, p: &Palette, place: &Place, active: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - 16.0, 28.0),
        Sense::click(),
    );
    let rect = rect.translate(Vec2::new(8.0, 0.0));
    let hovered = resp.hovered();
    if active {
        ui.painter().rect_filled(rect, 6, p.row_selected);
    } else if hovered {
        ui.painter().rect_filled(rect, 6, p.row_hover);
    }
    let tint = if active { p.ink } else { p.text_muted };
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 20.0, rect.center().y),
            Vec2::splat(15.0),
        ),
        place.icon,
        tint,
);
    let text = elide(
        ui,
        &place.label,
        FontId::proportional(12.5),
        p.text,
        rect.width() - 48.0,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 34.0, rect.center().y - text.size().y / 2.0),
        text,
        if active { p.text } else { p.text_muted },
    );
    resp.on_hover_text(place.path.display().to_string()).clicked()
}

/// Breadcrumb trail; returns the index of the clicked crumb.
///
/// Crumb widths are measured up front, then drawn against a shrinking budget so
/// a long directory name can never push the trail over its allotted space and
/// collide with the widgets to its right. A crumb that has to shrink keeps its
/// tail, which is the part that identifies the folder.
fn breadcrumbs(
    ui: &mut egui::Ui,
    p: &Palette,
    targets: &[(String, PathBuf)],
    max_w: f32,
) -> Option<usize> {
    if targets.is_empty() {
        return None;
    }
    let font = FontId::proportional(12.5);
    let small = FontId::proportional(12.5);

    let widths: Vec<f32> = targets
        .iter()
        .map(|(label, _)| {
            ui.painter()
                .layout_no_wrap(label.clone(), font.clone(), p.text)
                .size()
                .x
                + 12.0
        })
        .collect();

    // Which crumbs fit, counted from the right? Always keep the last one.
    let mut start = targets.len() - 1;
    let mut used = widths[targets.len() - 1];
    while start > 0 && used + widths[start - 1] + 14.0 <= max_w {
        start -= 1;
        used += widths[start] + 14.0;
    }

    let mut budget = max_w;
    let mut clicked = None;
    if start > 1 {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 22.0), Sense::hover());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "…",
            small.clone(),
            p.text_faint,
        );
        chevron(ui, p, 8.0);
        budget -= 26.0;
    }

    for (i, (label, _)) in targets.iter().enumerate().skip(start) {
        let last = i + 1 == targets.len();
        let chev_space = if last { 0.0 } else { 10.0 };
        // Shrink the crumb to what is left instead of overflowing.
        let w = widths[i].min((budget - chev_space).max(24.0));
        let galley = design::elide_left(
            ui,
            label,
            if last { font.clone() } else { small.clone() },
            if last { p.text } else { p.text_muted },
            (w - 12.0).max(8.0),
        );
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, 5, p.row_hover);
        }
        ui.painter().galley(
            egui::pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0),
            galley,
            p.text,
        );
        if resp.clicked() {
            clicked = Some(i);
        }
        budget -= w;
        if !last {
            chevron(ui, p, chev_space);
            budget -= chev_space;
        }
    }
    clicked
}

/// Archive breadcrumb: first crumb is the file name, the rest are inner folders.
///
/// The archive file name always stays visible — it is what tells you which
/// archive you are looking at — and every crumb is clamped to the budget so a
/// long name can never run over the format chip or the filter box.
fn archive_crumbs(
    ui: &mut egui::Ui,
    p: &Palette,
    targets: &[(String, String)],
    max_w: f32,
) -> Option<usize> {
    let font = FontId::proportional(12.5);
    let measure = |ui: &egui::Ui, s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_string(), font.clone(), p.text)
            .size()
            .x
            + 12.0
    };
    if targets.is_empty() || max_w < 40.0 {
        return None;
    }
    let n = targets.len();
    let mut budget = max_w;
    let mut clicked = None;

    let draw_crumb = |ui: &mut egui::Ui,
                      clicked: &mut Option<usize>,
                      i: usize,
                      w: f32,
                      budget: &mut f32| {
        let label = &targets[i].0;
        let galley = design::elide_left(ui, label, font.clone(), p.text, (w - 12.0).max(8.0));
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, 5, p.row_hover);
        }
        ui.painter().galley(
            egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            p.text,
        );
        if resp.clicked() {
            *clicked = Some(i);
        }
        *budget -= w;
    };

    // The archive name itself: capped to less than half the row so the current
    // folder stays readable next to it.
    let first_cap = ((max_w * 0.45).max(56.0)).min(max_w - 24.0);
    let first_w = measure(ui, &targets[0].0).min(first_cap);
    draw_crumb(ui, &mut clicked, 0, first_w, &mut budget);

    if n == 1 {
        return clicked;
    }

    // How many inner crumbs fit after the name?
    let mut start = n - 1;
    let mut acc = measure(ui, &targets[n - 1].0);
    while start > 1 {
        let w = measure(ui, &targets[start - 1].0) + 10.0;
        if acc + w + 26.0 > budget {
            break;
        }
        acc += w;
        start -= 1;
    }

    chevron(ui, p, 10.0);
    budget -= 10.0;
    if start > 1 {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 22.0), Sense::hover());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "…",
            font.clone(),
            p.text_faint,
        );
        chevron(ui, p, 8.0);
        budget -= 26.0;
    }

    for i in start..n {
        let last = i + 1 == n;
        let chev_space = if last { 0.0 } else { 10.0 };
        let w = measure(ui, &targets[i].0).min((budget - chev_space).max(24.0));
        draw_crumb(ui, &mut clicked, i, w, &mut budget);
        if !last {
            chevron(ui, p, chev_space);
            budget -= chev_space;
        }
    }
    clicked
}

fn chevron(ui: &mut egui::Ui, p: &Palette, w: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::hover());
    paint_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(10.0)),
        Icon::ChevronRight,
        p.text_faint,
);
}

fn column_header(
    ui: &mut egui::Ui,
    p: &Palette,
    packed: bool,
    current: SortCol,
    ascending: bool,
) -> Option<SortCol> {
    let w = ui.available_width().max(220.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
    let cols = Cols::new(rect, packed);
    let font = FontId::proportional(11.5);

    let painter = ui.painter();
    let cy = rect.center().y;

    let label = |x: f32, align: Align2, text: &str, col: SortCol| {
        let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), p.text_faint);
        let w = galley.size().x;
        let pos = align.anchor_size(egui::pos2(x, cy), galley.size()).min;
        painter.galley(pos, galley, p.text_faint);
        if current == col {
            let cx = match align {
                Align2::LEFT_CENTER => x + w + 6.0,
                _ => x - w - 6.0,
            };
            let shape = if ascending {
                vec![
                    egui::pos2(cx - 3.5, cy + 2.0),
                    egui::pos2(cx + 3.5, cy + 2.0),
                    egui::pos2(cx, cy - 2.5),
                ]
            } else {
                vec![
                    egui::pos2(cx - 3.5, cy - 2.0),
                    egui::pos2(cx + 3.5, cy - 2.0),
                    egui::pos2(cx, cy + 2.5),
                ]
            };
            painter.add(egui::Shape::convex_polygon(shape, p.text_muted, Stroke::NONE));
        }
    };

    label(cols.name_l, Align2::LEFT_CENTER, "Name", SortCol::Name);
    if let Some(r) = cols.size_r {
        label(r, Align2::RIGHT_CENTER, "Size", SortCol::Size);
    }
    if let Some(r) = cols.packed_r {
        label(r, Align2::RIGHT_CENTER, "Packed", SortCol::Packed);
    }
    if let Some(r) = cols.date_r {
        label(r, Align2::RIGHT_CENTER, "Modified", SortCol::Modified);
    }
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, p.border),
    );

    if !resp.clicked() {
        return None;
    }
    let pos = resp.interact_pointer_pos()?;
    let size_l = cols.size_r.map(|r| r - 66.0).unwrap_or(f32::INFINITY);
    let packed_l = cols.packed_r.map(|r| r - 74.0).unwrap_or(f32::INFINITY);
    let date_l = cols.date_r.map(|r| r - 92.0).unwrap_or(f32::INFINITY);
    if pos.x < size_l {
        Some(SortCol::Name)
    } else if pos.x < packed_l {
        Some(SortCol::Size)
    } else if pos.x < date_l {
        Some(SortCol::Packed)
    } else {
        Some(SortCol::Modified)
    }
}

#[allow(clippy::too_many_arguments)]
fn list_row(
    ui: &mut egui::Ui,
    p: &Palette,
    icon: Icon,
    icon_tint: Color32,
    name: &str,
    size: &str,
    packed: &str,
    date: &str,
    selected: bool,
    focused: bool,
    show_packed: bool,
) -> Response {
    let w = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, ROW_H), Sense::click());
    let cols = Cols::new(rect, show_packed);
    let body = rect.shrink2(Vec2::new(4.0, 1.0));

    let painter = ui.painter();
    if selected {
        painter.rect_filled(body, 6, p.row_selected);
        if focused {
            painter.rect_stroke(
                body,
                6,
                Stroke::new(1.0, p.row_focus),
                egui::StrokeKind::Inside,
            );
        }
    } else if resp.hovered() {
        painter.rect_filled(body, 6, p.row_hover);
    }

    paint_icon(
        painter,
        Rect::from_center_size(
            egui::pos2(rect.left() + 10.0 + 8.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        icon,
        icon_tint,
);

    let name_font = FontId::proportional(13.0);
    let num_font = FontId::proportional(11.5);
    let cy = rect.center().y;

    // Keep both ends of a long name so the extension stays readable.
    let galley = design::elide_middle(ui, name, name_font, p.text, cols.name_r - cols.name_l);
    let painter = ui.painter();
    painter.galley(
        egui::pos2(cols.name_l, cy - galley.size().y / 2.0),
        galley,
        p.text,
    );

    if let Some(r) = cols.size_r {
        if !size.is_empty() {
            painter.text(
                egui::pos2(r, cy),
                Align2::RIGHT_CENTER,
                size,
                num_font.clone(),
                p.text_muted,
            );
        }
    }
    if let Some(pr) = cols.packed_r {
        if !packed.is_empty() {
            painter.text(
                egui::pos2(pr, cy),
                Align2::RIGHT_CENTER,
                packed,
                num_font,
                p.text_muted,
            );
        }
    }
    if let Some(r) = cols.date_r {
        if !date.is_empty() {
            painter.text(
                egui::pos2(r, cy),
                Align2::RIGHT_CENTER,
                date,
                FontId::proportional(11.0),
                p.text_faint,
            );
        }
    }
    resp
}

fn empty_state(ui: &mut egui::Ui, p: &Palette, icon: Icon, title: &str, hint: &str) {
    let rect = ui.max_rect();
    let c = rect.center() - Vec2::new(0.0, 24.0);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(c, Vec2::splat(44.0)),
        icon,
        p.text_faint,
);
    ui.painter().text(
        c + Vec2::new(0.0, 40.0),
        Align2::CENTER_CENTER,
        title,
        FontId::proportional(13.5),
        p.text_muted,
    );
    if !hint.is_empty() {
        ui.painter().text(
            c + Vec2::new(0.0, 60.0),
            Align2::CENTER_CENTER,
            hint,
            FontId::proportional(11.5),
            p.text_faint,
        );
    }
}

/// Rounded search field; returns true when the text changed.
fn search_box(
    ui: &mut egui::Ui,
    p: &Palette,
    text: &mut String,
    width: f32,
    hint: &str,
) -> bool {
    let mut changed = false;
    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(p.bg_field)
        .corner_radius(7)
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        egui::pos2(ui.cursor().left() + 7.0, ui.cursor().center().y),
                        Vec2::splat(13.0),
                    ),
                    Icon::Search,
                    p.text_faint,
);
                ui.add_space(17.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(text)
                        .frame(egui::Frame::NONE)
                        .desired_width(width)
                        .font(FontId::proportional(12.0))
                        .hint_text(hint),
                );
                changed = resp.changed();
                if !text.is_empty()
                    && icon_button(ui, p, Icon::Close, 18.0, true, false)
                        .on_hover_text("Clear")
                        .clicked()
                {
                    text.clear();
                    changed = true;
                }
            });
        });
    changed
}

// ------------------------------------------------------------- app glue

impl eframe::App for ChoabZip {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        if self.busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dropped_paths: Vec<PathBuf> = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|d| d.path().to_path_buf())
                .collect()
        });
        if !dropped_paths.is_empty() {
            if dropped_paths.len() == 1 && crate::looks_like_archive(&dropped_paths[0]) {
                self.open_archive(dropped_paths[0].clone());
            } else if dropped_paths.len() == 1 && dropped_paths[0].is_dir() {
                self.enter_dir(dropped_paths[0].clone());
            } else {
                self.create_sources = dropped_paths.clone();
                let stem = dropped_paths[0]
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("archive");
                self.create_name = format!("{stem}.7z");
                self.show_create = true;
            }
        }

        let is_maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        let shadow_margin: f32 = if is_maximized { 0.0 } else { 10.0 };
        let radius: u8 = if is_maximized { 0 } else { 12 };

        let outer = ui.available_rect_before_wrap();
        let app_rect = outer.shrink(shadow_margin);
        let p = self.palette();

        if !is_maximized {
            ui.painter().add(p.shadow.as_shape(app_rect, radius));
            handle_window_resize(ui, app_rect, &mut self.resizing);
        }
        ui.painter().rect_filled(app_rect, radius, p.bg_content);

        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(app_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );

        // Keyboard navigation and type-to-find, unless a text field has focus.
        // With a single pane the keyboard always talks to what is visible.
        self.focus = if self.archive.is_some() {
            Pane::Archive
        } else {
            Pane::Files
        };
        let rows = self.visible_rows();
        let names: Vec<String> = match self.focus {
            Pane::Files => self.dirs.iter().map(|r| r.name.clone()).collect(),
            Pane::Archive => rows.iter().map(|r| r.name.clone()).collect(),
        };
        if !child.ctx().egui_wants_keyboard_input() {
            self.typeahead(&mut child, &names);
            self.handle_keys(&mut child, &rows);
        }

        let top_corners = egui::CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        };
        let bottom_corners = egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius,
            se: radius,
        };

        egui::Panel::top("window_header")
            .exact_size(HEADER_H)
            .frame(
                egui::Frame::NONE
                    .fill(p.bg_header)
                    .corner_radius(top_corners)
                    .inner_margin(Margin::symmetric(12, 0)),
            )
            .show(&mut child, |ui| self.ui_header(ui));

        egui::Panel::top("command_bar")
            .exact_size(COMMAND_H)
            .frame(
                egui::Frame::NONE
                    .fill(p.bg_command)
                    .inner_margin(Margin::symmetric(10, 0)),
            )
            .show(&mut child, |ui| self.ui_commandbar(ui));
        hairline(&mut child, &p);

        egui::Panel::bottom("status_bar")
            .exact_size(STATUS_H)
            .frame(
                egui::Frame::NONE
                    .fill(p.bg_status)
                    .corner_radius(bottom_corners),
            )
            .show(&mut child, |ui| self.ui_status(ui));

        self.ui_preview(&mut child);

        if self.show_sidebar {
            egui::Panel::left("sidebar")
                .exact_size(SIDEBAR_W)
                .frame(egui::Frame::NONE.fill(p.bg_sidebar))
                .show(&mut child, |ui| self.ui_sidebar(ui));
        }

        // One content pane. The filesystem and the open archive share it: an
        // archive replaces the file list instead of squeezing it into half a
        // window, and Ctrl+W brings the filesystem back.
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.bg_content))
            .show(&mut child, |ui| {
                if self.archive.is_some() {
                    self.ui_archive(ui);
                } else {
                    self.ui_files(ui);
                }
            });

        self.ui_dialogs(child.ctx());

        child.painter().rect_stroke(
            app_rect,
            radius,
            Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
    }
}

// ------------------------------------------------------- window plumbing

impl ChoabZip {
    /// Own implementation of "drag the window by its titlebar".
    ///
    /// Started only after the pointer actually travels, so a double-click (which
    /// toggles maximize) never degenerates into a pointer-grabbing move.
    fn titlebar_drag(
        &mut self,
        ctx: &egui::Context,
        drag_rect: Rect,
        is_maximized: bool,
    ) {
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

        if primary_released {
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
    ctx.request_repaint();
}

fn handle_window_resize(ui: &mut egui::Ui, app_rect: Rect, resizing: &mut bool) {
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
        (true, false, true, false) => (
            Some(egui::ResizeDirection::NorthWest),
            egui::CursorIcon::ResizeNorthWest,
        ),
        (false, true, true, false) => (
            Some(egui::ResizeDirection::NorthEast),
            egui::CursorIcon::ResizeNorthEast,
        ),
        (true, false, false, true) => (
            Some(egui::ResizeDirection::SouthWest),
            egui::CursorIcon::ResizeSouthWest,
        ),
        (false, true, false, true) => (
            Some(egui::ResizeDirection::SouthEast),
            egui::CursorIcon::ResizeSouthEast,
        ),
        (true, false, false, false) => {
            (Some(egui::ResizeDirection::West), egui::CursorIcon::ResizeWest)
        }
        (false, true, false, false) => {
            (Some(egui::ResizeDirection::East), egui::CursorIcon::ResizeEast)
        }
        (false, false, true, false) => {
            (Some(egui::ResizeDirection::North), egui::CursorIcon::ResizeNorth)
        }
        (false, false, false, true) => {
            (Some(egui::ResizeDirection::South), egui::CursorIcon::ResizeSouth)
        }
        _ => (None, egui::CursorIcon::Default),
    };

    if let Some(dir) = dir {
        ui.ctx().set_cursor_icon(cursor);
        if !*resizing && ui.input(|i| i.pointer.primary_pressed()) {
            *resizing = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
            release_pointer_to_compositor(ui.ctx());
        }
    }
}

/// Keep unused imports of the design module honest while the app is built
/// without every helper (they are used by the dialogs on some paths).
#[allow(dead_code)]
fn _design_keepalive(p: &Palette) -> Color32 {
    p.border
}

#[allow(dead_code)]
fn _arc(p: Arc<Palette>) -> Arc<Palette> {
    p
}
