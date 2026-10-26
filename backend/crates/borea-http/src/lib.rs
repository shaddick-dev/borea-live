#![forbid(unsafe_code)]
//! Shared proxy and system CA trust, with TLS verification always enabled.
use borea_core::{Hint, hint};
use std::{path::PathBuf, time::Duration};
const SYSTEM_BUNDLES: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
    "/etc/ssl/ca-bundle.pem",
    "/var/lib/ca-certificates/ca-bundle.pem",
];
pub fn system_bundle() -> Result<PathBuf, Hint> {
    if let Some(path) = std::env::var_os("SSL_CERT_FILE") {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(hint("tls", "The configured CA trust file is missing."))
        };
    }
    SYSTEM_BUNDLES
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .ok_or_else(|| hint("tls", "The live environment needs a system CA certificate bundle."))
}
pub fn agent(timeout: Duration) -> Result<ureq::Agent, Hint> {
    let mut paths = SYSTEM_BUNDLES.iter().map(PathBuf::from).collect::<Vec<_>>();
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
