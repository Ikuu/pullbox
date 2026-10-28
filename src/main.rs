mod anilist;
mod app;
mod archive;
mod comicinfo;
mod comicvine;
mod convert;
mod icons;
mod jobs;
mod library;
mod settings;
mod theme;
mod thumbnails;
mod ui;
mod util;

use app::PullboxApp;

/// Desktop application ID; see `packaging/pullbox.desktop`.
const APP_ID: &str = "pullbox";

fn main() -> eframe::Result {
    util::migrate_old_dirs();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Pullbox")
            // Must match the .desktop file name so the desktop shows the right icon.
            .with_app_id(APP_ID)
            .with_inner_size([1400.0, 850.0])
            .with_min_inner_size([800.0, 500.0]),
        // Window size and UI layout (panel widths, column widths, expanded
        // folders) are saved here on exit and restored on the next launch.
        persistence_path: util::project_dirs().map(|dirs| dirs.data_dir().join("window.ron")),
        ..Default::default()
    };

    eframe::run_native(
        "Pullbox",
        options,
        Box::new(|cc| {
            // Lets egui::Image load ComicVine and AniList cover URLs.
            egui_extras::install_image_loaders(&cc.egui_ctx);
            theme::init(&cc.egui_ctx);
            let mut app = PullboxApp::new(library::default_db_path(), &cc.egui_ctx)?;
            // Pick up anything that changed on disk while the app was closed.
            app.start_scan(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}
