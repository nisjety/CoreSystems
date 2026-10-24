//! WS14.S5: the static check of a batch's URLs at handoff.
//!
//! `POST /v1/batch` used to accept any URL. This module checks each one before a
//! job exists, with no network work: the URL must parse, use http or https, pass
//! the address and userinfo heuristics (`quarry_security::heur::check`), be unique
//! once its fragment is ignored, and sit under the batch cap. A refused URL carries
//! a closed code from the crawl denial vocabulary (`quarry_core::crawl_denial`) and
//! that vocabulary's operator message. Neither echoes a host or a reason detail
//! from the caller's URL.
//!
//! The cap is `QUARRY_EDGE__CRAWL_BATCH_MAX_URLS` (default 500, at most 10 000). It
//! is read once at boot, and an unparsable value fails boot. The gateway keeps its
//! own cap of 200.
//!
//! The module is `batch_check` rather than `crawl_request`: that name belongs to the
//! WS14.S6 request validator, which is built in the same tree.

use std::collections::HashSet;
use std::sync::OnceLock;

use quarry_core::crawl_denial::CrawlDenialReason;
use quarry_security::{heur, Decision};
use serde::{Deserialize, Serialize};
use url::Url;

/// The environment name of the batch cap.
pub const ENV_CRAWL_BATCH_MAX_URLS: &str = "QUARRY_EDGE__CRAWL_BATCH_MAX_URLS";
/// The batch cap when the variable is absent or blank.
pub const DEFAULT_BATCH_MAX_URLS: usize = 500;
/// The largest value the variable may take.
pub const CEILING_BATCH_MAX_URLS: usize = 10_000;

/// One URL of a batch that the static check refused. `code` is a snake_case code
/// of the crawl denial vocabulary, so it is `not_crawlable`, `security_rejected`,
/// `duplicate` or `max_pages_reached`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRejection {
    pub url: String,
    pub code: String,
    pub message: String,
}

/// The outcome of [`validate_batch_urls`]: the URLs to hand off, in request order,
/// and the ones refused.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchCheck {
    pub accepted: Vec<String>,
    pub rejected: Vec<BatchRejection>,
}

/// Parse the batch cap. Absent or blank is the default; anything that is not a whole
/// number from 1 to [`CEILING_BATCH_MAX_URLS`] is an error that does not echo the value.
pub fn parse_batch_max_urls(raw: Option<&str>) -> Result<usize, String> {
    let Some(value) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(DEFAULT_BATCH_MAX_URLS);
    };
    match value.parse::<usize>() {
        Ok(n) if (1..=CEILING_BATCH_MAX_URLS).contains(&n) => Ok(n),
        _ => Err(format!(
            "{ENV_CRAWL_BATCH_MAX_URLS} must be a whole number from 1 to {CEILING_BATCH_MAX_URLS}"
        )),
    }
}

/// Validate the batch cap from the process environment (used by boot).
pub fn check_boot() -> Result<usize, String> {
    parse_batch_max_urls(std::env::var(ENV_CRAWL_BATCH_MAX_URLS).ok().as_deref())
}

/// The batch cap the process runs with, read once. Boot validates the environment
/// first, so a value that reaches here was valid at start-up.
pub fn batch_max_urls() -> usize {
    static CAP: OnceLock<usize> = OnceLock::new();
    *CAP.get_or_init(|| check_boot().unwrap_or(DEFAULT_BATCH_MAX_URLS))
}

/// Check every URL of a batch, in request order, against the cap `max`. Nothing is
/// fetched and nothing is resolved here. Each URL is trimmed; an accepted URL keeps
/// its trimmed text, so the orchestrator receives what the caller sent.
pub fn validate_batch_urls(urls: &[String], max: usize) -> BatchCheck {
    let limit = u32::try_from(max).unwrap_or(u32::MAX);
    let mut seen: HashSet<String> = HashSet::new();
    let mut check = BatchCheck::default();
    for (index, raw) in urls.iter().enumerate() {
        let url = raw.trim();
        if index >= max {
            check
                .rejected
                .push(refuse(url, &CrawlDenialReason::MaxPagesReached { limit }));
            continue;
        }
        match check_one(url) {
            Ok(key) => {
                if seen.insert(key) {
                    check.accepted.push(url.to_string());
                } else {
                    check.rejected.push(refuse(url, &CrawlDenialReason::Duplicate));
                }
            }
            Err(reason) => check.rejected.push(refuse(url, &reason)),
        }
    }
    check
}

/// The duplicate key of a URL that may be crawled: its normalised text without the
/// fragment. A refusal is the reason it may not.
fn check_one(raw: &str) -> Result<String, CrawlDenialReason> {
    let parsed = Url::parse(raw).map_err(|_| CrawlDenialReason::NotCrawlable {
        reason: "the URL does not parse".to_string(),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(CrawlDenialReason::NotCrawlable {
            reason: "only http and https URLs can be crawled".to_string(),
        });
    }
    if heur::check(&parsed).decision == Decision::Block {
        return Err(CrawlDenialReason::SecurityRejected {
            reason: "the address or userinfo check refused the URL".to_string(),
        });
    }
    let mut key = parsed;
    key.set_fragment(None);
    Ok(key.to_string())
}

fn refuse(url: &str, reason: &CrawlDenialReason) -> BatchRejection {
    BatchRejection {
        url: url.to_string(),
        code: reason.code().to_string(),
        message: reason.message(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn code_of(check: &BatchCheck, url: &str) -> Option<String> {
        check
            .rejected
            .iter()
            .find(|r| r.url == url)
            .map(|r| r.code.clone())
    }

    #[test]
    fn plain_https_urls_are_accepted_in_order() {
        let check = validate_batch_urls(
            &urls(&["https://a.test/", "http://b.test/x", "  https://c.test/y  "]),
            500,
        );
        assert_eq!(
            check.accepted,
            vec!["https://a.test/", "http://b.test/x", "https://c.test/y"]
        );
        assert!(check.rejected.is_empty());
    }

    #[test]
    fn file_and_ftp_schemes_are_not_crawlable() {
        let check = validate_batch_urls(&urls(&["file:///etc/passwd", "ftp://a.test/x"]), 500);
        assert!(check.accepted.is_empty());
        assert_eq!(code_of(&check, "file:///etc/passwd").as_deref(), Some("not_crawlable"));
        assert_eq!(code_of(&check, "ftp://a.test/x").as_deref(), Some("not_crawlable"));
    }

    #[test]
    fn unparseable_text_is_not_crawlable() {
        let check = validate_batch_urls(&urls(&["", "not a url"]), 500);
        assert!(check.accepted.is_empty());
        assert_eq!(check.rejected.len(), 2);
        assert!(check.rejected.iter().all(|r| r.code == "not_crawlable"));
    }

    #[test]
    fn userinfo_and_private_hosts_are_security_rejected() {
        let refused = [
            "http://user:pw@a.test/",
            "http://127.0.0.1/private",
            "http://[::1]/private",
            "http://localhost/private",
        ];
        let check = validate_batch_urls(&urls(&refused), 500);
        assert!(check.accepted.is_empty());
        for url in refused {
            assert_eq!(code_of(&check, url).as_deref(), Some("security_rejected"), "{url}");
        }
    }

    #[test]
    fn repeats_that_differ_only_by_fragment_are_duplicates() {
        let check = validate_batch_urls(
            &urls(&["https://a.test/docs/a", "https://a.test/docs/a#x", "HTTPS://A.test/docs/a"]),
            500,
        );
        assert_eq!(check.accepted, vec!["https://a.test/docs/a"]);
        assert_eq!(check.rejected.len(), 2);
        assert!(check.rejected.iter().all(|r| r.code == "duplicate"));
    }

    #[test]
    fn urls_past_the_cap_are_refused_and_the_rest_kept() {
        let many: Vec<String> = (0..501).map(|i| format!("https://a.test/p{i}")).collect();
        let check = validate_batch_urls(&many, 500);
        assert_eq!(check.accepted.len(), 500);
        assert_eq!(check.rejected.len(), 1);
        assert_eq!(check.rejected[0].code, "max_pages_reached");
        assert_eq!(check.rejected[0].url, "https://a.test/p500");
    }

    #[test]
    fn a_message_never_echoes_the_host() {
        let check = validate_batch_urls(&urls(&["http://127.0.0.1/secret-host-token"]), 500);
        let message = &check.rejected[0].message;
        assert!(!message.contains("127.0.0.1"), "{message}");
        assert!(!message.contains("secret-host-token"), "{message}");
    }

    #[test]
    fn the_batch_cap_defaults_and_refuses_bad_values() {
        assert_eq!(parse_batch_max_urls(None), Ok(500));
        assert_eq!(parse_batch_max_urls(Some("   ")), Ok(500));
        assert_eq!(parse_batch_max_urls(Some("200")), Ok(200));
        assert_eq!(parse_batch_max_urls(Some("10000")), Ok(10_000));
        assert!(parse_batch_max_urls(Some("0")).is_err());
        assert!(parse_batch_max_urls(Some("10001")).is_err());
        assert!(parse_batch_max_urls(Some("many")).is_err());
        let message = parse_batch_max_urls(Some("secret-value")).unwrap_err();
        assert!(!message.contains("secret-value"));
    }
}
