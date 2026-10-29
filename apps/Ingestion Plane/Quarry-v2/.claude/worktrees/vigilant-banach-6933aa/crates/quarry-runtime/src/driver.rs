//! Driver selection. Static vs browser vs TLS-profile fetch.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_tls::TlsProfile;

use crate::fetch::FetchResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverSelection {
    pub requested: Option<DriverKind>,
    pub chosen: DriverKind,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BrowserMeta {
    pub session_id: Option<String>,
    pub live_view_url: Option<String>,
    pub recording_id: Option<String>,
}

#[async_trait]
pub trait Driver: Send + Sync {
    fn kind(&self) -> DriverKind;
    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse>;
    fn tls_profile(&self) -> Option<TlsProfile> { None }
    fn browser_meta(&self) -> Option<BrowserMeta> { None }
}

/// Select driver by url + hints. Simple rule for scaffold:
/// - explicit hint wins
/// - otherwise `static`
/// - upgrade to `browser` if hostname is on JS-required list (populated later)
pub fn select(url: &Url, hint: Option<DriverKind>, js_required_hosts: &[&str]) -> DriverSelection {
    if let Some(k) = hint {
        return DriverSelection {
            requested: Some(k),
            chosen: k,
            reasons: vec!["explicit hint".into()],
        };
    }
    let host = url.host_str().unwrap_or("");
    if js_required_hosts.iter().any(|h| host.ends_with(h)) {
        return DriverSelection {
            requested: None,
            chosen: DriverKind::Browser,
            reasons: vec![format!("host requires js: {host}")],
        };
    }
    DriverSelection {
        requested: None,
        chosen: DriverKind::Static,
        reasons: vec!["default static fetch".into()],
    }
}
