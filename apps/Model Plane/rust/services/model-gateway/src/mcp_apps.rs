//! MCP Apps resource read (Wave 3 EP-4 S8).
//!
//! An MCP tool may name an HTML app that draws its result: the MCP Apps
//! extension puts a `ui://` resource uri in the tool's `_meta`, and the client
//! fetches that resource with `resources/read`. The SPA asks for the app by
//! server and tool name only; this module re-checks that the viewer may use the
//! server exactly as [`crate::runtime_registries::handle_proxy_mcp_tool`] does
//! (usable by this user, enabled, allowlisted, HTTPS transport), re-discovers
//! the tool to learn which uri it declares now, and reads that uri with the
//! viewer's authority (D10). Nothing is stored: a 60 s memory cache keyed by
//! (org, server, uri) spares a re-read while a transcript redraws.
//!
//! The uri is not part of the tool digest in v1 (D11): the app runs in the
//! opaque-origin sandbox with no network and no ambient authority, so a changed
//! uri changes only what is drawn. Network domains an app declares are reported
//! as `needs_network`, never honoured (D12): the frame's CSP stays as it is.
//!
//! Every extension key lives in this file, because the extension is young and
//! its `_meta` keys may still move (checked against the 2026-01-26 revision:
//! `_meta.ui.resourceUri` on the tool, the deprecated flat `_meta["ui/resourceUri"]`
//! still read, `text/html;profile=mcp-app` on the resource, `_meta.ui.csp` on
//! the read content).

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use base64::Engine as _;
use dashmap::DashMap;
use mp_contracts::model_plane::v1::{ListMcpServersRequest, McpServer};
use serde_json::Value;

use crate::ownership::OwnershipStore;
use crate::runtime_registries::McpRegistry;

/// The switch: only the literal `true` turns the route on.
pub const ENABLED_VAR: &str = "MCP_APPS_ENABLED";
/// The catalogue feature a zero-retention caller is refused with. An app read
/// egresses to the MCP server like a tool call, so it shares the tool call's key.
pub const ZDR_FEATURE: &str = "mcp_execution";
/// The only scheme an app resource may use; anything else is not an app.
pub const UI_SCHEME: &str = "ui://";
/// The largest app document served, in bytes of UTF-8.
pub const MAX_RESOURCE_BYTES: usize = 512 * 1024;
/// How long a read resource is reused for the same (org, server, uri).
pub const CACHE_TTL: Duration = Duration::from_secs(60);
/// Bound on cached documents per process, so the cache cannot grow without end.
const CACHE_MAX_ENTRIES: usize = 256;
/// Bound on a declared uri; a longer one is not treated as an app.
const MAX_URI_BYTES: usize = 2048;
/// Bound on the server id and tool name a caller may send.
pub const MAX_NAME_BYTES: usize = 256;

/// Where the tool names its app: `_meta.ui.resourceUri`.
const META_UI: &str = "ui";
const META_RESOURCE_URI: &str = "resourceUri";
/// The deprecated flat spelling, still sent by servers built on early drafts.
const META_LEGACY_RESOURCE_URI: &str = "ui/resourceUri";
/// The CSP declaration on the read content: `_meta.ui.csp`.
const META_CSP: &str = "csp";
/// Declared origins the frame would need network for. `baseUriDomains` asks for
/// no fetch, so it is not counted.
const CSP_NETWORK_KEYS: &[&str] = &["connectDomains", "resourceDomains", "frameDomains"];
/// The media type an app must have; a `profile` parameter is allowed.
const HTML_MIME: &str = "text/html";

/// `MCP_APPS_ENABLED`: only the literal `true` turns it on.
#[must_use]
pub fn enabled_from_env_value(value: Option<&str>) -> bool {
    value == Some("true")
}

/// The process's `MCP_APPS_ENABLED`, read once so it cannot change under a request.
#[must_use]
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| enabled_from_env_value(std::env::var(ENABLED_VAR).ok().as_deref()))
}

/// The `ui://` uri a raw `tools/list` entry names as its app, or `None` when it
/// names none, names another scheme, or names something malformed.
#[must_use]
pub fn ui_resource_for(tool: &Value) -> Option<String> {
    let meta = tool.get("_meta")?;
    let declared = meta
        .get(META_UI)
        .and_then(|ui| ui.get(META_RESOURCE_URI))
        .or_else(|| meta.get(META_LEGACY_RESOURCE_URI))
        .and_then(Value::as_str)?
        .trim();
    let accepted = declared.len() > UI_SCHEME.len()
        && declared.len() <= MAX_URI_BYTES
        && declared.starts_with(UI_SCHEME)
        && !declared
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace());
    accepted.then(|| declared.to_owned())
}

/// Whether a resource's media type is HTML (`text/html`, any parameters).
#[must_use]
pub fn is_html_mime(mime: &str) -> bool {
    mime.split(';')
        .next()
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case(HTML_MIME))
}

/// Whether a content's `_meta.ui.csp` declares any origin the app would need
/// network for. Reported to the viewer, never added to the frame's CSP.
#[must_use]
pub fn declares_network(meta: Option<&Value>) -> bool {
    let Some(csp) = meta
        .and_then(|meta| meta.get(META_UI))
        .and_then(|ui| ui.get(META_CSP))
    else {
        return false;
    };
    CSP_NETWORK_KEYS.iter().any(|key| {
        csp.get(*key)
            .and_then(Value::as_array)
            .is_some_and(|domains| {
                domains
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|domain| !domain.trim().is_empty())
            })
    })
}

/// One app document, as the route answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppResource {
    pub uri: String,
    pub html: String,
    pub needs_network: bool,
}

/// Why an app could not be read. Each maps to one typed error code; none
/// carries text from the MCP server, which is untrusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppResourceError {
    /// No such server for this viewer, or the tool is not usable, or it has no
    /// app. One code for all, so a viewer learns nothing about servers they
    /// cannot use.
    NotFound(&'static str),
    /// The document is larger than [`MAX_RESOURCE_BYTES`].
    TooLarge,
    /// The document is not UTF-8 `text/html`.
    UnsupportedMime,
    /// The server could not be reached, listed or read.
    Upstream,
}

impl AppResourceError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::TooLarge => "too_large",
            Self::UnsupportedMime => "unsupported_mime",
            Self::Upstream => "mcp_unavailable",
        }
    }

    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::NotFound(message) => message,
            Self::TooLarge => "the app document is larger than 512 KiB",
            Self::UnsupportedMime => "the app document is not UTF-8 text/html",
            Self::Upstream => "the MCP server could not be reached",
        }
    }

    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::TooLarge | Self::UnsupportedMime => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Upstream => StatusCode::BAD_GATEWAY,
        }
    }
}

const SERVER_NOT_FOUND: &str = "mcp server not found";
const TOOL_NOT_FOUND: &str = "the tool is not available on this server";
const NO_APP: &str = "the tool has no app";
const OTHER_RESOURCE: &str = "the server did not return the app the tool declares";

/// The server the viewer may read an app of: the same checks, in the same
/// order and with the same not-found answer for another user's private server,
/// as `handle_proxy_mcp_tool`. A disabled server and a tool outside the
/// allowlist also read as not found, since nothing of them may be drawn.
///
/// # Errors
/// [`AppResourceError::NotFound`] for every refusal.
pub fn authorize(
    reg: &McpRegistry,
    ownership: &OwnershipStore,
    org_id: &str,
    user_id: &str,
    server_id: &str,
    tool: &str,
) -> Result<McpServer, AppResourceError> {
    // An app is always read for a person; without one there is no authority to read with.
    if user_id.trim().is_empty()
        || !ownership.usable(org_id, crate::ownership::KIND_MCP, server_id, user_id)
    {
        return Err(AppResourceError::NotFound(SERVER_NOT_FOUND));
    }
    let server = crate::runtime_registries::handle_list_mcp_servers(
        reg,
        ListMcpServersRequest {
            request_id: String::new(),
            org_id: org_id.to_owned(),
        },
    )
    .map_err(|_| AppResourceError::NotFound(SERVER_NOT_FOUND))?
    .servers
    .into_iter()
    .find(|server| server.server_id == server_id)
    .ok_or(AppResourceError::NotFound(SERVER_NOT_FOUND))?;
    if !server.enabled {
        return Err(AppResourceError::NotFound(SERVER_NOT_FOUND));
    }
    if !server.tool_allowlist.iter().any(|allowed| allowed == tool) {
        return Err(AppResourceError::NotFound(TOOL_NOT_FOUND));
    }
    // Only the validated HTTPS transport is dispatched, as for tool calls; the
    // client's SSRF and scheme guard checks the url itself on connect.
    if server.transport != "http" {
        return Err(AppResourceError::NotFound(SERVER_NOT_FOUND));
    }
    Ok(server)
}

/// The uri `tool` declares in a fresh `tools/list`. A tool that is gone, or no
/// longer names a `ui://` app, has none to read.
///
/// # Errors
/// [`AppResourceError::NotFound`].
pub fn declared_uri(tools: &[Value], tool: &str) -> Result<String, AppResourceError> {
    let entry = tools
        .iter()
        .find(|entry| entry.get("name").and_then(Value::as_str) == Some(tool))
        .ok_or(AppResourceError::NotFound(TOOL_NOT_FOUND))?;
    ui_resource_for(entry).ok_or(AppResourceError::NotFound(NO_APP))
}

/// The app document in a `resources/read` result for `uri`: the content entry
/// for exactly that uri, HTML, within the size bound, valid UTF-8 (text or a
/// base64 blob).
///
/// # Errors
/// [`AppResourceError::NotFound`] when no entry is for `uri`;
/// [`AppResourceError::UnsupportedMime`] or [`AppResourceError::TooLarge`] otherwise.
pub fn app_resource_from_read(uri: &str, result: &Value) -> Result<AppResource, AppResourceError> {
    let content = result
        .get("contents")
        .and_then(Value::as_array)
        .and_then(|contents| {
            contents
                .iter()
                .find(|content| content.get("uri").and_then(Value::as_str) == Some(uri))
        })
        .ok_or(AppResourceError::NotFound(OTHER_RESOURCE))?;
    let mime = content
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !is_html_mime(mime) {
        return Err(AppResourceError::UnsupportedMime);
    }
    let html = if let Some(text) = content.get("text").and_then(Value::as_str) {
        if text.len() > MAX_RESOURCE_BYTES {
            return Err(AppResourceError::TooLarge);
        }
        text.to_owned()
    } else if let Some(blob) = content.get("blob").and_then(Value::as_str) {
        // Checked before decoding, so an oversized blob is never expanded.
        if blob.len() > MAX_RESOURCE_BYTES.div_ceil(3) * 4 {
            return Err(AppResourceError::TooLarge);
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(blob.trim())
            .map_err(|_| AppResourceError::UnsupportedMime)?;
        if bytes.len() > MAX_RESOURCE_BYTES {
            return Err(AppResourceError::TooLarge);
        }
        String::from_utf8(bytes).map_err(|_| AppResourceError::UnsupportedMime)?
    } else {
        return Err(AppResourceError::UnsupportedMime);
    };
    Ok(AppResource {
        uri: uri.to_owned(),
        html,
        needs_network: declares_network(content.get("_meta")),
    })
}

type CacheKey = (String, String, String);

/// The 60 s memory cache of read app documents, keyed by (org, server, uri) only:
/// the viewer's authority is checked on every request before it is consulted.
pub struct AppResourceCache {
    entries: DashMap<CacheKey, (Instant, Arc<AppResource>)>,
    ttl: Duration,
}

impl AppResourceCache {
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: DashMap::new(),
            ttl,
        }
    }

    /// The process's cache.
    pub fn global() -> &'static Self {
        static CACHE: OnceLock<AppResourceCache> = OnceLock::new();
        CACHE.get_or_init(|| Self::new(CACHE_TTL))
    }

    fn key(org_id: &str, server_id: &str, uri: &str) -> CacheKey {
        (org_id.to_owned(), server_id.to_owned(), uri.to_owned())
    }

    /// A fresh entry, or `None`; an expired entry is dropped on the way.
    #[must_use]
    pub fn get(&self, org_id: &str, server_id: &str, uri: &str) -> Option<Arc<AppResource>> {
        let key = Self::key(org_id, server_id, uri);
        let fresh = self
            .entries
            .get(&key)
            .and_then(|entry| (entry.0.elapsed() < self.ttl).then(|| Arc::clone(&entry.1)));
        if fresh.is_none() {
            self.entries.remove(&key);
        }
        fresh
    }

    /// Keep `resource`. When the cache is full, expired entries go first; if it
    /// is still full the document is simply not kept.
    pub fn insert(&self, org_id: &str, server_id: &str, resource: Arc<AppResource>) {
        if self.entries.len() >= CACHE_MAX_ENTRIES {
            let ttl = self.ttl;
            self.entries.retain(|_, entry| entry.0.elapsed() < ttl);
            if self.entries.len() >= CACHE_MAX_ENTRIES {
                return;
            }
        }
        let key = Self::key(org_id, server_id, &resource.uri);
        self.entries.insert(key, (Instant::now(), resource));
    }
}

/// Read the app `tool` declares on `server_id` for the viewer `user_id`:
/// [`authorize`], a fresh session and `tools/list` to learn the declared uri,
/// then the cache or `resources/read`. `oauth_token` is the server's stored
/// OAuth token when it has one, resolved as for tool calls.
///
/// Logs ids and sizes only, never the document.
///
/// # Errors
/// See [`AppResourceError`].
#[allow(clippy::too_many_arguments)]
pub async fn read_app_resource(
    reg: &McpRegistry,
    ownership: &OwnershipStore,
    cache: &AppResourceCache,
    org_id: &str,
    user_id: &str,
    server_id: &str,
    tool: &str,
    oauth_token: Option<&str>,
) -> Result<Arc<AppResource>, AppResourceError> {
    let server = authorize(reg, ownership, org_id, user_id, server_id, tool)?;
    let token = oauth_token
        .filter(|token| !token.is_empty())
        .unwrap_or(&server.token);
    let session = crate::mcp_http::McpHttpSession::connect(&server.url, token)
        .await
        .map_err(|error| upstream(server_id, "connect", &error))?;
    let tools = session
        .list_tools_raw()
        .await
        .map_err(|error| upstream(server_id, "tools/list", &error))?;
    let uri = declared_uri(&tools, tool)?;
    if let Some(hit) = cache.get(org_id, server_id, &uri) {
        return Ok(hit);
    }
    let result = session
        .read_resource(&uri)
        .await
        .map_err(|error| upstream(server_id, "resources/read", &error))?;
    let resource = Arc::new(app_resource_from_read(&uri, &result)?);
    tracing::debug!(
        %server_id,
        html_bytes = resource.html.len(),
        needs_network = resource.needs_network,
        "mcp apps: resource read"
    );
    cache.insert(org_id, server_id, Arc::clone(&resource));
    Ok(resource)
}

/// The typed upstream failure; the server's own text is logged by length only.
fn upstream(server_id: &str, step: &'static str, error: &str) -> AppResourceError {
    tracing::debug!(%server_id, step, error_len = error.len(), "mcp apps: upstream failure");
    AppResourceError::Upstream
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ownership::Ownership;
    use serde_json::json;

    const ORG: &str = "org-a";
    const SERVER: &str = "srv-1";

    fn app_tool(uri: &str) -> Value {
        json!({ "name": "show_chart", "_meta": { "ui": { "resourceUri": uri } } })
    }

    fn registry(enabled: bool, allowlist: &[&str]) -> McpRegistry {
        let reg = McpRegistry::new();
        reg.replace_org(
            ORG,
            [McpServer {
                server_id: SERVER.to_owned(),
                name: "Charts".to_owned(),
                url: "https://mcp.example.test".to_owned(),
                transport: "http".to_owned(),
                token: String::new(),
                tool_allowlist: allowlist.iter().map(|tool| (*tool).to_owned()).collect(),
                enabled,
            }],
        );
        reg
    }

    fn owned_by(user: &str) -> OwnershipStore {
        let ownership = OwnershipStore::new();
        ownership.set(
            ORG,
            crate::ownership::KIND_MCP,
            SERVER,
            Ownership::user(user),
        );
        ownership
    }

    fn html_read(uri: &str, text: &str, meta: Value) -> Value {
        json!({ "contents": [{
            "uri": uri, "mimeType": "text/html;profile=mcp-app", "text": text, "_meta": meta
        }] })
    }

    #[test]
    fn switch_parses_strictly() {
        assert!(enabled_from_env_value(Some("true")));
        for value in [
            None,
            Some(""),
            Some("TRUE"),
            Some("1"),
            Some(" true"),
            Some("yes"),
        ] {
            assert!(!enabled_from_env_value(value), "{value:?}");
        }
    }

    #[test]
    fn only_ui_scheme_is_accepted() {
        assert_eq!(
            ui_resource_for(&app_tool("ui://charts/bar")).as_deref(),
            Some("ui://charts/bar")
        );
        // The deprecated flat key is still read.
        let legacy = json!({ "name": "t", "_meta": { "ui/resourceUri": "ui://charts/pie" } });
        assert_eq!(ui_resource_for(&legacy).as_deref(), Some("ui://charts/pie"));
        for uri in [
            "https://evil.example/app.html",
            "file:///etc/passwd",
            "data:text/html,<p>x</p>",
            "ui://",
            "UI://charts/bar",
            "ui://charts/a b",
            "ui://charts/\nbar",
        ] {
            assert_eq!(ui_resource_for(&app_tool(uri)), None, "{uri:?}");
        }
        assert_eq!(ui_resource_for(&json!({ "name": "plain" })), None);
        assert_eq!(
            ui_resource_for(&app_tool(&format!("ui://{}", "a".repeat(MAX_URI_BYTES)))),
            None
        );
    }

    #[test]
    fn other_users_private_server_is_not_found() {
        let reg = registry(true, &["show_chart"]);
        let ownership = owned_by("user-b");
        assert_eq!(
            authorize(&reg, &ownership, ORG, "user-a", SERVER, "show_chart"),
            Err(AppResourceError::NotFound(SERVER_NOT_FOUND))
        );
        assert!(authorize(&reg, &ownership, ORG, "user-b", SERVER, "show_chart").is_ok());
        // Another org's viewer, and a request with no viewer, see nothing either.
        assert!(authorize(&reg, &ownership, "org-b", "user-b", SERVER, "show_chart").is_err());
        assert!(authorize(&reg, &ownership, ORG, " ", SERVER, "show_chart").is_err());
    }

    #[test]
    fn tool_not_in_allowlist_is_refused() {
        let reg = registry(true, &["other_tool"]);
        assert_eq!(
            authorize(
                &reg,
                &owned_by("user-a"),
                ORG,
                "user-a",
                SERVER,
                "show_chart"
            ),
            Err(AppResourceError::NotFound(TOOL_NOT_FOUND))
        );
    }

    #[test]
    fn disabled_server_is_not_found() {
        let reg = registry(false, &["show_chart"]);
        assert_eq!(
            authorize(
                &reg,
                &owned_by("user-a"),
                ORG,
                "user-a",
                SERVER,
                "show_chart"
            ),
            Err(AppResourceError::NotFound(SERVER_NOT_FOUND))
        );
    }

    #[test]
    fn uri_not_declared_by_the_tool_is_refused() {
        let tools = vec![app_tool("ui://charts/bar"), json!({ "name": "plain" })];
        assert_eq!(
            declared_uri(&tools, "show_chart").as_deref(),
            Ok("ui://charts/bar")
        );
        assert_eq!(
            declared_uri(&tools, "plain"),
            Err(AppResourceError::NotFound(NO_APP))
        );
        assert_eq!(
            declared_uri(&tools, "gone"),
            Err(AppResourceError::NotFound(TOOL_NOT_FOUND))
        );
        // The server answers with a resource the tool did not declare.
        let read = html_read("ui://charts/other", "<p>x</p>", Value::Null);
        assert_eq!(
            app_resource_from_read("ui://charts/bar", &read),
            Err(AppResourceError::NotFound(OTHER_RESOURCE))
        );
    }

    #[test]
    fn oversized_resource_is_refused() {
        let big = "a".repeat(MAX_RESOURCE_BYTES + 1);
        let read = html_read("ui://a", &big, Value::Null);
        assert_eq!(
            app_resource_from_read("ui://a", &read),
            Err(AppResourceError::TooLarge)
        );
        let fits = "a".repeat(MAX_RESOURCE_BYTES);
        assert!(app_resource_from_read("ui://a", &html_read("ui://a", &fits, Value::Null)).is_ok());
        let blob = base64::engine::general_purpose::STANDARD.encode(big.as_bytes());
        let read =
            json!({ "contents": [{ "uri": "ui://a", "mimeType": "text/html", "blob": blob }] });
        assert_eq!(
            app_resource_from_read("ui://a", &read),
            Err(AppResourceError::TooLarge)
        );
    }

    #[test]
    fn non_html_is_refused() {
        for mime in ["application/javascript", "text/plain", "image/svg+xml", ""] {
            let read = json!({ "contents": [{ "uri": "ui://a", "mimeType": mime, "text": "x" }] });
            assert_eq!(
                app_resource_from_read("ui://a", &read),
                Err(AppResourceError::UnsupportedMime),
                "{mime:?}"
            );
        }
        // A blob that is not UTF-8 is not an HTML document either.
        let blob = base64::engine::general_purpose::STANDARD.encode([0xff_u8, 0xfe, 0x00]);
        let read =
            json!({ "contents": [{ "uri": "ui://a", "mimeType": "text/html", "blob": blob }] });
        assert_eq!(
            app_resource_from_read("ui://a", &read),
            Err(AppResourceError::UnsupportedMime)
        );
        // A UTF-8 blob is decoded.
        let blob = base64::engine::general_purpose::STANDARD.encode("<p>Hei</p>");
        let read = json!({ "contents": [{ "uri": "ui://a", "mimeType": "TEXT/HTML; charset=utf-8", "blob": blob }] });
        assert_eq!(
            app_resource_from_read("ui://a", &read).map(|resource| resource.html),
            Ok("<p>Hei</p>".to_owned())
        );
    }

    #[test]
    fn network_domains_are_reported_not_honoured() {
        let html = "<p>app</p>";
        let meta = json!({ "ui": { "csp": { "connectDomains": ["https://api.example.test"] } } });
        let resource =
            app_resource_from_read("ui://a", &html_read("ui://a", html, meta)).expect("read");
        assert!(resource.needs_network);
        // The document is passed on unchanged: nothing of the declaration reaches it.
        assert_eq!(resource.html, html);
        let none = app_resource_from_read(
            "ui://a",
            &html_read("ui://a", html, json!({ "ui": { "csp": { "connectDomains": [], "baseUriDomains": ["https://x.test"] } } })),
        )
        .expect("read");
        assert!(!none.needs_network);
        let plain = app_resource_from_read("ui://a", &html_read("ui://a", html, Value::Null))
            .expect("read");
        assert!(!plain.needs_network);
    }

    #[test]
    fn cache_is_keyed_by_org_server_and_uri_and_expires() {
        let cache = AppResourceCache::new(CACHE_TTL);
        let resource = Arc::new(AppResource {
            uri: "ui://a".to_owned(),
            html: "<p>x</p>".to_owned(),
            needs_network: false,
        });
        cache.insert(ORG, SERVER, Arc::clone(&resource));
        assert_eq!(cache.get(ORG, SERVER, "ui://a"), Some(resource.clone()));
        assert_eq!(cache.get("org-b", SERVER, "ui://a"), None);
        assert_eq!(cache.get(ORG, "srv-2", "ui://a"), None);
        assert_eq!(cache.get(ORG, SERVER, "ui://b"), None);
        let expired = AppResourceCache::new(Duration::ZERO);
        expired.insert(ORG, SERVER, resource);
        assert_eq!(expired.get(ORG, SERVER, "ui://a"), None);
    }

    #[test]
    fn errors_map_to_typed_codes() {
        assert_eq!(
            AppResourceError::NotFound(NO_APP).status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(AppResourceError::TooLarge.code(), "too_large");
        assert_eq!(AppResourceError::UnsupportedMime.code(), "unsupported_mime");
        assert_eq!(AppResourceError::Upstream.status(), StatusCode::BAD_GATEWAY);
    }
}
