//! Windpad's Wi-Fi drawer, backed by NetworkManager on a worker thread so the window
//! never waits on D-Bus.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use borea_network::{Network, Secret, Security, Status, Wifi};

pub enum Command {
    Refresh,
    Rescan,
    Join { network: Network, passphrase: Option<Secret> },
    JoinHidden { ssid: String, security: Security, passphrase: Option<Secret> },
}

enum Update {
    Status(Status),
    Networks(Vec<Network>),
    Message(String),
    Unavailable(String),
}

// A scan can fail while a cable or an already joined network is working. Never
// let that failure prevent fresh connection status from reaching the interface.
fn publish_refresh(
    status: borea_network::Result<Status>,
    networks: borea_network::Result<Vec<Network>>,
    send: &impl Fn(Update),
) {
    match status {
        Ok(status) => send(Update::Status(status)),
        Err(error) => send(Update::Message(error.to_string())),
    }
    match networks {
        Ok(networks) => send(Update::Networks(networks)),
        Err(error) => send(Update::Message(error.to_string())),
    }
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
    pub fn spawn() -> Self {
        let (commands, inbox) = mpsc::channel::<Command>();
        let (outbox, updates) = mpsc::channel::<Update>();
        thread::Builder::new()
            .name("windpad-wifi".into())
            .spawn(move || {
                let send = |update| {
                    let _ = outbox.send(update);
                };
                let Some(wifi) = connect(&send) else { return };
                let refresh = |wifi: &Wifi| {
                    publish_refresh(wifi.status(), wifi.networks(), &send);
                };
                // Scan straight away rather than showing whatever NetworkManager saw last.
                let _ = wifi.rescan();
                refresh(&wifi);
                // A USB adapter plugged in later is scanned with as soon as it appears.
                let mut adapters = wifi.adapter_count();
                loop {
                    let command = match inbox.recv_timeout(WATCH) {
                        Ok(command) => command,
                        Err(mpsc::RecvTimeoutError::Timeout) => Command::Refresh,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    };
                    let now = wifi.adapter_count();
                    let plugged_in = now > adapters;
                    adapters = now;
                    let outcome = match command {
                        Command::Refresh if plugged_in => wifi
                            .rescan()
                            .map(|()| Some("Found a Wi-Fi adapter. Scanning…".to_owned())),
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
                            refresh(&wifi);
                            // Publish the joined network before asking for a
                            // probe, which may take a while on a captive portal.
                            let _ = wifi.check_connectivity();
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
                Update::Status(status) => {
                    if status.connectivity == borea_network::Connectivity::Full {
                        self.message = None;
                    }
                    self.status = Some(status);
                }
                Update::Networks(networks) => self.networks = networks,
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

    pub fn connected(&self) -> bool {
        self.online()
            || self.status.as_ref().is_some_and(|s| s.primary.is_some())
            || self.networks.iter().any(|n| n.active)
    }

    /// Association and internet reachability are different facts. A blocked
    /// connectivity probe must not label a joined Wi-Fi network as disconnected.
    pub fn connection_label(&self) -> String {
        use borea_network::Connectivity;
        if self.online() {
            return "Online".into();
        }
        if self.status.as_ref().is_some_and(|s| s.connectivity == Connectivity::Portal) {
            return "Connected · sign-in required".into();
        }
        if self.connected() {
            return match self.status.as_ref().map(|s| s.connectivity) {
                Some(Connectivity::Limited) => "Connected · local network only".into(),
                _ => "Connected · internet unconfirmed".into(),
            };
        }
        if self.unavailable.is_some() {
            "Network status unavailable".into()
        } else if self.status.is_none() {
            "Checking connection…".into()
        } else {
            "Offline".into()
        }
    }
}

/// What the user is typing in the Wi-Fi panel. The passphrase is cleared whenever it
/// is sent or the panel moves to another network.
#[derive(Default)]
pub struct WifiForm {
    picked: Option<String>,
}
impl WifiForm {
    pub fn suggest(&mut self, ssid: &str) {
        if self.picked.is_none() {
            self.picked = Some(ssid.to_owned());
        }
    }
    pub fn picked(&self) -> Option<&str> {
        self.picked.as_deref()
    }
}

pub(crate) fn valid_passphrase(security: Security, text: &str) -> bool {
    match security {
        Security::WpaPersonal => {
            (8..=63).contains(&text.len())
                || text.len() == 64 && text.bytes().all(|c| c.is_ascii_hexdigit())
        }
        Security::Wpa3Personal => (1..=63).contains(&text.len()),
        Security::Open | Security::EnhancedOpen => true,
        Security::Wep | Security::Enterprise => false,
    }
}

/// A bounded row keeps Join immediately beside the field, instead of centering
/// it in all of the space left below the network list.
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
mod tests {
    use super::*;
    use borea_network::{Band, Connectivity, NetworkError};

    fn status(connectivity: Connectivity) -> Status {
        Status { connectivity, radio_enabled: true, radio_unblocked: true, primary: None }
    }

    #[test]
    fn scan_failure_does_not_discard_a_working_connection() {
        let (tx, rx) = mpsc::channel();
        publish_refresh(Ok(status(Connectivity::Full)), Err(NetworkError::NoAdapter), &|update| {
            tx.send(update).unwrap();
        });
        let mut link = WifiLink::offline("test");
        link.unavailable = None;
        link.updates = Some(rx);
        link.poll();
        assert!(link.online());
        assert!(link.message.is_some(), "the scan error is still reported");
    }

    #[test]
    fn joined_wifi_is_connected_even_when_the_internet_probe_is_limited() {
        let link = WifiLink::simulated(
            status(Connectivity::Limited),
            vec![Network::sample("Home", 90, Security::WpaPersonal, Band::Ghz5, true)],
        );
        assert!(link.connected());
        assert!(!link.online(), "association does not prove internet access");
        assert_eq!(link.connection_label(), "Connected · local network only");
    }

    #[test]
    fn cable_portal_and_pending_states_are_distinct() {
        let mut state = status(Connectivity::Unknown);
        state.primary = Some("Wired connection".into());
        let mut link = WifiLink::simulated(state, vec![]);
        assert_eq!(link.connection_label(), "Connected · internet unconfirmed");
        link.status.as_mut().unwrap().connectivity = Connectivity::Portal;
        assert_eq!(link.connection_label(), "Connected · sign-in required");
        link.status = None;
        assert_eq!(link.connection_label(), "Checking connection…");
    }

    #[test]
    fn submission_requires_a_valid_personal_network_key() {
        assert!(!valid_passphrase(Security::WpaPersonal, ""));
        assert!(!valid_passphrase(Security::WpaPersonal, "short"));
        assert!(valid_passphrase(Security::WpaPersonal, "long-enough"));
        assert!(valid_passphrase(Security::WpaPersonal, &"a".repeat(64)));
        assert!(!valid_passphrase(Security::WpaPersonal, &"g".repeat(64)));
        assert!(valid_passphrase(Security::Wpa3Personal, "a"));
        assert!(!valid_passphrase(Security::Enterprise, "long-enough"));
        assert!(valid_passphrase(Security::Open, ""));
    }
}
