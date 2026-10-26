#![forbid(unsafe_code)]
//! Shared proxy and system CA trust, with TLS verification always enabled.
use borea_core::{Hint, hint};
use std::{path::PathBuf, time::Duration};
pub fn agent(timeout: Duration) -> Result<ureq::Agent, Hint> {
    let mut paths = vec![PathBuf::from("/etc/ssl/certs/ca-certificates.crt")];
    for name in ["SSL_CERT_FILE", "CODEX_PROXY_CERT"] {
        if let Some(value) = std::env::var_os(name) {
            let path = PathBuf::from(value);
            if !path.is_file() {
                return Err(hint("tls", "The configured CA trust file is missing."));
            }
            paths.push(path);
        }
    }
    let mut certs = Vec::new();
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let data = std::fs::read_to_string(path)
            .map_err(|_| hint("tls", "The system CA bundle cannot be read."))?;
        for chunk in data.split_inclusive("-----END CERTIFICATE-----") {
            if let Some(start) = chunk.find("-----BEGIN CERTIFICATE-----") {
                certs.push(ureq::tls::Certificate::from_pem(&chunk.as_bytes()[start..]).map_err(
                    |_| hint("tls", "The system CA bundle contains an invalid certificate."),
                )?);
            }
        }
    }
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(if certs.is_empty() { ureq::tls::RootCerts::WebPki } else { certs.into() })
        .build();
    Ok(ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .proxy(ureq::Proxy::try_from_env())
        .tls_config(tls)
        .build()
        .into())
}
