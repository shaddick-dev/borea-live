//! `borea-live`: the installer Borea Live boots into, full-screen.

use borea_gui::LiveInstaller;
use eframe::egui;

/// Smoke mode exercises real window/context creation and several presented frames.
struct BootSmoke {
    installer: LiveInstaller,
    frames: u8,
}

impl eframe::App for BootSmoke {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.installer, ui, frame);
        self.frames += 1;
        if self.frames >= 3 {
            println!("BOREA_GUI_READY");
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

fn main() -> eframe::Result {
    // Borea Live starts it full-screen; `--window` is for development on a desktop.
    let windowed = std::env::args().any(|a| a == "--window");
    let smoke = std::env::args().any(|a| a == "--boot-smoke");
    let viewport = egui::ViewportBuilder::default()
        .with_title("Borea Live")
        .with_app_id("borea-live")
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([900.0, 600.0])
        .with_fullscreen(!windowed);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Borea Live",
        options,
        Box::new(move |cc| {
            let installer = LiveInstaller::new(cc);
            if smoke {
                Ok(Box::new(BootSmoke { installer, frames: 0 }))
            } else {
                Ok(Box::new(installer))
            }
        }),
    )
}
