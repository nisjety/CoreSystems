//! INNH-7: the URL result cache that `max_age` reads on `POST /v1/scrape`.
//!
//! The page cache (`cache.rs`, `prev_fingerprint`) can only be hit by a caller that
//! already holds the page's body hash. This cache is addressed by what the caller
//! asked for: org + canonical URL + the options that change a result
//! (`PageCache::result_key`). A request with `max_age: N` (milliseconds) is served the
//! stored result for its key if that result was fetched at most N ms ago, without a
//! fetch; otherwise the page is fetched and the entry replaced. This module holds the
//! decisions around that: who may use the cache, how a hit's inline text is rebuilt,
//! and how old an entry is.
//!
//! Rules that never bend:
//! - ZDR never reads it, never writes it, and no key (not even a hash of the URL) is
//!   computed for a ZDR request: [`eligibility`] answers before a key exists.
//! - A capture, an action, an AI format, the browser driver, a non-default web policy,
//!   an `ingest` request and a request that classes its privacy as ephemeral or secret
//!   are never cached: they are not a plain read of a public page.
//! - The entry holds no inline text. A hit rebuilds markdown, html and rawHtml from the
//!   stored artifacts of the original run, so a missing artifact is a miss (the page is
//!   fetched again), never a 500 and never a partial answer.
//! - Only a 2xx result of the static or TLS driver is written, and never a page the
//!   run itself flagged as not content: the pipeline marks a bot wall
//!   (`InlineParts::bot_wall_suspected`) and a soft-404 (`InlineParts::soft_404`) in
//!   every `QUARRY_CHALLENGE_ENFORCE` mode. Under the shipped default (`log`) such a
//!   page does NOT fail the run (it returns Ok), so the flags, not the run's success,
//!   are what keep it out of the cache. `enforce` fails the run before any write.

use chrono::{DateTime, Utc};
use quarry_core::output::DriverKind;
use quarry_core::privacy::{PrivacyClassification, PrivacyPolicy};
use quarry_core::scrape::{ResolvedScrapeOptions, ScrapeWarning};
use quarry_runtime::artifact_store::ArtifactStore;
use quarry_runtime::pipeline::{offer_inline, InlineParts, InlineRequest};

use crate::cache::CachedScrape;

/// Everything about a request that decides whether the URL result cache may serve or
/// store it, beyond what its options say.
pub struct Request<'a> {
    pub resolved: &'a ResolvedScrapeOptions,
    /// The ZDR guard refused a cache write (`zdr::guard(zdr, WriteKind::Cache)`).
    pub zdr_blocks_cache: bool,
    /// The driver the request is planned to run on.
    pub driver: DriverKind,
    /// The web policy is the catalogue default (no per-request tightening).
    pub default_web_policy: bool,
    pub privacy: &'a PrivacyPolicy,
    /// The request asked for the page to be ingested; a hit would skip that.
    pub ingest: bool,
}

/// Whether the URL result cache may be used for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    Eligible,
    /// ZDR: the cache is not read, not written, and no key is computed.
    Zdr,
    /// Some other reason the request is not a plain cacheable read.
    NotCacheable,
}

impl Eligibility {
    /// The warning a request that asked for `max_age` gets when the cache was not used,
    /// or `None` when it was.
    pub fn warning(self) -> Option<ScrapeWarning> {
        match self {
            Self::Eligible => None,
            Self::Zdr => Some(ScrapeWarning::for_field(
                "max_age_ignored_zdr",
                "max_age",
                "max_age was ignored: a zero-data-retention request never reads or writes the result cache",
            )),
            Self::NotCacheable => Some(ScrapeWarning::for_field(
                "max_age_ignored",
                "max_age",
                "max_age was ignored: this request (captures, actions, AI formats, the browser, a custom web policy, ingest or a private privacy class) is never cached",
            )),
        }
    }
}

/// The warning for `max_age` on a stack with no cache backend (no Redis) or for an
/// org that cannot address one.
pub fn unavailable_warning() -> ScrapeWarning {
    ScrapeWarning::for_field(
        "cache_unavailable",
        "max_age",
        "max_age was ignored: the result cache is not available on this deployment",
    )
}

pub fn eligibility(request: &Request<'_>) -> Eligibility {
    if request.zdr_blocks_cache || request.privacy.zdr.is_active() {
        return Eligibility::Zdr;
    }
    let privacy_ok = !matches!(
        request.privacy.privacy_classification,
        PrivacyClassification::ZdrEphemeral | PrivacyClassification::CredentialOrSecret
    );
    if request.resolved.result_cache_admissible()
        && request.driver != DriverKind::Browser
        && request.default_web_policy
        && privacy_ok
        && !request.ingest
    {
        Eligibility::Eligible
    } else {
        Eligibility::NotCacheable
    }
}

/// How old a stored result is, in milliseconds. A clock that ran backwards reads as 0.
pub fn age_ms(fetched_at: DateTime<Utc>, now: DateTime<Utc>) -> u64 {
    u64::try_from((now - fetched_at).num_milliseconds()).unwrap_or(0)
}

/// Whether a result that old may be served for a `max_age` of `max_age_ms`. A `max_age`
/// of zero never serves anything, whatever the age.
pub fn is_fresh(age_ms: u64, max_age_ms: u64) -> bool {
    max_age_ms > 0 && age_ms <= max_age_ms
}

/// Whether a fresh result may be stored: a 2xx page from a driver that fetched it
/// without a browser, which the run did not flag as a bot wall or a soft-404. A
/// browser-served result (an escalation) stays out in this version. The flags come
/// from `inline` because in the default `log` challenge mode a bot wall or a
/// "page not found" body still returns a successful output.
pub fn is_storable(output: &quarry_core::output::NormalizedOutput, inline: &InlineParts) -> bool {
    (200..300).contains(&output.status)
        && output.driver.kind != DriverKind::Browser
        && output.formats.markdown.is_some()
        && !inline.bot_wall_suspected
        && !inline.soft_404
}

/// Rebuild the inline parts a request asked for from the artifacts of the run that
/// produced `cached`. `None` when any artifact it needs is gone or unreadable: the
/// caller treats that as a miss.
pub async fn rehydrate(
    artifacts: &dyn ArtifactStore,
    org_id: &str,
    cached: &CachedScrape,
    request: &InlineRequest,
) -> Option<InlineParts> {
    let output = &cached.output;
    let mut parts = InlineParts {
        warnings: cached.warnings.clone(),
        ..InlineParts::default()
    };
    // A stored result is never ZDR, so an over-cap field is left out and named, as in a run.
    let cap = request.max_bytes;
    if request.markdown {
        let reference = output.formats.markdown.as_ref()?;
        let bytes = artifacts.get(org_id, &reference.artifact_id).await.ok()?;
        let text = String::from_utf8(bytes).ok()?;
        offer_inline(
            "markdown",
            &text,
            cap,
            false,
            &mut parts.markdown,
            &mut parts.truncated,
            &mut parts.omitted,
        );
    }
    if request.html || request.raw_html {
        // A document keeps its original bytes as `raw`, never as `html`; neither text
        // form exists for it (the warning is already in `cached.warnings`).
        let is_document = output.formats.raw.is_some() && output.formats.html.is_none();
        if is_document {
            for (wanted, field) in [(request.html, "html"), (request.raw_html, "raw_html")] {
                if wanted {
                    parts.omitted.push(field.to_owned());
                }
            }
        } else {
            let reference = output.formats.html.as_ref()?;
            let bytes = artifacts.get(org_id, &reference.artifact_id).await.ok()?;
            let declared = output
                .metadata
                .content_type
                .as_deref()
                .and_then(quarry_transform::charset::charset_from_content_type);
            let body_text = quarry_transform::charset::decode(&bytes, declared).text;
            if request.html {
                // The same call a run makes when no cleaning produced the HTML already.
                let html_text =
                    quarry_transform::clean_html::to_markdown(&body_text, &request.clean).html;
                offer_inline(
                    "html",
                    &html_text,
                    cap,
                    false,
                    &mut parts.html,
                    &mut parts.truncated,
                    &mut parts.omitted,
                );
            }
            if request.raw_html {
                offer_inline(
                    "raw_html",
                    &body_text,
                    cap,
                    false,
                    &mut parts.raw_html,
                    &mut parts.truncated,
                    &mut parts.omitted,
                );
            }
        }
    }
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::scrape::{ScrapeLimits, ScrapeOptions};
    use quarry_runtime::artifact_store::InMemoryStore;
    use serde_json::json;

    fn resolved(body: serde_json::Value) -> ResolvedScrapeOptions {
        let limits = ScrapeLimits {
            captures_enabled: true,
            actions_enabled: true,
            ai_formats_enabled: true,
            max_age_enabled: true,
            ..ScrapeLimits::default()
        };
        ScrapeOptions::from_value(&body)
            .expect("options parse")
            .resolve(false, &limits)
            .expect("options resolve")
    }

    fn privacy() -> PrivacyPolicy {
        PrivacyPolicy::default()
    }

    fn verdict(
        body: serde_json::Value,
        mutate: impl FnOnce(&mut Request<'_>),
    ) -> Eligibility {
        let resolved = resolved(body);
        let privacy = privacy();
        let mut request = Request {
            resolved: &resolved,
            zdr_blocks_cache: false,
            driver: DriverKind::Static,
            default_web_policy: true,
            privacy: &privacy,
            ingest: false,
        };
        mutate(&mut request);
        eligibility(&request)
    }

    #[test]
    fn a_plain_request_and_one_with_options_are_eligible() {
        assert_eq!(verdict(json!({ "max_age": 1000 }), |_| {}), Eligibility::Eligible);
        assert_eq!(
            verdict(
                json!({ "max_age": 1000, "formats": ["markdown", "html"], "include_tags": ["main"], "extractor": "raw", "only_main_content": false }),
                |_| {}
            ),
            Eligibility::Eligible
        );
        assert_eq!(verdict(json!({}), |_| {}), Eligibility::Eligible);
    }

    #[test]
    fn zdr_wins_over_everything_else() {
        assert_eq!(verdict(json!({ "max_age": 1000 }), |r| r.zdr_blocks_cache = true), Eligibility::Zdr);
        assert_eq!(
            verdict(json!({ "formats": ["screenshot"] }), |r| r.zdr_blocks_cache = true),
            Eligibility::Zdr
        );
        let resolved = resolved(json!({}));
        let zdr_privacy = PrivacyPolicy::default().with_zdr(quarry_core::zdr::ZdrMode::On);
        let request = Request {
            resolved: &resolved,
            zdr_blocks_cache: false,
            driver: DriverKind::Static,
            default_web_policy: true,
            privacy: &zdr_privacy,
            ingest: false,
        };
        assert_eq!(eligibility(&request), Eligibility::Zdr);
    }

    #[test]
    fn captures_actions_ai_formats_the_browser_and_policy_changes_are_not_cacheable() {
        for body in [
            json!({ "formats": ["screenshot"] }),
            json!({ "formats": [{ "type": "pdf" }] }),
            json!({ "actions": [{ "type": "press", "key": "Enter" }] }),
            json!({ "formats": [{ "type": "summary" }] }),
            json!({ "formats": [{ "type": "question", "question": "Who?" }] }),
        ] {
            assert_eq!(verdict(body.clone(), |_| {}), Eligibility::NotCacheable, "{body}");
        }
        assert_eq!(verdict(json!({}), |r| r.driver = DriverKind::Browser), Eligibility::NotCacheable);
        assert_eq!(verdict(json!({}), |r| r.default_web_policy = false), Eligibility::NotCacheable);
        assert_eq!(verdict(json!({}), |r| r.ingest = true), Eligibility::NotCacheable);
        // A TLS-profile fetch is still a plain fetch.
        assert_eq!(verdict(json!({}), |r| r.driver = DriverKind::Tls), Eligibility::Eligible);
    }

    #[test]
    fn an_ephemeral_or_secret_privacy_class_is_not_cacheable() {
        for class in [PrivacyClassification::ZdrEphemeral, PrivacyClassification::CredentialOrSecret] {
            let resolved = resolved(json!({}));
            let privacy = PrivacyPolicy {
                privacy_classification: class,
                ..PrivacyPolicy::default()
            };
            let request = Request {
                resolved: &resolved,
                zdr_blocks_cache: false,
                driver: DriverKind::Static,
                default_web_policy: true,
                privacy: &privacy,
                ingest: false,
            };
            assert_eq!(eligibility(&request), Eligibility::NotCacheable, "{class:?}");
        }
    }

    #[test]
    fn warnings_name_max_age_and_say_why() {
        assert!(Eligibility::Eligible.warning().is_none());
        let zdr = Eligibility::Zdr.warning().unwrap();
        assert_eq!((zdr.code.as_str(), zdr.field.as_deref()), ("max_age_ignored_zdr", Some("max_age")));
        let other = Eligibility::NotCacheable.warning().unwrap();
        assert_eq!(other.code, "max_age_ignored");
        assert_eq!(unavailable_warning().code, "cache_unavailable");
    }

    #[test]
    fn freshness_is_inclusive_and_zero_serves_nothing() {
        let now = Utc::now();
        assert_eq!(age_ms(now - chrono::Duration::milliseconds(1500), now), 1500);
        assert_eq!(age_ms(now + chrono::Duration::seconds(5), now), 0, "a future stamp reads as fresh, not negative");
        assert!(is_fresh(1000, 1000));
        assert!(is_fresh(0, 1));
        assert!(!is_fresh(1001, 1000));
        assert!(!is_fresh(0, 0), "max_age 0 never serves");
    }

    fn output(status: u16, driver: &str, with_markdown: bool) -> quarry_core::output::NormalizedOutput {
        let mut value = json!({
            "run_id": quarry_core::ids::kinds::RunKind::new(),
            "url": { "requested": "https://example.com/a", "final_url": "https://example.com/a" },
            "status": status,
            "fetched_at": "2026-01-01T00:00:00Z",
            "fingerprint": "fp",
            "formats": {},
            "change": { "status": "new" },
            "metadata": { "title": "T", "content_type": "text/html; charset=utf-8" },
            "driver": { "kind": driver, "duration_ms": 1 }
        });
        if with_markdown {
            value["formats"]["markdown"] = json!({ "artifact_id": quarry_core::ids::kinds::ArtifactKind::new(), "bytes": 1 });
        }
        serde_json::from_value(value).expect("a NormalizedOutput")
    }

    #[test]
    fn only_a_2xx_non_browser_result_with_a_markdown_artifact_is_stored() {
        let clean = InlineParts::default();
        assert!(is_storable(&output(200, "static", true), &clean));
        assert!(is_storable(&output(204, "tls", true), &clean));
        assert!(!is_storable(&output(404, "static", true), &clean));
        assert!(!is_storable(&output(500, "static", true), &clean));
        assert!(!is_storable(&output(301, "static", true), &clean));
        assert!(!is_storable(&output(200, "browser", true), &clean));
        assert!(!is_storable(&output(200, "static", false), &clean));
    }

    #[test]
    fn a_bot_wall_or_a_soft_404_is_never_stored_even_though_the_run_succeeded() {
        let ok = output(200, "static", true);
        let wall = InlineParts {
            bot_wall_suspected: true,
            ..InlineParts::default()
        };
        let soft = InlineParts {
            soft_404: true,
            ..InlineParts::default()
        };
        assert!(!is_storable(&ok, &wall));
        assert!(!is_storable(&ok, &soft));
        assert!(is_storable(&ok, &InlineParts::default()));
    }

    const ORG: &str = "org_rehydrate";
    const RAW: &str = "<html><head><title>T</title></head><body><nav>menu</nav><main><p>Hello world</p></main></body></html>";

    async fn stored(store: &InMemoryStore) -> CachedScrape {
        let run: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
        let raw = store.put(ORG, &run, "ph", "html", RAW.as_bytes().to_vec()).await.unwrap();
        let md = store.put(ORG, &run, "ph", "markdown", b"# Hello\n\nworld".to_vec()).await.unwrap();
        let mut out = output(200, "static", false);
        out.formats.html = Some(quarry_core::output::FormatRef { artifact_id: raw.artifact_id, bytes: raw.bytes });
        out.formats.markdown = Some(quarry_core::output::FormatRef { artifact_id: md.artifact_id, bytes: md.bytes });
        CachedScrape {
            output: out,
            warnings: vec![ScrapeWarning::new("some_run_warning", "kept on a hit")],
        }
    }

    fn inline(markdown: bool, html: bool, raw_html: bool) -> InlineRequest {
        InlineRequest {
            markdown,
            html,
            raw_html,
            ..InlineRequest::default_for(quarry_core::zdr::ZdrMode::Off)
        }
    }

    #[tokio::test]
    async fn a_hit_rebuilds_the_text_the_request_asked_for_from_the_artifacts() {
        let store = InMemoryStore::new();
        let cached = stored(&store).await;
        let parts = rehydrate(&store, ORG, &cached, &inline(true, true, true)).await.expect("rehydrated");
        assert_eq!(parts.markdown.as_deref(), Some("# Hello\n\nworld"));
        assert_eq!(parts.raw_html.as_deref(), Some(RAW));
        let html = parts.html.expect("html");
        assert!(html.contains("Hello world"), "{html}");
        assert_eq!(parts.warnings[0].code, "some_run_warning");
        assert!(parts.truncated.is_empty() && parts.omitted.is_empty());
        // Nothing asked: nothing read, nothing returned.
        let none = rehydrate(&store, ORG, &cached, &inline(false, false, false)).await.unwrap();
        assert!(none.markdown.is_none() && none.html.is_none() && none.raw_html.is_none());
    }

    #[tokio::test]
    async fn an_over_cap_field_is_left_out_and_named() {
        let store = InMemoryStore::new();
        let cached = stored(&store).await;
        let mut request = inline(true, false, true);
        request.max_bytes = 8;
        let parts = rehydrate(&store, ORG, &cached, &request).await.unwrap();
        assert!(parts.markdown.is_none() && parts.raw_html.is_none());
        assert_eq!(parts.omitted, vec!["markdown".to_owned(), "raw_html".to_owned()]);
    }

    #[tokio::test]
    async fn a_missing_artifact_or_another_org_is_a_miss() {
        let store = InMemoryStore::new();
        let cached = stored(&store).await;
        // The artifacts belong to ORG; another org reads NotFound.
        assert!(rehydrate(&store, "org_other", &cached, &inline(true, false, false)).await.is_none());
        assert!(rehydrate(&store, "org_other", &cached, &inline(false, false, true)).await.is_none());
        // An entry whose artifacts were never stored (or expired) is a miss too.
        let empty = InMemoryStore::new();
        assert!(rehydrate(&empty, ORG, &cached, &inline(true, false, false)).await.is_none());
        assert!(rehydrate(&empty, ORG, &cached, &inline(false, true, false)).await.is_none());
        // ...but a request that reads no artifact does not need them.
        assert!(rehydrate(&empty, ORG, &cached, &inline(false, false, false)).await.is_some());
    }

    #[tokio::test]
    async fn a_document_has_no_html_forms() {
        let store = InMemoryStore::new();
        let mut cached = stored(&store).await;
        let raw = cached.output.formats.html.take();
        cached.output.formats.raw = raw;
        let parts = rehydrate(&store, ORG, &cached, &inline(true, true, true)).await.unwrap();
        assert!(parts.markdown.is_some());
        assert!(parts.html.is_none() && parts.raw_html.is_none());
        assert_eq!(parts.omitted, vec!["html".to_owned(), "raw_html".to_owned()]);
    }
}
