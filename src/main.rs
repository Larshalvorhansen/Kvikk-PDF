mod app;
mod backend;
mod layout;
#[cfg(target_os = "macos")]
mod macos;
mod model;
mod platform;

#[cfg(not(target_os = "macos"))]
use app::KvikkApp;
use eframe::egui;
use std::path::PathBuf;

fn native_options() -> eframe::NativeOptions {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo.png"))
        .expect("assets/logo.png must be a valid PNG");

    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("kvikk pdf")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([720.0, 480.0])
            .with_icon(icon),
        ..Default::default()
    }
}

#[cfg(not(target_os = "macos"))]
fn main() -> eframe::Result {
    let startup_path = std::env::args_os().nth(1).map(PathBuf::from);
    eframe::run_native(
        "kvikk pdf",
        native_options(),
        Box::new(move |cc| Ok(Box::new(KvikkApp::new(cc, startup_path)))),
    )
}

#[cfg(target_os = "macos")]
fn main() -> eframe::Result {
    let startup_path = std::env::args_os().nth(1).map(PathBuf::from);
    macos::run(startup_path, native_options())
}
