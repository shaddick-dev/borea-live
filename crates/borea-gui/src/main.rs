//! `windpad`: open Borea's installer window.

use borea_gui::Windpad;
use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Borea · Windpad")
            .with_app_id("borea-windpad")
            .with_inner_size([1180.0, 860.0])
            .with_min_inner_size([380.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("Borea · Windpad", options, Box::new(|cc| Ok(Box::new(Windpad::new(cc)))))
}
