use eframe::egui;
use std::sync::Arc;

pub struct Icons {
    /// The "7" badge cut from the project logo (in-app titlebar).
    pub app_mark: egui::TextureHandle,
    pub add: egui::TextureHandle,
    pub extract: egui::TextureHandle,
    pub test: egui::TextureHandle,
    pub copy: egui::TextureHandle,
    pub move_item: egui::TextureHandle,
    pub delete: egui::TextureHandle,
    pub info: egui::TextureHandle,
    pub folder: egui::TextureHandle,
    pub folder_up: egui::TextureHandle,
    pub archive: egui::TextureHandle,
    pub file: egui::TextureHandle,
    pub up: egui::TextureHandle,
    pub refresh: egui::TextureHandle,
    pub window_minimize: egui::TextureHandle,
    pub window_maximize: egui::TextureHandle,
    pub window_restore: egui::TextureHandle,
    pub window_close: egui::TextureHandle,
}

fn load(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let icon = eframe::icon_data::from_png_bytes(bytes).expect("valid icon PNG");
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}

impl Icons {
    pub fn new(ctx: &egui::Context) -> Arc<Self> {
        Arc::new(Self {
            app_mark: load(ctx, "icon_app_mark", include_bytes!("../assets/icons/app_mark.png")),
            add: load(ctx, "icon_add", include_bytes!("../assets/icons/add.png")),
            extract: load(ctx, "icon_extract", include_bytes!("../assets/icons/extract.png")),
            test: load(ctx, "icon_test", include_bytes!("../assets/icons/test.png")),
            copy: load(ctx, "icon_copy", include_bytes!("../assets/icons/copy.png")),
            move_item: load(ctx, "icon_move", include_bytes!("../assets/icons/move.png")),
            delete: load(ctx, "icon_delete", include_bytes!("../assets/icons/delete.png")),
            info: load(ctx, "icon_info", include_bytes!("../assets/icons/info.png")),
            folder: load(ctx, "icon_folder", include_bytes!("../assets/icons/folder16.png")),
            folder_up: load(ctx, "icon_folder_up", include_bytes!("../assets/icons/folder_up16.png")),
            archive: load(ctx, "icon_archive", include_bytes!("../assets/icons/archive16.png")),
            file: load(ctx, "icon_file", include_bytes!("../assets/icons/file16.png")),
            up: load(ctx, "icon_up", include_bytes!("../assets/icons/up.png")),
            refresh: load(ctx, "icon_refresh", include_bytes!("../assets/icons/refresh.png")),
            window_minimize: load(ctx, "icon_win_minimize", include_bytes!("../assets/icons/window_minimize.png")),
            window_maximize: load(ctx, "icon_win_maximize", include_bytes!("../assets/icons/window_maximize.png")),
            window_restore: load(ctx, "icon_win_restore", include_bytes!("../assets/icons/window_restore.png")),
            window_close: load(ctx, "icon_win_close", include_bytes!("../assets/icons/window_close.png")),
        })
    }
}
