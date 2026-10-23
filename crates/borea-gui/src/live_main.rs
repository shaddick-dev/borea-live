//! `borea-live`: the installer Borea Live boots into, full-screen.

use borea_gui::LiveInstaller;
use eframe::egui;

fn main() -> eframe::Result {
    // Borea Live starts it full-screen; `--window` is for development on a desktop.
    let windowed = std::env::args().any(|a| a == "--window");
    let viewport = egui::ViewportBuilder::default()
        .with_title("Borea Live")
        .with_app_id("borea-live")
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([900.0, 600.0])
        .with_fullscreen(!windowed);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Borea Live", options, Box::new(|cc| Ok(Box::new(LiveInstaller::new(cc)))))
}
