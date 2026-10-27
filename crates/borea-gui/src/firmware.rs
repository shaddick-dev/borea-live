//! Firmware actions run off the UI thread. Preview instances never contact the host.
use std::{
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use eframe::egui;
use serde_json::Value;
use zbus::blocking::{Connection, Proxy};

pub struct FirmwareLink {
    pub available: bool,
    pub reason: String,
    pub restarting: bool,
    pub error: Option<String>,
    preview: bool,
    pending: Option<Receiver<Result<(), String>>>,
}

impl FirmwareLink {
    pub fn preview() -> Self {
        Self {
            available: true,
            reason: String::new(),
            restarting: false,
            error: None,
            preview: true,
            pending: None,
        }
    }

    pub fn system(ctx: egui::Context) -> Self {
        let mut link = Self::preview();
        link.preview = false;
        link.available = false;
        link.reason = "Checking firmware restart support…".into();
        link.start(ctx, false);
        link
    }

    fn start(&mut self, ctx: egui::Context, reboot: bool) {
        let (tx, rx) = mpsc::channel();
        match thread::Builder::new().name("firmware-setup".into()).spawn(move || {
            let _ = tx.send(request(reboot));
            ctx.request_repaint();
        }) {
            Ok(_) => self.pending = Some(rx),
            Err(error) => {
                self.restarting = false;
                self.error = Some(format!("Could not start the firmware request: {error}"));
            }
        }
    }

    pub fn poll(&mut self) {
        let Some(rx) = &self.pending else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The firmware service stopped responding.".into())
            }
        };
        self.pending = None;
        if self.restarting {
            if let Err(error) = result {
                self.restarting = false;
                self.error = Some(error);
            }
        } else {
            self.available = result.is_ok();
            self.reason = result.err().unwrap_or_default();
        }
    }

    pub fn reboot(&mut self, ctx: egui::Context) {
        if !self.available || self.restarting {
            return;
        }
        self.error = None;
        if self.preview {
            self.error = Some("Firmware restart is unavailable in this preview.".into());
            return;
        }
        self.restarting = true;
        self.start(ctx, true);
    }
}

fn backend_request(reboot: bool) -> Result<(), String> {
    let timeout = Duration::from_secs(30);
    if reboot {
        let params = borea_protocol::RebootParams { firmware_setup: true };
        return borea_protocol::call::<_, Value>(
            borea_protocol::method::SYSTEM_REBOOT,
            &params,
            timeout,
        )
        .map(drop)
        .map_err(|e| e.to_string());
    }
    let info: borea_protocol::SystemInfo =
        borea_protocol::call(borea_protocol::method::SYSTEM_INFO, &Value::Null, timeout)
            .map_err(|e| e.to_string())?;
    if !info.firmware_setup {
        return Err("This firmware does not support restarting directly into setup. Use its startup key instead.".into());
    }
    Ok(())
}

fn request(reboot: bool) -> Result<(), String> {
    // installd owns the action, including its active-job guard. The logind path is
    // only for development sessions without the service: installs need installd, so
    // with no socket there is no installation to interrupt.
    if borea_protocol::present() {
        return backend_request(reboot);
    }
    let connection = Connection::system().map_err(|e| e.to_string())?;
    let manager = Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(|e| e.to_string())?;
    let capability: String =
        manager.call("CanRebootToFirmwareSetup", &()).map_err(|e| e.to_string())?;
    if !matches!(capability.as_str(), "yes" | "challenge") {
        return Err("This session cannot restart into firmware setup. Use your firmware's startup key instead.".into());
    }
    if reboot {
        manager
            .call::<_, _, ()>("SetRebootToFirmwareSetup", &(true,))
            .map_err(|e| e.to_string())?;
        if let Err(error) = manager.call::<_, _, ()>("Reboot", &(false,)) {
            // A refused request must not leave a firmware flag for an unrelated reboot.
            let _ = manager.call::<_, _, ()>("SetRebootToFirmwareSetup", &(false,));
            return Err(format!("Could not restart into firmware setup: {error}"));
        }
    }
    Ok(())
}
