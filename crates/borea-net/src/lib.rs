#![forbid(unsafe_code)]
pub mod nm;
use borea_core::{Hint, hint};
use serde::{Deserialize, Serialize};
pub const GEO_ENDPOINT: &str = "https://geoip.fedoraproject.org/city";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoSuggestion {
    pub country_code: String,
    pub time_zone: Option<String>,
}
pub fn geo_suggest() -> Option<GeoSuggestion> {
    let agent = borea_http::agent(std::time::Duration::from_secs(3)).ok()?;
    for _ in 0..2 {
        if let Ok(mut response) = agent.get(GEO_ENDPOINT).call() {
            if let Ok(s) = response.body_mut().read_to_string() {
                if let Ok(geo) = serde_json::from_str::<GeoSuggestion>(&s) {
                    if geo.country_code.len() == 2
                        && geo.country_code.bytes().all(|c| c.is_ascii_alphabetic())
                    {
                        return Some(geo);
                    }
                }
            }
        }
    }
    None
}
pub trait NetworkBackend {
    fn scan(&self) -> Result<Vec<nm::Network>, Hint>;
    fn status(&self) -> Result<nm::Status, Hint>;
    fn connect(&self, ssid: &str, hidden: bool, secret: Option<nm::Secret>) -> Result<(), Hint>;
}
impl NetworkBackend for nm::Wifi {
    fn scan(&self) -> Result<Vec<nm::Network>, Hint> {
        let scan = self.rescan();
        let networks = self.networks().map_err(|e| hint("network", e.to_string()))?;
        if networks.is_empty() {
            scan.map_err(|e| hint("network", e.to_string()))?;
        }
        Ok(networks)
    }
    fn status(&self) -> Result<nm::Status, Hint> {
        nm::Wifi::status(self).map_err(|e| hint("network", e.to_string()))
    }
    fn connect(&self, ssid: &str, hidden: bool, secret: Option<nm::Secret>) -> Result<(), Hint> {
        if hidden {
            return self
                .join_hidden(
                    ssid,
                    if secret.is_some() { nm::Security::WpaPersonal } else { nm::Security::Open },
                    secret,
                )
                .map_err(|e| hint("network", e.to_string()));
        }
        let networks = self.networks().map_err(|e| hint("network", e.to_string()))?;
        let network = networks
            .iter()
            .find(|n| n.ssid == ssid)
            .ok_or_else(|| hint("network", "This network is no longer in range."))?;
        self.join(network, secret).map_err(|e| hint("network", e.to_string()))
    }
}
pub fn check_mirror(url: &str) -> Result<(), Hint> {
    if !url.starts_with("https://") {
        return Err(hint("network", "The target mirror must use HTTPS."));
    }
    let agent = borea_http::agent(std::time::Duration::from_secs(10))?;
    agent
        .head(url)
        .call()
        .map(|_| ())
        .map_err(|_| hint("network", "The chosen distribution's mirror is not reachable."))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ConnectionState {
    Connecting,
    Connected,
    WrongPassword,
    Failed { reason: String },
}
pub fn connection_state(status: &nm::Status, ssid: &str, reason: Option<u32>) -> ConnectionState {
    if status.primary.as_deref() == Some(ssid) {
        return ConnectionState::Connected;
    }
    match reason {
        Some(7) => ConnectionState::WrongPassword,
        Some(code) => ConnectionState::Failed {
            reason: format!("NetworkManager activation failed (reason {code})."),
        },
        None => ConnectionState::Connecting,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Mock;
    impl NetworkBackend for Mock {
        fn scan(&self) -> Result<Vec<nm::Network>, Hint> {
            Ok(vec![])
        }
        fn status(&self) -> Result<nm::Status, Hint> {
            Ok(nm::Status {
                connectivity: nm::Connectivity::Limited,
                radio_enabled: true,
                radio_unblocked: true,
                primary: Some("Home".into()),
            })
        }
        fn connect(&self, _: &str, _: bool, _: Option<nm::Secret>) -> Result<(), Hint> {
            Ok(())
        }
    }
    #[test]
    fn association_and_internet_are_separate() {
        let status = Mock.status().unwrap_or_else(|_| unreachable!());
        assert_eq!(connection_state(&status, "Home", None), ConnectionState::Connected);
        assert_eq!(connection_state(&status, "Other", None), ConnectionState::Connecting);
        assert_eq!(connection_state(&status, "Other", Some(7)), ConnectionState::WrongPassword);
        assert!(matches!(
            connection_state(&status, "Other", Some(11)),
            ConnectionState::Failed { .. }
        ));
        assert_eq!(status.connectivity, nm::Connectivity::Limited);
    }
}
