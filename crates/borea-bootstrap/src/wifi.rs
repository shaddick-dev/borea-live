//! `borea wifi`: get online before anything is downloaded.

use std::io::Write;
use std::thread;
use std::time::Duration;

use borea_network::{Network, Secret, Security, Wifi};

use crate::Result;
use crate::ui::{Out, row, section};

/// How long a rescan is given before results are listed.
const SCAN_SETTLE: Duration = Duration::from_secs(3);

pub fn status(out: &mut Out, rescan: bool) -> Result {
    let wifi = Wifi::system()?;
    if rescan {
        wifi.rescan()?;
        thread::sleep(SCAN_SETTLE);
    }
    let status = wifi.status()?;
    section(out, "Connection")?;
    row(out, "Internet", status.connectivity)?;
    if let Some(primary) = &status.primary {
        row(out, "Through", primary)?;
    }
    match (status.radio_unblocked, status.radio_enabled) {
        (false, _) => row(out, "Wi-Fi", "blocked by a hardware switch or rfkill")?,
        (true, false) => row(out, "Wi-Fi", "switched off")?,
        (true, true) => row(out, "Wi-Fi", "on")?,
    }

    let networks = wifi.networks()?;
    section(out, "Networks in range")?;
    if networks.is_empty() {
        writeln!(out, "  none found; try `borea wifi scan`")?;
    }
    for network in &networks {
        let mark = if network.active { "●" } else { " " };
        writeln!(
            out,
            "  {mark} {:<28} {} {:>3}%  {:<13} {}",
            network.ssid,
            bars(network.strength),
            network.strength,
            network.security,
            network.band
        )?;
    }
    writeln!(out, "\n  Join with: borea wifi join \"<network name>\"")?;
    Ok(())
}

pub fn join(out: &mut Out, ssid: &str) -> Result {
    let wifi = Wifi::system()?;
    let network = wifi
        .networks()?
        .into_iter()
        .find(|n| n.ssid == ssid)
        .ok_or_else(|| borea_network::NetworkError::NotInRange(ssid.into()))?;
    let passphrase = ask(&network)?;
    wifi.join(&network, passphrase)?;
    writeln!(out, "Joining “{ssid}”. Check with: borea wifi")?;
    Ok(())
}

pub fn hidden(out: &mut Out, ssid: &str, security: &str) -> Result {
    let security = match security {
        "open" => Security::Open,
        "wpa2" => Security::WpaPersonal,
        "wpa3" => Security::Wpa3Personal,
        other => return Err(format!("security must be open, wpa2, or wpa3, not `{other}`").into()),
    };
    let passphrase = security.needs_passphrase().then(|| prompt(ssid)).transpose()?;
    Wifi::system()?.join_hidden(ssid, security, passphrase)?;
    writeln!(out, "Joining hidden network “{ssid}”. Check with: borea wifi")?;
    Ok(())
}

fn ask(network: &Network) -> Result<Option<Secret>> {
    Ok(network.security.needs_passphrase().then(|| prompt(&network.ssid)).transpose()?)
}

/// Read a passphrase from the terminal without echo.
fn prompt(ssid: &str) -> std::io::Result<Secret> {
    rpassword::prompt_password(format!("Passphrase for “{ssid}”: ")).map(Secret::new)
}

/// Signal as four bars.
fn bars(strength: u8) -> &'static str {
    match strength {
        0..=24 => "▂   ",
        25..=49 => "▂▄  ",
        50..=74 => "▂▄▆ ",
        _ => "▂▄▆█",
    }
}
