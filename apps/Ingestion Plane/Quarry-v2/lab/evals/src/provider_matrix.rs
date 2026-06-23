//! quarry-provider-matrix — provider fingerprint acceptance harness (QRY-01).
//!
//! Captures TLS/HTTP2/UA fingerprints from each configured browser provider
//! against well-known fingerprint endpoints, then writes a matrix JSON the
//! release process diffs across runs. Detects fingerprint regressions before
//! they hit production.
//!
//! ## Modes
//!
//! - **Offline (default, no creds)** — emits a placeholder matrix with
//!   `provider_count = 0` so CI doesn't fail. Returns immediately.
//! - **Live (env-gated)** — when env vars hold provider creds, the harness
//!   actually requests a session against each provider's `endpoint` (defaults
//!   to https://tls.peet.ws/api/all) and records the captured fingerprint.
//!
//! ## Inputs
//!
//! `PROVIDERS_PATH` env or default `lab/evals/provider_matrix.json`:
//!
//! ```json
//! {
//!   "endpoints": ["https://tls.peet.ws/api/all"],
//!   "providers": [
//!     {"name": "static_chrome", "kind": "static", "tls_profile": "chrome"},
//!     {"name": "browserless",   "kind": "browserless", "url": "https://...", "token_env": "BROWSERLESS_TOKEN"},
//!     {"name": "browserbase",   "kind": "browserbase",  "api_key_env": "BROWSERBASE_API_KEY"},
//!     {"name": "kernel",        "kind": "kernel",       "url": "https://api.kernel.so", "api_key_env": "KERNEL_API_KEY"}
//!   ]
//! }
//! ```
//!
//! Provider creds are NEVER hard-coded in the JSON — `*_env` fields name
//! the environment variable to read at runtime.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Default)]
struct Config {
    #[serde(default)]
    endpoints: Vec<String>,
    #[serde(default)]
    providers: Vec<Provider>,
}

#[derive(Debug, Deserialize, Clone)]
struct Provider {
    name: String,
    kind: String,
    #[serde(default)]
    tls_profile: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    api_key_env: Option<String>,
    #[serde(default)]
    token_env: Option<String>,
}

#[derive(Debug, Serialize)]
struct Matrix {
    generated_at: String,
    endpoints: Vec<String>,
    rows: Vec<MatrixRow>,
    notes: Vec<String>,
    summary: MatrixSummary,
}

#[derive(Debug, Serialize)]
struct MatrixSummary {
    provider_count: usize,
    captured_count: usize,
    skipped_count: usize,
    distinct_ja3: usize,
    distinct_ja4: usize,
}

#[derive(Debug, Serialize, Clone)]
struct MatrixRow {
    provider: String,
    kind: String,
    tls_profile: Option<String>,
    captures: Vec<EndpointCapture>,
    note: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct EndpointCapture {
    endpoint: String,
    ja3: Option<String>,
    ja3_hash: Option<String>,
    ja4: Option<String>,
    h2_fp: Option<String>,
    user_agent: Option<String>,
    status: u16,
    latency_ms: u64,
    error: Option<String>,
}

fn config_path() -> Option<PathBuf> {
    let candidates = [
        env::var("PROVIDERS_PATH").ok().map(PathBuf::from),
        Some(PathBuf::from("lab/evals/provider_matrix.json")),
        Some(PathBuf::from("provider_matrix.json")),
    ];
    candidates.into_iter().flatten().find(|p| p.exists())
}

fn load_config() -> Config {
    if let Some(path) = config_path() {
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(e) => {
                eprintln!("warn: failed to read {}: {e}", path.display());
                Config::default()
            }
        }
    } else {
        Config::default()
    }
}

fn main() {
    let cfg = load_config();
    let endpoints = if cfg.endpoints.is_empty() {
        vec!["https://tls.peet.ws/api/all".to_string()]
    } else {
        cfg.endpoints.clone()
    };

    let mut rows: Vec<MatrixRow> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    if cfg.providers.is_empty() {
        notes.push(
            "no providers configured — set PROVIDERS_PATH or write provider_matrix.json"
                .to_string(),
        );
    }

    // Use multi-threaded runtime so we can capture across providers and
    // endpoints concurrently. Live endpoints (tls.peet.ws,
    // browserleaks.com) regularly take 1-3s; serial capture for 4
    // providers × 2 endpoints = up to 24s. Parallel cuts that to ~3s.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("tokio runtime");

    let captured_rows: Vec<MatrixRow> = runtime.block_on(async {
        let mut row_futures = Vec::new();
        for provider in &cfg.providers {
            let provider = provider.clone();
            let endpoints = endpoints.clone();
            row_futures.push(async move {
                // For each provider, fan out its endpoint captures concurrently.
                let endpoint_futures: Vec<_> = endpoints
                    .iter()
                    .map(|endpoint| capture_one(&provider, endpoint))
                    .collect();
                let captures: Vec<EndpointCapture> =
                    futures::future::join_all(endpoint_futures).await;
                let any_skipped = captures.iter().any(|c| c.error.is_some());
                MatrixRow {
                    provider: provider.name.clone(),
                    kind: provider.kind.clone(),
                    tls_profile: provider.tls_profile.clone(),
                    captures,
                    note: if any_skipped {
                        Some("one or more endpoints skipped — see captures".into())
                    } else {
                        None
                    },
                }
            });
        }
        futures::future::join_all(row_futures).await
    });

    rows.extend(captured_rows);

    // Distinct fingerprint counts (ignoring `None`) tell us at a glance
    // whether different providers produce different signatures.
    let mut ja3_set = std::collections::HashSet::new();
    let mut ja4_set = std::collections::HashSet::new();
    let mut captured = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        for c in &row.captures {
            if c.error.is_some() {
                skipped += 1;
                continue;
            }
            captured += 1;
            if let Some(ja3) = &c.ja3_hash {
                ja3_set.insert(ja3.clone());
            }
            if let Some(ja4) = &c.ja4 {
                ja4_set.insert(ja4.clone());
            }
        }
    }

    let matrix = Matrix {
        generated_at: Utc::now().to_rfc3339(),
        endpoints: endpoints.clone(),
        rows,
        notes,
        summary: MatrixSummary {
            provider_count: cfg.providers.len(),
            captured_count: captured,
            skipped_count: skipped,
            distinct_ja3: ja3_set.len(),
            distinct_ja4: ja4_set.len(),
        },
    };

    let out = serde_json::to_string_pretty(&matrix).unwrap();
    let out_path =
        env::var("MATRIX_OUT").unwrap_or_else(|_| "lab/evals/provider_matrix.report.json".into());
    fs::write(&out_path, &out).unwrap_or_else(|e| {
        eprintln!("warn: failed to write {out_path}: {e}");
    });
    println!("{}", out);
}

async fn capture_one(provider: &Provider, endpoint: &str) -> EndpointCapture {
    let started = Instant::now();
    let result = match provider.kind.as_str() {
        "static" | "tls" => capture_via_static(endpoint).await,
        "browserless" => capture_via_browserless(provider, endpoint).await,
        "kernel" => capture_via_kernel(provider, endpoint).await,
        "browserbase" => Err(
            "browserbase capture requires a CDP-driven page request; not supported via HTTP shim"
                .to_string(),
        ),
        other => Err(format!("unknown provider kind: {other}")),
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok((status, fp)) => EndpointCapture {
            endpoint: endpoint.to_string(),
            ja3: fp.ja3,
            ja3_hash: fp.ja3_hash,
            ja4: fp.ja4,
            h2_fp: fp.h2_fp,
            user_agent: fp.user_agent,
            status,
            latency_ms,
            error: None,
        },
        Err(e) => EndpointCapture {
            endpoint: endpoint.to_string(),
            ja3: None,
            ja3_hash: None,
            ja4: None,
            h2_fp: None,
            user_agent: None,
            status: 0,
            latency_ms,
            error: Some(e),
        },
    }
}

#[derive(Debug, Default)]
struct Fingerprint {
    ja3: Option<String>,
    ja3_hash: Option<String>,
    ja4: Option<String>,
    h2_fp: Option<String>,
    user_agent: Option<String>,
}

/// Plain reqwest-over-rustls capture. This is the baseline — what the
/// process's default TLS stack looks like. Useful to compare against the
/// impersonated TLS profiles.
async fn capture_via_static(endpoint: &str) -> Result<(u16, Fingerprint), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("client build: {e}"))?;
    let resp = client
        .get(endpoint)
        .send()
        .await
        .map_err(|e| format!("request: {e}"))?;
    let status = resp.status().as_u16();
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("decode: {e}"))?;
    Ok((status, parse_fingerprint(&body)))
}

/// Browserless `/content` POST that fetches the fingerprint endpoint
/// through Browserless's Chrome instance, then we extract the fingerprint
/// JSON from the rendered HTML body.
async fn capture_via_browserless(
    provider: &Provider,
    endpoint: &str,
) -> Result<(u16, Fingerprint), String> {
    let url = provider
        .url
        .as_deref()
        .unwrap_or("https://chrome.browserless.io");
    let token = provider
        .token_env
        .as_deref()
        .and_then(|v| env::var(v).ok())
        .ok_or_else(|| "browserless token env not set".to_string())?;

    let bl_url = format!("{}/content?token={}", url.trim_end_matches('/'), token);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("client: {e}"))?;
    let resp = client
        .post(&bl_url)
        .json(&serde_json::json!({ "url": endpoint }))
        .send()
        .await
        .map_err(|e| format!("request: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| format!("decode: {e}"))?;
    let body: serde_json::Value =
        serde_json::from_str(extract_json_from_html(&text)).unwrap_or(serde_json::Value::Null);
    Ok((status, parse_fingerprint(&body)))
}

/// Kernel browser request — uses Kernel's REST adapter to navigate to the
/// fingerprint endpoint and read the response body.
async fn capture_via_kernel(
    provider: &Provider,
    endpoint: &str,
) -> Result<(u16, Fingerprint), String> {
    let url = provider
        .url
        .as_deref()
        .ok_or_else(|| "kernel url not set".to_string())?;
    let api_key = provider
        .api_key_env
        .as_deref()
        .and_then(|v| env::var(v).ok())
        .ok_or_else(|| "kernel api_key env not set".to_string())?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("client: {e}"))?;

    // Create browser
    let create = client
        .post(format!("{}/v1/browsers", url.trim_end_matches('/')))
        .header("authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({ "ttl_s": 60 }))
        .send()
        .await
        .map_err(|e| format!("kernel create: {e}"))?;

    if !create.status().is_success() {
        return Err(format!("kernel create returned {}", create.status()));
    }
    let resp_json: serde_json::Value = create.json().await.map_err(|e| format!("decode: {e}"))?;
    let browser_id = resp_json
        .get("browser_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing browser_id".to_string())?
        .to_string();

    // Goto + content
    let _ = client
        .post(format!(
            "{}/v1/browsers/{browser_id}/goto",
            url.trim_end_matches('/')
        ))
        .header("authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({ "url": endpoint }))
        .send()
        .await;

    let content = client
        .post(format!(
            "{}/v1/browsers/{browser_id}/content",
            url.trim_end_matches('/')
        ))
        .header("authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({ "url": endpoint }))
        .send()
        .await
        .map_err(|e| format!("kernel content: {e}"))?;
    let status = content.status().as_u16();
    let body_text = content.text().await.map_err(|e| format!("decode: {e}"))?;
    let body: serde_json::Value =
        serde_json::from_str(extract_json_from_html(&body_text)).unwrap_or(serde_json::Value::Null);

    // Best-effort cleanup
    let _ = client
        .delete(format!(
            "{}/v1/browsers/{browser_id}",
            url.trim_end_matches('/')
        ))
        .header("authorization", format!("Bearer {api_key}"))
        .send()
        .await;

    Ok((status, parse_fingerprint(&body)))
}

/// `tls.peet.ws` returns a JSON body directly (no HTML wrapper); when the
/// fingerprint endpoint is fetched through a browser, the body is wrapped
/// in `<pre>...JSON...</pre>` or shows up between `{` and the matching
/// closing brace at the document root. Strip surrounding markup.
fn extract_json_from_html(s: &str) -> &str {
    let trimmed = s.trim();
    if trimmed.starts_with('{') {
        return trimmed;
    }
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return &trimmed[start..=end];
            }
        }
    }
    trimmed
}

/// Map fields from common fingerprint endpoints (tls.peet.ws,
/// browserleaks.com) into our `Fingerprint` shape. Different endpoints use
/// different field names, so we look for several aliases.
fn parse_fingerprint(body: &serde_json::Value) -> Fingerprint {
    let str_field = |keys: &[&str]| -> Option<String> {
        for k in keys {
            if let Some(v) = body.get(k).and_then(|v| v.as_str()) {
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
            // tls.peet.ws nests under "tls"
            if let Some(tls) = body.get("tls") {
                if let Some(v) = tls.get(k).and_then(|v| v.as_str()) {
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
            if let Some(http2) = body.get("http2") {
                if let Some(v) = http2.get(k).and_then(|v| v.as_str()) {
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
        }
        None
    };

    Fingerprint {
        ja3: str_field(&["ja3", "ja3_string"]),
        ja3_hash: str_field(&["ja3_hash", "ja3hash"]),
        ja4: str_field(&["ja4", "ja4_full", "ja4_string"]),
        h2_fp: str_field(&["akamai_fingerprint", "h2_fingerprint", "fingerprint"]),
        user_agent: str_field(&["user_agent", "User-Agent", "userAgent"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_fingerprint_handles_tls_peet_ws_shape() {
        let body = json!({
            "tls": {
                "ja3": "771,4865-4866-4867,...",
                "ja3_hash": "abc123",
            },
            "http2": {
                "akamai_fingerprint": "1:65536;3:1000;...",
            },
            "user_agent": "Mozilla/5.0"
        });
        let fp = parse_fingerprint(&body);
        assert_eq!(fp.ja3.as_deref(), Some("771,4865-4866-4867,..."));
        assert_eq!(fp.ja3_hash.as_deref(), Some("abc123"));
        assert!(fp.h2_fp.is_some());
        assert_eq!(fp.user_agent.as_deref(), Some("Mozilla/5.0"));
    }

    #[test]
    fn parse_fingerprint_handles_browserleaks_shape() {
        let body = json!({
            "ja3_hash": "deadbeef",
            "ja4": "t13d1516h2_8daaf6152771_b186095e22b6",
            "user_agent": "Chrome/120"
        });
        let fp = parse_fingerprint(&body);
        assert_eq!(fp.ja3_hash.as_deref(), Some("deadbeef"));
        assert!(fp.ja4.unwrap().starts_with("t13d"));
    }

    #[test]
    fn parse_fingerprint_returns_none_when_fields_missing() {
        let body = json!({});
        let fp = parse_fingerprint(&body);
        assert!(fp.ja3.is_none());
        assert!(fp.ja4.is_none());
        assert!(fp.user_agent.is_none());
    }

    #[test]
    fn extract_json_strips_html_wrapper() {
        let html = "<html><body><pre>{\"ja3_hash\":\"abc\"}</pre></body></html>";
        let extracted = extract_json_from_html(html);
        assert!(extracted.starts_with("{"));
        assert!(extracted.ends_with("}"));
    }

    #[test]
    fn extract_json_passes_through_bare_json() {
        let raw = "{\"x\":1}";
        assert_eq!(extract_json_from_html(raw), raw);
    }
}
