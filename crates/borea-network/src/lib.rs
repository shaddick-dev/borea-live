//! Integrated Wi-Fi, shared by the `borea` bootstrapper and Windpad.
//!
//! Talks to NetworkManager over the D-Bus system bus, the same service Borea Live
//! runs. Reading state and scanning change nothing persistent. Joining a network
//! creates a NetworkManager connection in the *live session* only; carrying it into
//! the installed target is a separate, explicit choice.
//!
//! Passphrases travel as [`Secret`]: never in command arguments, never printed, and
//! overwritten when dropped.

use std::collections::HashMap;
use std::fmt;

use thiserror::Error;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, Value};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";

/// `NM_DEVICE_TYPE_WIFI`.
const DEVICE_TYPE_WIFI: u32 = 2;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("NetworkManager is not reachable on the system bus ({0})")]
    Unavailable(zbus::Error),
    #[error("NetworkManager request failed: {0}")]
    Bus(#[from] zbus::Error),
    #[error(
        "no Wi-Fi adapter yet: plug in a USB Wi-Fi adapter, an Ethernet cable, or a phone with USB tethering on, and it appears here by itself"
    )]
    NoAdapter,
    #[error("no network named “{0}” is in range (use hidden to join a hidden network)")]
    NotInRange(String),
    #[error("{0} networks need certificate setup, which Borea does not do yet")]
    Unsupported(Security),
    #[error("this network needs a passphrase")]
    PassphraseRequired,
}

pub type Result<T> = std::result::Result<T, NetworkError>;

/// Whether the internet is actually reachable, not just whether Wi-Fi associated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connectivity {
    Unknown,
    None,
    /// Associated, but a captive portal wants a browser sign-in first.
    Portal,
    /// Local network only.
    Limited,
    Full,
}

impl Connectivity {
    fn from_nm(state: u32) -> Self {
        match state {
            1 => Connectivity::None,
            2 => Connectivity::Portal,
            3 => Connectivity::Limited,
            4 => Connectivity::Full,
            _ => Connectivity::Unknown,
        }
    }
}

impl fmt::Display for Connectivity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Connectivity::Unknown => "unknown",
            Connectivity::None => "offline",
            Connectivity::Portal => "sign-in required (captive portal)",
            Connectivity::Limited => "local network only",
            Connectivity::Full => "online",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Security {
    Open,
    /// Opportunistic Wireless Encryption: open, but encrypted.
    EnhancedOpen,
    Wep,
    WpaPersonal,
    Wpa3Personal,
    Enterprise,
}

impl Security {
    /// Classify from NetworkManager's AccessPoint `Flags`, `WpaFlags`, `RsnFlags`.
    pub fn from_flags(flags: u32, wpa: u32, rsn: u32) -> Self {
        const PRIVACY: u32 = 0x1;
        const PSK: u32 = 0x100;
        const EAP: u32 = 0x200;
        const SAE: u32 = 0x400;
        const OWE: u32 = 0x800 | 0x1000;
        let key_mgmt = wpa | rsn;
        match () {
            _ if key_mgmt & EAP != 0 => Security::Enterprise,
            _ if key_mgmt & SAE != 0 => Security::Wpa3Personal,
            _ if key_mgmt & PSK != 0 => Security::WpaPersonal,
            _ if key_mgmt & OWE != 0 => Security::EnhancedOpen,
            _ if flags & PRIVACY != 0 => Security::Wep,
            _ => Security::Open,
        }
    }

    pub fn needs_passphrase(self) -> bool {
        matches!(self, Security::Wep | Security::WpaPersonal | Security::Wpa3Personal)
    }
}

impl fmt::Display for Security {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Security::Open => "open",
            Security::EnhancedOpen => "enhanced open",
            Security::Wep => "WEP",
            Security::WpaPersonal => "WPA2",
            Security::Wpa3Personal => "WPA3",
            Security::Enterprise => "enterprise",
        })
    }
}

/// One network as the user sees it: an SSID, merged across access points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub ssid: String,
    /// Signal quality, 0–100, of the strongest access point.
    pub strength: u8,
    pub security: Security,
    pub band: Band,
    pub active: bool,
    access_point: String,
    device: String,
}

impl Network {
    /// A network as seen in a scan, without NetworkManager objects behind it: for
    /// previews and tests. Joining it is not possible.
    pub fn sample(ssid: &str, strength: u8, security: Security, band: Band, active: bool) -> Self {
        Network {
            ssid: ssid.into(),
            strength,
            security,
            band,
            active,
            access_point: String::new(),
            device: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Ghz2_4,
    Ghz5,
    Ghz6,
}

impl Band {
    pub fn from_mhz(mhz: u32) -> Self {
        match mhz {
            ..=3000 => Band::Ghz2_4,
            3001..=5925 => Band::Ghz5,
            _ => Band::Ghz6,
        }
    }
}

impl fmt::Display for Band {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Band::Ghz2_4 => "2.4 GHz",
            Band::Ghz5 => "5 GHz",
            Band::Ghz6 => "6 GHz",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub connectivity: Connectivity,
    /// Wi-Fi switched on in software.
    pub radio_enabled: bool,
    /// Not blocked by a hardware switch.
    pub radio_unblocked: bool,
    /// Name of the connection carrying the default route.
    pub primary: Option<String>,
}

/// A passphrase. Not `Clone`, redacted in `Debug`, overwritten on drop.
pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Self {
        Secret(value)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Safe overwrite of the owned buffer; copies made by D-Bus serialization are
        // short-lived and outside our control, as the design notes acknowledge.
        let len = self.0.len();
        self.0.clear();
        self.0.extend(std::iter::repeat_n('\0', len));
    }
}

/// Merge access points into networks: one entry per SSID, strongest first, the
/// active network on top, hidden (empty) SSIDs left out.
pub fn merge(mut seen: Vec<Network>) -> Vec<Network> {
    seen.retain(|n| !n.ssid.is_empty());
    seen.sort_by_key(|n| std::cmp::Reverse((n.active, n.strength)));
    let mut merged: Vec<Network> = Vec::with_capacity(seen.len());
    for network in seen {
        if !merged.iter().any(|m| m.ssid == network.ssid) {
            merged.push(network);
        }
    }
    merged
}

pub struct Wifi {
    bus: Connection,
}

impl Wifi {
    pub fn system() -> Result<Self> {
        let bus = Connection::system().map_err(NetworkError::Unavailable)?;
        let wifi = Wifi { bus };
        // Fail early, with a clear message, when the bus has no NetworkManager.
        wifi.manager()?.get_property::<String>("Version").map_err(NetworkError::Unavailable)?;
        Ok(wifi)
    }

    fn proxy(&self, path: &str, interface: &'static str) -> Result<Proxy<'_>> {
        Ok(Proxy::new(&self.bus, NM, path.to_owned(), interface)?)
    }

    fn manager(&self) -> Result<Proxy<'_>> {
        self.proxy(NM_PATH, NM)
    }

    pub fn status(&self) -> Result<Status> {
        let nm = self.manager()?;
        let primary: OwnedObjectPath = nm.get_property("PrimaryConnection")?;
        let primary = match primary.as_str() {
            "/" => None,
            path => self.proxy(path, ACTIVE)?.get_property::<String>("Id").ok(),
        };
        Ok(Status {
            connectivity: Connectivity::from_nm(nm.get_property("Connectivity")?),
            radio_enabled: nm.get_property("WirelessEnabled")?,
            radio_unblocked: nm.get_property("WirelessHardwareEnabled")?,
            primary,
        })
    }

    /// Recheck internet reachability after a join instead of waiting up to a
    /// minute for NetworkManager's periodic probe. NetworkManager owns the
    /// configured probe and captive-portal detection; association alone is not
    /// treated as proof of internet access.
    pub fn check_connectivity(&self) -> Result<Connectivity> {
        let state: u32 = self.manager()?.call("CheckConnectivity", &())?;
        Ok(Connectivity::from_nm(state))
    }

    /// Wi-Fi devices' object paths.
    fn adapters(&self) -> Result<Vec<String>> {
        let devices: Vec<OwnedObjectPath> = self.manager()?.call("GetDevices", &())?;
        let mut wifi = Vec::new();
        for device in devices {
            let kind: u32 = self.proxy(device.as_str(), DEVICE)?.get_property("DeviceType")?;
            if kind == DEVICE_TYPE_WIFI {
                wifi.push(device.as_str().to_owned());
            }
        }
        Ok(wifi)
    }

    /// How many Wi-Fi adapters NetworkManager has now (0 when it can't say).
    pub fn adapter_count(&self) -> usize {
        self.adapters().map(|a| a.len()).unwrap_or(0)
    }

    /// Ask every adapter to rescan. Results arrive over the next few seconds.
    pub fn rescan(&self) -> Result<()> {
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        for device in self.adapters()? {
            self.proxy(&device, WIRELESS)?.call::<_, _, ()>("RequestScan", &(options.clone(),))?;
        }
        Ok(())
    }

    /// Networks in range, merged by SSID.
    pub fn networks(&self) -> Result<Vec<Network>> {
        let mut seen = Vec::new();
        for device in self.adapters()? {
            let wireless = self.proxy(&device, WIRELESS)?;
            let active: OwnedObjectPath = wireless.get_property("ActiveAccessPoint")?;
            let points: Vec<OwnedObjectPath> = wireless.call("GetAllAccessPoints", &())?;
            for point in points {
                let ap = self.proxy(point.as_str(), ACCESS_POINT)?;
                let ssid: Vec<u8> = ap.get_property("Ssid")?;
                seen.push(Network {
                    ssid: String::from_utf8_lossy(&ssid).into_owned(),
                    strength: ap.get_property("Strength")?,
                    security: Security::from_flags(
                        ap.get_property("Flags")?,
                        ap.get_property("WpaFlags")?,
                        ap.get_property("RsnFlags")?,
                    ),
                    band: Band::from_mhz(ap.get_property("Frequency")?),
                    active: point == active,
                    access_point: point.as_str().to_owned(),
                    device: device.clone(),
                });
            }
        }
        Ok(merge(seen))
    }

    /// Join a visible network. Creates a live-session connection and activates it.
    pub fn join(&self, network: &Network, passphrase: Option<Secret>) -> Result<()> {
        self.activate(
            &network.ssid,
            network.security,
            false,
            &network.device,
            &network.access_point,
            passphrase,
        )
    }

    /// Join a hidden network by name; NetworkManager probes for it directly.
    pub fn join_hidden(
        &self,
        ssid: &str,
        security: Security,
        passphrase: Option<Secret>,
    ) -> Result<()> {
        let device = self.adapters()?.into_iter().next().ok_or(NetworkError::NoAdapter)?;
        self.activate(ssid, security, true, &device, "/", passphrase)
    }

    fn activate(
        &self,
        ssid: &str,
        security: Security,
        hidden: bool,
        device: &str,
        access_point: &str,
        passphrase: Option<Secret>,
    ) -> Result<()> {
        let key_mgmt = match security {
            Security::Enterprise | Security::Wep => {
                return Err(NetworkError::Unsupported(security));
            }
            Security::Wpa3Personal => Some("sae"),
            Security::WpaPersonal => Some("wpa-psk"),
            Security::EnhancedOpen => Some("owe"),
            Security::Open => None,
        };
        let passphrase = match (security.needs_passphrase(), passphrase) {
            (true, Some(secret)) if !secret.is_empty() => Some(secret),
            (true, _) => return Err(NetworkError::PassphraseRequired),
            (false, _) => None,
        };

        let mut settings: HashMap<&str, HashMap<&str, Value<'_>>> = HashMap::from([
            (
                "connection",
                HashMap::from([
                    ("type", Value::from("802-11-wireless")),
                    ("id", Value::from(ssid)),
                ]),
            ),
            (
                "802-11-wireless",
                HashMap::from([
                    ("ssid", Value::from(ssid.as_bytes())),
                    ("mode", Value::from("infrastructure")),
                    ("hidden", Value::from(hidden)),
                ]),
            ),
        ]);
        if let Some(key_mgmt) = key_mgmt {
            let mut wsec = HashMap::from([("key-mgmt", Value::from(key_mgmt))]);
            if let Some(secret) = &passphrase {
                wsec.insert("psk", Value::from(secret.0.as_str()));
            }
            settings.insert("802-11-wireless-security", wsec);
        }

        let device = OwnedObjectPath::try_from(device.to_owned()).map_err(zbus::Error::from)?;
        let access_point =
            OwnedObjectPath::try_from(access_point.to_owned()).map_err(zbus::Error::from)?;
        self.manager()?.call::<_, _, (OwnedObjectPath, OwnedObjectPath)>(
            "AddAndActivateConnection",
            &(settings, device, access_point),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_from_access_point_flags() {
        assert_eq!(Security::from_flags(0, 0, 0), Security::Open);
        assert_eq!(Security::from_flags(1, 0, 0), Security::Wep);
        assert_eq!(Security::from_flags(1, 0, 0x188), Security::WpaPersonal);
        assert_eq!(Security::from_flags(1, 0, 0x588), Security::Wpa3Personal, "WPA2/3 transition");
        assert_eq!(Security::from_flags(1, 0, 0x288), Security::Enterprise);
        assert_eq!(Security::from_flags(0, 0, 0x800), Security::EnhancedOpen);
    }

    fn net(ssid: &str, strength: u8, active: bool) -> Network {
        Network {
            ssid: ssid.into(),
            strength,
            security: Security::WpaPersonal,
            band: Band::Ghz5,
            active,
            access_point: String::new(),
            device: String::new(),
        }
    }

    #[test]
    fn merge_keeps_strongest_per_ssid_and_active_first() {
        let merged = merge(vec![
            net("Cafe", 40, false),
            net("Home", 55, true),
            net("Cafe", 80, false),
            net("", 99, false),
        ]);
        let order: Vec<_> = merged.iter().map(|n| (n.ssid.as_str(), n.strength)).collect();
        assert_eq!(order, [("Home", 55), ("Cafe", 80)]);
    }

    #[test]
    fn bands_and_connectivity() {
        assert_eq!(Band::from_mhz(2437), Band::Ghz2_4);
        assert_eq!(Band::from_mhz(5180), Band::Ghz5);
        assert_eq!(Band::from_mhz(5955), Band::Ghz6);
        assert_eq!(Connectivity::from_nm(2), Connectivity::Portal);
    }

    #[test]
    fn secrets_never_print() {
        let secret = Secret::new("hunter22".into());
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
    }
}
