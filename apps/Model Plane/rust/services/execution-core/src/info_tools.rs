//! Real-time information tools for the agentic loop — Norwegian read-only
//! lookups backed by the Application Plane `information-core` service plus the
//! public Brønnøysund Enhetsregisteret (company registry).
//!
//! These are the first NON-research read tools in the governed multi-tool loop:
//! `yr_weather`, `traffic`, `news`, `track_shipment` (all via information-core's
//! internal HTTP surface, guarded by `x-internal-api-key`), and
//! `company_lookup` (the public, unauthenticated Brønnøysund API — only the open
//! distribution, which carries no personal data).
//!
//! information-core wraps several external public APIs (Yr/met.no, Statens
//! Vegvesen, RSS news, Bring tracking) and exposes a single internal surface, so
//! — like `web_tools` and `model-gateway`'s Fetch client — responses are parsed
//! defensively as `serde_json::Value` (the upstream DTOs live in a separate
//! TypeScript service and shift with the wrapped providers) rather than mirrored
//! structs. All five tools are read-only (none match `permission::is_risky_tool`),
//! so they run under `ask` posture without an approval gate.
//!
//! Transport:
//!   GET {INFORMATION_CORE_URL}/api/v1/weather?lat&lon  → Yr forecast
//!   GET {INFORMATION_CORE_URL}/api/v1/traffic?lat&lon&radius → SVV stations
//!   GET {INFORMATION_CORE_URL}/api/v1/news?category&limit   → RSS articles
//!   GET {INFORMATION_CORE_URL}/api/v1/shipping/track?trackingNumber → Bring
//!   GET {BRREG_API_URL}/enheter?navn=… | /enheter/{orgnr}   → company registry

// `# Errors` prose for the obvious `Result<String, String>` helpers is noise;
// `doc_markdown` over-flags wire tokens. Low-signal pedantic lints.
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use std::fmt::Write as _;
use std::time::Duration;

use serde_json::Value;

/// Default information-core address (compose service DNS name + port 3190).
const DEFAULT_INFO_CORE_URL: &str = "http://information-core:3190";

/// Default public Brønnøysund Enhetsregisteret API base.
const DEFAULT_BRREG_API_URL: &str = "https://data.brreg.no/enhetsregisteret/api";

/// Cap on how many news articles / company matches we render to the model.
const MAX_NEWS_ARTICLES: usize = 8;
const MAX_COMPANY_MATCHES: usize = 8;
const MAX_TRACKING_EVENTS: usize = 6;
const MAX_TEXT_CHARS: usize = 280;

/// HTTP client for information-core + the public company registry. Cheap to
/// clone.
#[derive(Clone)]
pub struct InfoToolsClient {
    base_url: String,
    brreg_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl InfoToolsClient {
    /// Build from the environment. Reads `INFORMATION_CORE_URL` (falling back to
    /// `INFORMATION_CORE_ADDR`, then a compose default) for the internal surface
    /// and `INFORMATION_CORE_INTERNAL_KEY` for the `x-internal-api-key` header.
    /// `BRREG_API_URL` overrides the public registry base. Returns `None` only
    /// when the reqwest client itself cannot be built (so callers surface a
    /// clear "not configured" error rather than echoing).
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("INFORMATION_CORE_URL")
            .or_else(|_| std::env::var("INFORMATION_CORE_ADDR"))
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_INFO_CORE_URL.to_owned());
        let brreg_url = std::env::var("BRREG_API_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BRREG_API_URL.to_owned());
        let api_key = std::env::var("INFORMATION_CORE_INTERNAL_KEY").unwrap_or_default();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .ok()?;
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            brreg_url: brreg_url.trim_end_matches('/').to_owned(),
            api_key,
            http,
        })
    }

    /// GET an information-core path with the internal-api-key header set,
    /// returning the decoded JSON body. `query` pairs are URL-encoded by reqwest.
    async fn get_info(&self, path: &str, query: &[(&str, String)]) -> Result<Value, String> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base_url))
            .header("x-internal-api-key", self.api_key.as_str())
            .query(query)
            .send()
            .await
            .map_err(|e| format!("information-core {path} request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("information-core {path} decode failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "information-core {path} returned {status}: {value}"
            ));
        }
        Ok(value)
    }

    /// GET a public Brønnøysund path (no auth), returning the decoded JSON body.
    async fn get_brreg(&self, path: &str, query: &[(&str, String)]) -> Result<Value, String> {
        let resp = self
            .http
            .get(format!("{}{path}", self.brreg_url))
            .header("accept", "application/json")
            .query(query)
            .send()
            .await
            .map_err(|e| format!("brreg {path} request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("brreg {path} decode failed: {e}"))?;
        if !status.is_success() {
            return Err(format!("brreg {path} returned {status}: {value}"));
        }
        Ok(value)
    }

    /// `yr_weather` → GET `/api/v1/weather?lat&lon`. Returns a compact forecast
    /// summary for the coordinate.
    pub async fn yr_weather(&self, lat: f64, lon: f64) -> Result<String, String> {
        let value = self
            .get_info(
                "/api/v1/weather",
                &[("lat", lat.to_string()), ("lon", lon.to_string())],
            )
            .await?;
        Ok(format_weather(lat, lon, &value))
    }

    /// `traffic` → GET `/api/v1/traffic?lat&lon&radius`. Returns nearby traffic
    /// registration stations with volume/speed where available.
    pub async fn traffic(&self, lat: f64, lon: f64, radius: u32) -> Result<String, String> {
        let value = self
            .get_info(
                "/api/v1/traffic",
                &[
                    ("lat", lat.to_string()),
                    ("lon", lon.to_string()),
                    ("radius", radius.to_string()),
                ],
            )
            .await?;
        Ok(format_traffic(lat, lon, &value))
    }

    /// `news` → GET `/api/v1/news?category&limit`. Returns the latest normalised
    /// articles. An empty `category` requests all feeds.
    pub async fn news(&self, category: &str, limit: u32) -> Result<String, String> {
        let mut query = vec![("limit", limit.to_string())];
        if !category.trim().is_empty() {
            query.push(("category", category.trim().to_owned()));
        }
        let value = self.get_info("/api/v1/news", &query).await?;
        Ok(format_news(category, &value))
    }

    /// `track_shipment` → GET `/api/v1/shipping/track?trackingNumber`. Returns
    /// the current carrier status + recent event history.
    pub async fn track_shipment(&self, tracking_number: &str) -> Result<String, String> {
        if tracking_number.trim().is_empty() {
            return Err("track_shipment requires a non-empty tracking number".to_owned());
        }
        let value = self
            .get_info(
                "/api/v1/shipping/track",
                &[("trackingNumber", tracking_number.trim().to_owned())],
            )
            .await?;
        Ok(format_tracking(tracking_number, &value))
    }

    /// `company_lookup` → public Brønnøysund Enhetsregisteret. A 9-digit input is
    /// treated as an organisation number (`/enheter/{orgnr}`); anything else is a
    /// name search (`/enheter?navn=…`). Returns name + org.nr + address + status.
    pub async fn company_lookup(&self, query: &str) -> Result<String, String> {
        let q = query.trim();
        if q.is_empty() {
            return Err("company_lookup requires a non-empty query".to_owned());
        }
        if is_org_number(q) {
            let value = self.get_brreg(&format!("/enheter/{q}"), &[]).await?;
            Ok(format_company_entity(&value))
        } else {
            let value = self
                .get_brreg("/enheter", &[("navn", q.to_owned())])
                .await?;
            Ok(format_company_search(q, &value))
        }
    }
}

/// True when `s` is exactly 9 ASCII digits (a Norwegian organisation number).
fn is_org_number(s: &str) -> bool {
    s.len() == 9 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Format a Yr `/weather` response into agent-readable text (pure; testable
/// without a live service). Parses defensively: information-core's exact shape
/// follows the wrapped Yr provider, so several plausible keys are accepted.
fn format_weather(lat: f64, lon: f64, value: &Value) -> String {
    let temp = pick_number(
        value,
        &["temperature", "temp", "airTemperature", "air_temperature"],
    );
    let wind = pick_number(value, &["windSpeed", "wind_speed", "wind"]);
    let symbol = pick_string(value, &["symbol", "symbolCode", "summary", "condition"]);
    let mut out = format!("Weather for ({lat:.4}, {lon:.4}):");
    let mut any = false;
    if let Some(t) = temp {
        let _ = write!(out, " {t:.1}°C");
        any = true;
    }
    if !symbol.is_empty() {
        let _ = write!(out, " — {symbol}");
        any = true;
    }
    if let Some(w) = wind {
        let _ = write!(out, " (wind {w:.1} m/s)");
        any = true;
    }
    if any {
        out
    } else {
        format!("Weather lookup for ({lat:.4}, {lon:.4}) returned no readable forecast fields.")
    }
}

/// Format an SVV `/traffic` response into agent-readable text (pure).
fn format_traffic(lat: f64, lon: f64, value: &Value) -> String {
    let stations = pick_array(
        value,
        &["stations", "trafficRegistrationPoints", "results", "data"],
    );
    if stations.is_empty() {
        return format!("No traffic stations found near ({lat:.4}, {lon:.4}).");
    }
    let mut out = format!(
        "Traffic stations near ({lat:.4}, {lon:.4}) ({} found):\n",
        stations.len()
    );
    for (i, s) in stations.iter().take(MAX_COMPANY_MATCHES).enumerate() {
        let name = pick_string(s, &["name", "stationName", "id"]);
        let volume = pick_number(s, &["volume", "trafficVolume", "count"]);
        let speed = pick_number(s, &["speed", "averageSpeed", "meanSpeed"]);
        let _ = write!(
            out,
            "{}. {}",
            i + 1,
            if name.is_empty() { "(unnamed)" } else { &name }
        );
        if let Some(v) = volume {
            let _ = write!(out, " — volume {v:.0}");
        }
        if let Some(sp) = speed {
            let _ = write!(out, ", avg speed {sp:.0} km/h");
        }
        out.push('\n');
    }
    out
}

/// Format a `/news` response into agent-readable text (pure).
fn format_news(category: &str, value: &Value) -> String {
    let articles = pick_array(value, &["articles", "items", "results", "data"]);
    let scope = if category.trim().is_empty() {
        "latest news".to_owned()
    } else {
        format!("news in \"{}\"", category.trim())
    };
    if articles.is_empty() {
        return format!("No {scope} available right now.");
    }
    let mut out = format!("Top {scope} ({} articles):\n", articles.len());
    for (i, a) in articles.iter().take(MAX_NEWS_ARTICLES).enumerate() {
        let title = pick_string(a, &["title", "headline", "name"]);
        let source = pick_string(a, &["source", "feed", "publisher"]);
        let link = pick_string(a, &["link", "url", "guid"]);
        let title = if title.is_empty() {
            "(untitled)".to_owned()
        } else {
            truncate_chars(&title, MAX_TEXT_CHARS)
        };
        let _ = write!(out, "{}. {title}", i + 1);
        if !source.is_empty() {
            let _ = write!(out, " [{source}]");
        }
        if !link.is_empty() {
            let _ = write!(out, "\n   {link}");
        }
        out.push('\n');
    }
    out
}

/// Format a Bring `/shipping/track` response into agent-readable text (pure).
fn format_tracking(tracking_number: &str, value: &Value) -> String {
    let status = pick_string(value, &["status", "statusCode"]);
    let description = pick_string(value, &["description"]);
    let carrier = pick_string(value, &["carrier"]);
    let eta = pick_string(value, &["estimatedDelivery", "estimated_delivery"]);
    let mut out = format!("Shipment {tracking_number}");
    if !carrier.is_empty() {
        let _ = write!(out, " ({carrier})");
    }
    let _ = write!(
        out,
        ": {}",
        if status.is_empty() {
            "status unknown"
        } else {
            &status
        }
    );
    if !description.is_empty() {
        let _ = write!(out, " — {description}");
    }
    if !eta.is_empty() {
        let _ = write!(out, ". Estimated delivery: {eta}");
    }
    let events = pick_array(value, &["events", "history"]);
    if !events.is_empty() {
        out.push_str("\nRecent events:\n");
        for e in events.iter().take(MAX_TRACKING_EVENTS) {
            let ts = pick_string(e, &["timestamp", "time", "date"]);
            let desc = pick_string(e, &["description", "status"]);
            let loc = pick_string(e, &["location", "place"]);
            let _ = write!(out, "  - {ts} {desc}");
            if !loc.is_empty() {
                let _ = write!(out, " @ {loc}");
            }
            out.push('\n');
        }
    }
    out
}

/// Format a Brønnøysund `/enheter` search list into agent-readable text (pure).
/// The list lives under `_embedded.enheter` per the registry's HAL response.
fn format_company_search(query: &str, value: &Value) -> String {
    let entities = value
        .get("_embedded")
        .and_then(|e| e.get("enheter"))
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| value.get("enheter").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    if entities.is_empty() {
        return format!("No companies found in Brønnøysund matching \"{query}\".");
    }
    let mut out = format!(
        "Companies matching \"{query}\" ({} found):\n",
        entities.len()
    );
    for (i, e) in entities.iter().take(MAX_COMPANY_MATCHES).enumerate() {
        let _ = writeln!(out, "{}. {}", i + 1, company_line(e));
    }
    out
}

/// Format a single Brønnøysund `/enheter/{orgnr}` entity into agent-readable
/// text (pure).
fn format_company_entity(value: &Value) -> String {
    company_line(value)
}

/// Render one registry entity as `Name (org.nr NNNNNNNNN) — address [status]`.
fn company_line(e: &Value) -> String {
    let name = pick_string(e, &["navn", "name"]);
    let orgnr = pick_string(e, &["organisasjonsnummer", "orgnr"]);
    let mut line = if name.is_empty() {
        "(unnamed)".to_owned()
    } else {
        name
    };
    if !orgnr.is_empty() {
        let _ = write!(line, " (org.nr {orgnr})");
    }
    let address = format_brreg_address(e);
    if !address.is_empty() {
        let _ = write!(line, " — {address}");
    }
    // `slettedato` (deletion date) present ⇒ dissolved; otherwise active.
    let status = if e.get("slettedato").is_some_and(|v| !v.is_null()) {
        "dissolved"
    } else {
        "active"
    };
    let _ = write!(line, " [{status}]");
    line
}

/// Pull a readable single-line address out of a registry entity's
/// `forretningsadresse` (business address) block.
fn format_brreg_address(e: &Value) -> String {
    let addr = e
        .get("forretningsadresse")
        .or_else(|| e.get("beliggenhetsadresse"));
    let Some(addr) = addr else {
        return String::new();
    };
    let street = addr
        .get("adresse")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let postal = pick_string(addr, &["postnummer"]);
    let city = pick_string(addr, &["poststed"]);
    let mut parts = Vec::new();
    if !street.is_empty() {
        parts.push(street);
    }
    let pc = format!("{postal} {city}").trim().to_owned();
    if !pc.is_empty() {
        parts.push(pc);
    }
    parts.join(", ")
}

/// First string value among `keys` (top-level), or empty.
fn pick_string(value: &Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(s) = value.get(k).and_then(Value::as_str) {
            if !s.is_empty() {
                return s.to_owned();
            }
        }
    }
    String::new()
}

/// First numeric value among `keys` (top-level), or `None`. Accepts JSON numbers
/// and numeric strings.
fn pick_number(value: &Value, keys: &[&str]) -> Option<f64> {
    for k in keys {
        match value.get(k) {
            Some(Value::Number(n)) => return n.as_f64(),
            Some(Value::String(s)) => {
                if let Ok(n) = s.parse::<f64>() {
                    return Some(n);
                }
            }
            _ => {}
        }
    }
    None
}

/// First array value among `keys` (top-level), or an empty vec.
fn pick_array(value: &Value, keys: &[&str]) -> Vec<Value> {
    for k in keys {
        if let Some(a) = value.get(k).and_then(Value::as_array) {
            return a.clone();
        }
    }
    Vec::new()
}

/// Char-boundary-safe truncation (never panics on multibyte input).
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn org_number_detection() {
        assert!(is_org_number("123456789"));
        assert!(!is_org_number("12345678")); // 8 digits
        assert!(!is_org_number("Equinor ASA"));
        assert!(!is_org_number("12345678a"));
    }

    #[test]
    fn weather_renders_temp_symbol_and_wind() {
        let v = json!({ "temperature": 12.3, "symbol": "cloudy", "windSpeed": 4.5 });
        let out = format_weather(59.91, 10.75, &v);
        assert!(out.contains("12.3°C"));
        assert!(out.contains("cloudy"));
        assert!(out.contains("wind 4.5 m/s"));
    }

    #[test]
    fn weather_accepts_alternate_keys_and_numeric_strings() {
        let v = json!({ "air_temperature": "7", "condition": "rain" });
        let out = format_weather(60.0, 5.0, &v);
        assert!(out.contains("7.0°C"));
        assert!(out.contains("rain"));
    }

    #[test]
    fn weather_with_no_fields_is_informative() {
        let out = format_weather(1.0, 2.0, &json!({ "unrelated": true }));
        assert!(out.contains("no readable forecast"));
    }

    #[test]
    fn traffic_lists_stations_with_metrics() {
        let v = json!({ "stations": [
            { "name": "E6 Klemetsrud", "volume": 42000, "speed": 78 },
            { "stationName": "Rv4 Nittedal" }
        ]});
        let out = format_traffic(59.8, 10.9, &v);
        assert!(out.contains("2 found"));
        assert!(out.contains("1. E6 Klemetsrud"));
        assert!(out.contains("volume 42000"));
        assert!(out.contains("avg speed 78 km/h"));
        assert!(out.contains("2. Rv4 Nittedal"));
    }

    #[test]
    fn traffic_empty_is_informative() {
        let out = format_traffic(0.0, 0.0, &json!({ "stations": [] }));
        assert!(out.contains("No traffic stations"));
    }

    #[test]
    fn news_renders_articles_with_source_and_link() {
        let v = json!({ "articles": [
            { "title": "Quarterly results", "source": "E24", "link": "https://e24.no/x" },
            { "headline": "Food safety alert" }
        ]});
        let out = format_news("business", &v);
        assert!(out.contains("news in \"business\""));
        assert!(out.contains("1. Quarterly results"));
        assert!(out.contains("[E24]"));
        assert!(out.contains("https://e24.no/x"));
        assert!(out.contains("2. Food safety alert"));
    }

    #[test]
    fn news_empty_is_informative() {
        let out = format_news("", &json!({ "articles": [] }));
        assert!(out.contains("No latest news"));
    }

    #[test]
    fn tracking_renders_status_eta_and_events() {
        let v = json!({
            "trackingNumber": "370000000000000000",
            "status": "In transit",
            "description": "Package is on its way",
            "carrier": "Bring/Posten",
            "estimatedDelivery": "2026-06-15",
            "events": [
                { "timestamp": "2026-06-13T10:00:00", "description": "Loaded on vehicle", "location": "Oslo, NO" }
            ]
        });
        let out = format_tracking("370000000000000000", &v);
        assert!(out.contains("(Bring/Posten)"));
        assert!(out.contains("In transit"));
        assert!(out.contains("Package is on its way"));
        assert!(out.contains("Estimated delivery: 2026-06-15"));
        assert!(out.contains("Loaded on vehicle"));
        assert!(out.contains("@ Oslo, NO"));
    }

    #[test]
    fn tracking_unknown_status_is_safe() {
        let out = format_tracking("ABC", &json!({}));
        assert!(out.contains("status unknown"));
    }

    #[test]
    fn company_search_renders_name_orgnr_address_status() {
        let v = json!({ "_embedded": { "enheter": [
            {
                "navn": "EQUINOR ASA",
                "organisasjonsnummer": "923609016",
                "forretningsadresse": {
                    "adresse": ["Forusbeen 50"],
                    "postnummer": "4035",
                    "poststed": "STAVANGER"
                }
            }
        ]}});
        let out = format_company_search("equinor", &v);
        assert!(out.contains("1. EQUINOR ASA"));
        assert!(out.contains("org.nr 923609016"));
        assert!(out.contains("Forusbeen 50"));
        assert!(out.contains("4035 STAVANGER"));
        assert!(out.contains("[active]"));
    }

    #[test]
    fn company_search_empty_is_informative() {
        let out = format_company_search("zzz", &json!({ "_embedded": { "enheter": [] } }));
        assert!(out.contains("No companies found"));
    }

    #[test]
    fn company_entity_marks_dissolved() {
        let v = json!({
            "navn": "GAMMEL AS",
            "organisasjonsnummer": "999888777",
            "slettedato": "2020-01-01"
        });
        let out = format_company_entity(&v);
        assert!(out.contains("GAMMEL AS"));
        assert!(out.contains("org.nr 999888777"));
        assert!(out.contains("[dissolved]"));
    }
}
