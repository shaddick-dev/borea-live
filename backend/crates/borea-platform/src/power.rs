//! Optional read-only sysfs power facts. Unknown state is never reported as AC.
use serde::Serialize;
use std::{fs, path::Path};
#[derive(Debug, Serialize)]
pub struct Power {
    pub battery_percent: Option<u8>,
    pub ac_connected: Option<bool>,
    pub warning: Option<String>,
}
pub fn probe(root: &Path) -> Power {
    let mut capacity: Option<u8> = None;
    let mut ac = None;
    let mut discharging = false;
    if let Ok(entries) = fs::read_dir(root.join("sys/class/power_supply")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let read = |name| fs::read_to_string(path.join(name)).ok().map(|s| s.trim().to_owned());
            if read("type").as_deref() == Some("Battery") {
                if read("present").as_deref() == Some("0") {
                    continue;
                }
                if let Some(percent) =
                    read("capacity").and_then(|s| s.parse::<u8>().ok()).filter(|p| *p <= 100)
                {
                    capacity = Some(capacity.map_or(percent, |previous| previous.min(percent)));
                }
                discharging |= read("status").as_deref() == Some("Discharging");
            } else if let Some(online) = read("online").filter(|s| s == "0" || s == "1") {
                ac = Some(ac.unwrap_or(false) || online == "1");
            }
        }
    }
    if ac.is_none() && discharging {
        ac = Some(false);
    }
    let warning = if ac == Some(false) && capacity.is_some_and(|p| p < 30) {
        Some("Connect AC power: a battery is below 30%. Losing power during installation can leave the system unbootable.".into())
    } else {
        None
    };
    Power { battery_percent: capacity, ac_connected: ac, warning }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[allow(clippy::unwrap_used)]
    fn low_discharging_battery_warns_and_online_ac_clears_the_warning() {
        let root = std::env::temp_dir().join(format!("borea-power-fixture-{}", std::process::id()));
        let unknown = probe(&root);
        assert_eq!(unknown.ac_connected, None);
        assert!(unknown.warning.is_none());
        let battery = root.join("sys/class/power_supply/BAT0");
        fs::create_dir_all(&battery).unwrap();
        for (name, value) in [("type", "Battery"), ("capacity", "29"), ("status", "Discharging")] {
            fs::write(battery.join(name), value).unwrap();
        }
        assert!(probe(&root).warning.is_some());
        let ac = root.join("sys/class/power_supply/AC0");
        fs::create_dir(&ac).unwrap();
        fs::write(ac.join("type"), "Mains").unwrap();
        fs::write(ac.join("online"), "1").unwrap();
        assert!(probe(&root).warning.is_none());
        assert_eq!(probe(&root).ac_connected, Some(true));
        fs::remove_dir_all(root).unwrap();
    }
}
