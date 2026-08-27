mod app;
mod backend;
mod layout;
mod model;

use app::KvikkApp;
use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    let startup_path = std::env::args_os().nth(1).map(PathBuf::from);
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo.png"))
        .expect("assets/logo.png must be a valid PNG");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("kvikk pdf")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([720.0, 480.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "kvikk pdf",
        options,
        Box::new(move |cc| Ok(Box::new(KvikkApp::new(cc, startup_path)))),
    )
}
