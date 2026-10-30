//! The wire contract between `borea-installd` and its clients (the Borea Live GUI
//! and `borea-cli`).
//!
//! JSON-RPC 2.0, one JSON object per line, over [`SOCKET`]. Method names, the
//! socket path and the small typed payloads that more than one program reads live
//! here, so a rename on one side fails to compile on the other instead of failing
//! on a user's machine.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    fmt,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

mod registry;
pub use registry::{METHODS, Method, markdown};

/// The service's listening socket.
pub const SOCKET: &str = "/run/borea/installd.sock";

/// Method names. Used as match patterns by the service and as call targets by clients.
pub mod method {
    pub const SYSTEM_INFO: &str = "system.info";
    pub const SYSTEM_METHODS: &str = "system.methods";
    pub const HARDWARE_GET: &str = "hardware.get";
    pub const NET_SCAN: &str = "net.scan";
    pub const NET_CONNECT: &str = "net.connect";
    pub const NET_STATUS: &str = "net.status";
    pub const GEO_SUGGEST: &str = "geo.suggest";
    pub const LOCALE_LANGUAGES: &str = "locale.languages";
    pub const LOCALE_KEYBOARDS: &str = "locale.keyboards";
    pub const LOCALE_TIMEZONES: &str = "locale.timezones";
    pub const CATALOG_GET: &str = "catalog.get";
    pub const DISKS_LIST: &str = "disks.list";
    pub const PLAN_VALIDATE: &str = "plan.validate";
    pub const PLAN_REVIEW: &str = "plan.review";
    pub const PLAN_RENDER: &str = "plan.render";
    pub const INSTALL_START: &str = "install.start";
    pub const INSTALL_EVENTS: &str = "install.events";
    pub const INSTALL_SUBSCRIBE: &str = "install.subscribe";
    pub const INSTALL_RETRY: &str = "install.retry";
    pub const INSTALL_LOG: &str = "install.log";
    pub const SYSTEM_REBOOT: &str = "system.reboot";
    pub const HANDOFF_REVIEW: &str = "handoff.review";
    pub const HANDOFF_START: &str = "handoff.start";
}

/// Result of `system.info`. Hardware-specific facts stay as JSON because their
/// types belong to crates the clients do not link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub version: String,
    pub architecture: String,
    pub uefi: bool,
    pub secure_boot: Value,
    pub firmware_setup: bool,
    pub motion: bool,
    pub power: Value,
}

/// Parameters of `system.reboot`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RebootParams {
    #[serde(default)]
    pub firmware_setup: bool,
}

/// Why a call did not produce a result.
#[derive(Debug)]
pub enum Error {
    /// The service is not listening (not installed, not started, or stopped).
    Unavailable(io::Error),
    /// The connection failed part-way, or timed out.
    Io(io::Error),
    /// The service answered with a JSON-RPC error.
    Refused { message: String, field: Option<String> },
    /// The reply was not a well-formed response to this request.
    Invalid,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unavailable(_) => f.write_str("The installer service is not running."),
            Error::Io(e) => write!(f, "The installer service connection failed: {e}"),
            Error::Refused { message, .. } => f.write_str(message),
            Error::Invalid => f.write_str("The installer service sent an invalid response."),
        }
    }
}

impl std::error::Error for Error {}

/// Largest reply a client reads; `install.log` is the biggest (512 KiB of text).
pub const MAX_RESPONSE: u64 = 4 * 1024 * 1024;

/// True when the service's socket exists. A stale socket still fails to connect.
pub fn present() -> bool {
    Path::new(SOCKET).exists()
}

/// One blocking request and its reply on a fresh connection.
pub fn call<P: Serialize, R: DeserializeOwned>(
    method: &str,
    params: &P,
    timeout: Duration,
) -> Result<R, Error> {
    let mut stream = UnixStream::connect(SOCKET).map_err(Error::Unavailable)?;
    stream.set_read_timeout(Some(timeout)).map_err(Error::Io)?;
    stream.set_write_timeout(Some(timeout)).map_err(Error::Io)?;
    #[derive(Serialize)]
    struct Request<'a, P> {
        jsonrpc: &'static str,
        id: u8,
        method: &'a str,
        params: &'a P,
    }
    let request = Request { jsonrpc: "2.0", id: 1, method, params };
    let mut line =
        zeroize::Zeroizing::new(serde_json::to_vec(&request).map_err(|_| Error::Invalid)?);
    line.push(b'\n');
    stream.write_all(&line).map_err(Error::Io)?;
    read_response(stream)
}

/// Reads and checks a single response line for request id 1.
pub fn read_response<R: DeserializeOwned>(stream: impl Read) -> Result<R, Error> {
    let mut line = String::new();
    BufReader::new(stream.take(MAX_RESPONSE)).read_line(&mut line).map_err(Error::Io)?;
    parse_response(&line)
}

fn parse_response<R: DeserializeOwned>(line: &str) -> Result<R, Error> {
    let response: Value = serde_json::from_str(line).map_err(|_| Error::Invalid)?;
    if response["jsonrpc"] != "2.0" || response["id"] != 1 {
        return Err(Error::Invalid);
    }
    if let Some(error) = response.get("error") {
        return Err(Error::Refused {
            message: error["message"]
                .as_str()
                .unwrap_or("The installer request failed.")
                .to_owned(),
            field: error["data"]["field"].as_str().map(str::to_owned),
        });
    }
    let result = response.get("result").ok_or(Error::Invalid)?;
    serde_json::from_value(result.clone()).map_err(|_| Error::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_uses_every_method_once() {
        let mut names: Vec<_> = METHODS.iter().map(|m| m.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
        assert_eq!(count, 23);
    }

    #[test]
    fn responses_are_checked() {
        let ok: Option<()> = parse_response(r#"{"jsonrpc":"2.0","id":1,"result":null}"#).ok();
        assert_eq!(ok, Some(()));
        assert!(matches!(
            parse_response::<Value>(r#"{"jsonrpc":"2.0","id":2,"result":null}"#),
            Err(Error::Invalid)
        ));
        assert!(matches!(
            parse_response::<Value>(r#"{"jsonrpc":"2.0","id":1}"#),
            Err(Error::Invalid)
        ));
        match parse_response::<Value>(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"busy","data":{"field":"disk"}}}"#,
        ) {
            Err(Error::Refused { message, field }) => {
                assert_eq!(message, "busy");
                assert_eq!(field.as_deref(), Some("disk"));
            }
            other => assert!(matches!(other, Err(Error::Refused { .. })), "{other:?}"),
        }
    }

    #[test]
    fn system_info_round_trips() {
        let info: SystemInfo = serde_json::from_value(json!({
            "version": "0.1.0", "architecture": "x86_64", "uefi": true,
            "secure_boot": null, "firmware_setup": false, "motion": true, "power": {}
        }))
        .unwrap_or_else(|_| unreachable!());
        assert!(!info.firmware_setup);
    }
}
