//! Windpad's Wi-Fi drawer, backed by NetworkManager on a worker thread so the window
//! never waits on D-Bus.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use borea_network::{Network, Secret, Security, Status, Wifi};
use eframe::egui::{self, Align, ComboBox, Layout, RichText, ScrollArea, TextEdit, Ui};

use crate::theme::{Palette, size};
use crate::widgets::{caption, choice_card, notice, primary, quiet};

pub enum Command {
    Refresh,
    Rescan,
    Join { network: Network, passphrase: Option<Secret> },
    JoinHidden { ssid: String, security: Security, passphrase: Option<Secret> },
}

enum Update {
    State { status: Status, networks: Vec<Network> },
    Message(String),
    Unavailable(String),
}

/// Reach NetworkManager, waiting for it to start if the installer came up first.
fn connect(send: &impl Fn(Update)) -> Option<Wifi> {
    let started = std::time::Instant::now();
    loop {
        match Wifi::system() {
            Ok(wifi) => return Some(wifi),
            Err(error) if started.elapsed() > STARTUP => {
                send(Update::Unavailable(error.to_string()));
                return None;
            }
            Err(_) => {
                send(Update::Message("Starting the network…".into()));
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

/// What the drawer shows, kept current by the worker.
pub struct WifiLink {
    commands: Option<Sender<Command>>,
    updates: Option<Receiver<Update>>,
    pub status: Option<Status>,
    pub networks: Vec<Network>,
    pub message: Option<String>,
    pub unavailable: Option<String>,
}

/// After asking NetworkManager to connect or scan, look again once it has had a moment.
const SETTLE: Duration = Duration::from_secs(3);
/// With nothing asked, look again this often, so connections made elsewhere (a cable
/// plugged in, a network that just came into range) show up on their own.
const WATCH: Duration = Duration::from_secs(4);
/// At boot NetworkManager may start after the installer; keep trying for this long.
const STARTUP: Duration = Duration::from_secs(90);

impl WifiLink {
    pub fn spawn(ctx: egui::Context) -> Self {
        let (commands, inbox) = mpsc::channel::<Command>();
        let (outbox, updates) = mpsc::channel::<Update>();
        thread::Builder::new()
            .name("windpad-wifi".into())
            .spawn(move || {
                let send = |update| {
                    let _ = outbox.send(update);
                    ctx.request_repaint();
                };
                let Some(wifi) = connect(&send) else { return };
                let refresh = |wifi: &Wifi| match (wifi.status(), wifi.networks()) {
                    (Ok(status), Ok(networks)) => send(Update::State { status, networks }),
                    (Err(error), _) | (_, Err(error)) => send(Update::Message(error.to_string())),
                };
                // Scan straight away rather than showing whatever NetworkManager saw last.
                let _ = wifi.rescan();
                refresh(&wifi);
                loop {
                    let command = match inbox.recv_timeout(WATCH) {
                        Ok(command) => command,
                        Err(mpsc::RecvTimeoutError::Timeout) => Command::Refresh,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    };
                    let outcome = match command {
                        Command::Refresh => Ok(None),
                        Command::Rescan => wifi.rescan().map(|()| Some("Scanning…".to_owned())),
                        Command::Join { network, passphrase } => wifi
                            .join(&network, passphrase)
                            .map(|()| Some(format!("Joining “{}”…", network.ssid))),
                        Command::JoinHidden { ssid, security, passphrase } => wifi
                            .join_hidden(&ssid, security, passphrase)
                            .map(|()| Some(format!("Looking for “{ssid}”…"))),
                    };
                    match outcome {
                        Ok(Some(message)) => {
                            send(Update::Message(message));
                            thread::sleep(SETTLE);
                        }
                        Ok(None) => {}
                        Err(error) => send(Update::Message(error.to_string())),
                    }
                    refresh(&wifi);
                }
            })
            .expect("spawn Wi-Fi worker");
        WifiLink {
            commands: Some(commands),
            updates: Some(updates),
            status: None,
            networks: Vec::new(),
            message: None,
            unavailable: None,
        }
    }

    /// A link that already has a state, with no backend: for tests and previews.
    pub fn simulated(status: Status, networks: Vec<Network>) -> Self {
        WifiLink {
            commands: None,
            updates: None,
            status: Some(status),
            networks,
            message: None,
            unavailable: None,
        }
    }

    /// A drawer with no backend, for previews and tests.
    pub fn offline(reason: &str) -> Self {
        WifiLink {
            commands: None,
            updates: None,
            status: None,
            networks: Vec::new(),
            message: None,
            unavailable: Some(reason.to_owned()),
        }
    }

    pub fn poll(&mut self) {
        let Some(updates) = &self.updates else { return };
        for update in updates.try_iter() {
            match update {
                Update::State { status, networks } => {
                    self.status = Some(status);
                    self.networks = networks;
                }
                Update::Message(message) => self.message = Some(message),
                Update::Unavailable(reason) => self.unavailable = Some(reason),
            }
        }
    }

    pub fn send(&self, command: Command) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(command);
        }
    }

    pub fn online(&self) -> bool {
        self.status.as_ref().is_some_and(|s| s.connectivity == borea_network::Connectivity::Full)
    }
}

/// What the user is typing in the Wi-Fi panel. The passphrase is cleared whenever it
/// is sent or the panel moves to another network.
#[derive(Default)]
pub struct WifiForm {
    picked: Option<String>,
    passphrase: String,
    reveal: bool,
    hidden_ssid: String,
    hidden_security: usize,
    /// Put the cursor in the passphrase field on the next frame.
    focus: bool,
}

impl WifiForm {
    /// Choose a network for the user, ready to type its passphrase, unless they have
    /// already chosen one themselves.
    pub fn suggest(&mut self, ssid: &str) {
        if self.picked.is_none() {
            self.picked = Some(ssid.to_owned());
            self.focus = true;
        }
    }

    pub fn picked(&self) -> Option<&str> {
        self.picked.as_deref()
    }

    pub fn forget_passphrase(&mut self) {
        self.passphrase.clear();
    }

    fn take_passphrase(&mut self, security: Security) -> Option<Secret> {
        let passphrase = std::mem::take(&mut self.passphrase);
        security.needs_passphrase().then(|| Secret::new(passphrase))
    }
}

const HIDDEN_SECURITY: [(Security, &str); 3] =
    [(Security::WpaPersonal, "WPA2"), (Security::Wpa3Personal, "WPA3"), (Security::Open, "Open")];

/// The Wi-Fi panel, shared by Windpad's drawer and the live installer's Connect step.
/// Returns a request for the worker when the user asks for one.
pub fn panel(ui: &mut Ui, p: &Palette, link: &WifiLink, form: &mut WifiForm) -> Option<Command> {
    let mut command = None;
    if let Some(reason) = &link.unavailable {
        notice(ui, p, &format!("Wi-Fi is unavailable here: {reason}."));
        return None;
    }
    let Some(status) = &link.status else {
        ui.label(caption("Asking NetworkManager…", p));
        return None;
    };
    ui.horizontal(|ui| {
        let radio = match (status.radio_unblocked, status.radio_enabled) {
            (false, _) => "Wi-Fi blocked by a switch",
            (true, false) => "Wi-Fi off",
            (true, true) => "Wi-Fi on",
        };
        ui.label(caption(format!("Internet: {} · {radio}", status.connectivity), p));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if quiet(ui, p, "Rescan").clicked() {
                command = Some(Command::Rescan);
            }
        });
    });
    if let Some(message) = &link.message {
        ui.label(caption(message, p));
    }
    ui.add_space(6.0);
    ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        for network in &link.networks {
            let picked = form.picked.as_deref() == Some(network.ssid.as_str());
            let label = format!("{} · {}% · {}", network.ssid, network.strength, network.security);
            let card = choice_card(
                ui,
                p,
                ("ssid", &network.ssid),
                &label,
                picked || network.active,
                true,
                |ui| {
                    ui.horizontal(|ui| {
                        let dot = if network.active {
                            format!("{}  ", crate::icon::DOT)
                        } else {
                            String::new()
                        };
                        ui.label(RichText::new(format!("{dot}{}", network.ssid)).size(size::BODY));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let detail = format!(
                                "{}% · {} · {}",
                                network.strength, network.security, network.band
                            );
                            ui.label(caption(detail, p));
                        });
                    });
                },
            );
            if card.clicked() && !picked {
                form.passphrase.clear();
                form.picked = Some(network.ssid.clone());
            }
        }
    });
    if let Some(network) =
        link.networks.iter().find(|n| form.picked.as_deref() == Some(n.ssid.as_str()))
    {
        ui.add_space(6.0);
        let mut submitted = false;
        if network.security.needs_passphrase() {
            let focus = std::mem::take(&mut form.focus);
            submitted = passphrase_field(ui, p, &mut form.passphrase, &mut form.reveal, focus);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let join = primary(ui, p, &format!("Join {}", network.ssid), false).clicked();
            if join || submitted {
                let passphrase = form.take_passphrase(network.security);
                command = Some(Command::Join { network: network.clone(), passphrase });
            }
        });
    }
    ui.add_space(6.0);
    egui::CollapsingHeader::new(caption("Hidden network", p)).show(ui, |ui| {
        let name = ui.label(RichText::new("Network name").size(size::TINY).color(p.sub));
        ui.add(TextEdit::singleline(&mut form.hidden_ssid).desired_width(f32::INFINITY))
            .labelled_by(name.id);
        let security = ui.label(RichText::new("Security").size(size::TINY).color(p.sub));
        ComboBox::from_id_salt("hidden-security")
            .selected_text(HIDDEN_SECURITY[form.hidden_security].1)
            .show_index(ui, &mut form.hidden_security, HIDDEN_SECURITY.len(), |i| {
                HIDDEN_SECURITY[i].1
            })
            .labelled_by(security.id);
        let (kind, _) = HIDDEN_SECURITY[form.hidden_security];
        if kind.needs_passphrase() {
            passphrase_field(ui, p, &mut form.passphrase, &mut form.reveal, false);
        }
        if !form.hidden_ssid.trim().is_empty()
            && primary(ui, p, "Join hidden network", false).clicked()
        {
            let passphrase = form.take_passphrase(kind);
            command = Some(Command::JoinHidden {
                ssid: form.hidden_ssid.trim().to_owned(),
                security: kind,
                passphrase,
            });
        }
    });
    ui.add_space(4.0);
    ui.label(caption(
        "Connections are added to this live session only. Passphrases are never logged.",
        p,
    ));
    command
}

/// The passphrase box. Returns true when Enter was pressed in it.
fn passphrase_field(
    ui: &mut Ui,
    p: &Palette,
    passphrase: &mut String,
    reveal: &mut bool,
    focus: bool,
) -> bool {
    let label = ui.label(RichText::new("Passphrase").size(size::TINY).color(p.sub));
    let field = ui
        .add(TextEdit::singleline(passphrase).password(!*reveal).desired_width(f32::INFINITY))
        .labelled_by(label.id);
    if focus {
        field.request_focus();
    }
    ui.checkbox(reveal, caption("Show passphrase", p));
    field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !passphrase.is_empty()
}
