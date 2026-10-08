//! QB-T04 - the `/v1/retrieve` wire contract for NON-USER callers.
//!
//! Quarry's vector leg is the first non-browser consumer of the engine's HTTP
//! retrieve response, and the 2026-10-01 live proof found it speaking a
//! different dialect from the one the engine actually emits:
//!
//!   * it sent `zdr_mode:"off"`, which the strict `ZdrMode` enum refuses
//!     (`disabled | reject | ephemeral`), and it sent no `sovereign_required`,
//!     which the engine reads as `true`, so the dense arm could never run;
//!   * its response mirror read keys the engine never writes (`score`,
//!     `source_url`, `uri`), because its wiremock tests were written against an
//!     invented shape rather than the real one;
//!   * the engine had no URL on `SourceRef` at all, so even a perfect mirror
//!     could not turn a hit into a citable page.
//!
//! This file pins the wire so the next dialect drift fails HERE, hermetically
//! (no Postgres, no network), instead of in a live search:
//!
//!   * the response golden is the serialisation of a REAL `RetrievalResponse`
//!     built in this file, never hand-edited JSON;
//!   * the request golden is deserialised into the REAL `RetrievalRequest`, and
//!     every key it carries must land on a field the engine really reads (serde
//!     ignores unknown keys silently, which is exactly how `zdr` or `source_url`
//!     can ride along for months and do nothing);
//!   * `zdr-modes.golden.json` is derived from the REAL `ZdrMode` enum and the
//!     engine's own rerank/sovereignty rules, so Quarry's typed map has an
//!     authoritative list of accepted values.
//!
//! The goldens live INSIDE this crate's build context
//! (`tests/fixtures/contracts/retrieve-v1/`, sha256-pinned by `SHA256SUMS`).
//! Quarry vendors the directory byte for byte and checks the same pins. The
//! provider owns the contract: a consumer that needs a different body changes
//! the golden here first, never the other way round.
//!
//! Regenerate (only after a deliberate contract change):
//!   `UPDATE_CONTRACT_GOLDEN=1 cargo test -p retrieval-engine-rs --test retrieve_contract`
//! A missing, unpinned or altered golden FAILS here; nothing in this file skips.
//!
//! The DB-gated companion at the bottom (`TEST_DATABASE_URL`, `#[ignore]`) runs
//! the real source-join SQL under the real org-RLS policy.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Once;

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgConnectOptions;
use sqlx::PgPool;

use retrieval_engine::cache::CachedRetrieval;
use retrieval_engine::pipeline::orchestrator::{
    fetch_source_refs, sovereign_required_or_strict, text_rerank_requested,
};
use retrieval_engine::pipeline::types::{
    ContextFact, ContextPack, RetrievalRequest, RetrievalResponse, ScoredCandidate, SourceRef,
    ZdrMode,
};

mod common;

const DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/contracts/retrieve-v1"
);
const RESPONSE: &str = "response.golden.json";
const REQUEST: &str = "request.golden.json";
const ZDR_MODES: &str = "zdr-modes.golden.json";
const SUMS: &str = "SHA256SUMS";
const GOLDENS: [&str; 3] = [REQUEST, RESPONSE, ZDR_MODES];

const ALL_MODES: [ZdrMode; 3] = [ZdrMode::Disabled, ZdrMode::Reject, ZdrMode::Ephemeral];

/// Spellings a caller might plausibly send for "no restriction" or a restrictive
/// posture, none of which the engine accepts. `"off"` is the one that shipped.
const REFUSED_ZDR_MODES: [&str; 16] = [
    "off",
    "on",
    "none",
    "true",
    "false",
    "null",
    "",
    "zdr",
    "Disabled",
    "DISABLED",
    "Reject",
    "REJECT",
    "Ephemeral",
    "EPHEMERAL",
    " disabled",
    "disabled ",
];

// ───────────────────────── the real objects the goldens come from ─────────────────────────

fn candidate(
    knowledge_id: &str,
    document_id: &str,
    text: &str,
    dense: f32,
    sparse: f32,
    final_score: f32,
    chunk_index: i32,
) -> ScoredCandidate {
    ScoredCandidate {
        knowledge_id: knowledge_id.into(),
        document_id: document_id.into(),
        text: text.into(),
        dense_score: dense,
        sparse_score: sparse,
        rerank_score: 0.0,
        final_score,
        chunk_index,
        metadata: HashMap::new(),
    }
}

/// A response with all three kinds of source the engine joins: a Quarry crawl
/// page (the only kind that carries a `url`), a connector document, and a wiki
/// page. Every float is exactly representable so the JSON text is stable.
fn sample_response() -> RetrievalResponse {
    RetrievalResponse {
        candidates: vec![
            candidate(
                "ku-quarry-pricing-0",
                "doc-quarry-pricing",
                "The plan costs a fixed amount per cleaning cycle.",
                0.75,
                0.5,
                0.03125,
                0,
            ),
            candidate(
                "ku-handbook-3",
                "doc-handbook",
                "Conveyor belts are cleaned at the end of every shift.",
                0.5,
                0.25,
                0.015625,
                3,
            ),
            candidate(
                "wiki-block-1",
                "wiki-page-hygiene",
                "Hygiene zones are separated by colour.",
                0.25,
                0.0,
                0.0078125,
                1,
            ),
        ],
        sources: vec![
            SourceRef {
                document_id: "doc-quarry-pricing".into(),
                title: "Pricing".into(),
                source: "quarry".into(),
                r#type: "web_page".into(),
                url: Some("https://docs.example.test/pricing?plan=pro".into()),
            },
            SourceRef {
                document_id: "doc-handbook".into(),
                title: "Handbook".into(),
                source: "sharepoint".into(),
                r#type: "sharepoint_page".into(),
                url: None,
            },
            SourceRef {
                document_id: "wiki-page-hygiene".into(),
                title: "Hygiene".into(),
                source: "wiki/hygiene".into(),
                r#type: "wiki".into(),
                url: None,
            },
        ],
        query: "how often is the conveyor belt cleaned".into(),
        org_id: "org-golden".into(),
        trace_id: "00000000-0000-4000-8000-000000000001".into(),
        index_version: "v2-current".into(),
        zdr_mode: ZdrMode::Disabled.as_str().into(),
        zdr_actions_applied: vec![],
        low_confidence: false,
        context_pack: Some(ContextPack {
            facts: vec![ContextFact {
                knowledge_id: "ku-handbook-3".into(),
                document_id: "doc-handbook".into(),
                text: "Conveyor belts are cleaned at the end of every shift.".into(),
                score: 0.5,
                source_title: "Handbook".into(),
                source_type: "sharepoint_page".into(),
                estimated_tokens: 12,
            }],
            total_tokens: 12,
            budget_tokens: 4000,
            format: "json".into(),
        }),
        suggested_next_tools: vec![],
    }
}

/// The body the Quarry vector leg must send. Built from the typed enum so the
/// spelling can never be typed by hand: `json!` serialises `ZdrMode` through
/// its own `Serialize`.
fn canonical_request_body() -> Value {
    json!({
        "org_id": "org-golden",
        "query": "how often is the conveyor belt cleaned",
        "top_n": 10,
        "filters": { "sources": ["quarry"] },
        "zdr_mode": ZdrMode::Disabled,
        // Absent means `true` (strict) inside the engine, which switches the
        // dense arm off. A caller that runs the vector leg states `false`.
        "sovereign_required": false,
        // The vector leg wants the fused order, not a paid cross-encoder call.
        "mode_mix": { "rerank": false }
    })
}

fn zdr_modes_contract() -> Value {
    let accepted: Vec<Value> = ALL_MODES
        .iter()
        .map(|mode| {
            json!({
                "value": mode,
                "restricts_egress": mode.restricts_egress(),
                "ephemeral": mode.is_ephemeral(),
            })
        })
        .collect();
    json!({
        "contract": "retrieve-v1",
        "field": "zdr_mode",
        "default_when_absent": ZdrMode::default(),
        "accepted": accepted,
        "refused_examples": REFUSED_ZDR_MODES,
        "sovereign_required_when_absent": sovereign_required_or_strict(None),
        "text_rerank_when_mode_mix_rerank_is_false":
            text_rerank_requested(ZdrMode::Disabled, Some(false), false),
    })
}

fn pretty(value: &impl serde::Serialize) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("serialisable");
    text.push('\n');
    text
}

// ───────────────────────── golden files (pinned, never skipped) ─────────────────────────

fn lf(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().filter(|b| *b != b'\r').collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(lf(bytes)))
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(DIR).join(name)
}

/// Writes every golden and the `SHA256SUMS` manifest, once, when asked to.
fn regenerate_if_requested() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if std::env::var("UPDATE_CONTRACT_GOLDEN").as_deref() != Ok("1") {
            return;
        }
        std::fs::create_dir_all(DIR).expect("create the golden directory");
        let files: [(&str, String); 3] = [
            (REQUEST, pretty(&canonical_request_body())),
            (RESPONSE, pretty(&sample_response())),
            (ZDR_MODES, pretty(&zdr_modes_contract())),
        ];
        let mut sums = String::new();
        for (name, text) in &files {
            std::fs::write(path(name), text).expect("write golden");
            sums.push_str(&format!("{}  {name}\n", sha256_hex(text.as_bytes())));
        }
        std::fs::write(path(SUMS), sums).expect("write SHA256SUMS");
    });
}

/// `name -> sha256` from the manifest (`sha256sum` format, two-space separator).
fn pins() -> BTreeMap<String, String> {
    regenerate_if_requested();
    let text = std::fs::read_to_string(path(SUMS)).unwrap_or_else(|error| {
        panic!("{SUMS} is missing ({error}); regenerate with UPDATE_CONTRACT_GOLDEN=1")
    });
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (sum, name) = line
                .split_once("  ")
                .unwrap_or_else(|| panic!("malformed {SUMS} line: {line:?}"));
            assert!(
                sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit()),
                "{SUMS} holds a sha256 per file, got {sum:?}"
            );
            (name.to_owned(), sum.to_owned())
        })
        .collect()
}

/// A golden's text, after proving it still matches its pin.
fn golden_text(name: &str) -> String {
    let pins = pins();
    let bytes = std::fs::read(path(name)).unwrap_or_else(|error| {
        panic!(
            "contract golden {name} is missing ({error}); regenerate with UPDATE_CONTRACT_GOLDEN=1"
        )
    });
    let expected = pins
        .get(name)
        .unwrap_or_else(|| panic!("{name} is not listed in {SUMS}"));
    assert_eq!(
        &sha256_hex(&bytes),
        expected,
        "{name} no longer matches its sha256 pin; it is generated by this test, never edited"
    );
    String::from_utf8(lf(&bytes)).expect("golden is UTF-8")
}

fn golden(name: &str) -> Value {
    serde_json::from_str(&golden_text(name)).expect("golden is JSON")
}

// ───────────────────────── the response ─────────────────────────

#[test]
fn the_manifest_pins_exactly_the_goldens_in_the_directory() {
    let pinned: BTreeSet<String> = pins().into_keys().collect();
    let expected: BTreeSet<String> = GOLDENS.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(
        pinned, expected,
        "{SUMS} must list every golden and nothing else"
    );

    let on_disk: BTreeSet<String> = std::fs::read_dir(DIR)
        .expect("golden directory")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name != SUMS)
        .collect();
    assert_eq!(
        on_disk, expected,
        "an unpinned file in the contract directory would be vendored without a hash"
    );
    for name in GOLDENS {
        golden_text(name);
    }
}

#[test]
fn the_response_golden_is_the_serialisation_of_a_real_retrieval_response() {
    let real = serde_json::to_value(sample_response()).expect("serialisable");
    assert_eq!(
        golden(RESPONSE),
        real,
        "the response wire changed: regenerate the golden deliberately and re-vendor it into Quarry"
    );
}

#[test]
fn a_source_url_is_present_when_set_and_absent_when_none() {
    let with_url = serde_json::to_value(SourceRef {
        document_id: "d".into(),
        title: "t".into(),
        source: "quarry".into(),
        r#type: "web_page".into(),
        url: Some("https://docs.example.test/a".into()),
    })
    .expect("serialisable");
    assert_eq!(with_url["url"], "https://docs.example.test/a");

    let without = serde_json::to_value(SourceRef {
        document_id: "d".into(),
        title: "t".into(),
        source: "sharepoint".into(),
        r#type: "sharepoint_page".into(),
        url: None,
    })
    .expect("serialisable");
    assert!(
        !without.as_object().expect("object").contains_key("url"),
        "an absent url is omitted, never serialised as null: {without}"
    );
}

#[test]
fn only_the_quarry_source_in_the_golden_exposes_a_url() {
    let golden = golden(RESPONSE);
    let urls: Vec<(String, Option<String>)> = golden["sources"]
        .as_array()
        .expect("sources")
        .iter()
        .map(|source| {
            (
                source["source"].as_str().expect("source").to_owned(),
                source
                    .get("url")
                    .map(|v| v.as_str().expect("url is a string").to_owned()),
            )
        })
        .collect();
    assert_eq!(
        urls,
        vec![
            (
                "quarry".to_owned(),
                Some("https://docs.example.test/pricing?plan=pro".to_owned())
            ),
            ("sharepoint".to_owned(), None),
            ("wiki/hygiene".to_owned(), None),
        ]
    );
}

#[test]
fn a_scored_candidate_serializes_no_score_and_no_url() {
    // What Quarry's strict mirror must read. The candidate has NO `score` and NO
    // `url`: the order of `candidates` is the rank, and the URL comes from
    // `sources[]`, joined on `document_id`.
    let candidate = serde_json::to_value(candidate("k", "d", "text", 0.5, 0.25, 0.125, 2))
        .expect("serialisable");
    let keys: BTreeSet<&str> = candidate
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "chunk_index",
            "dense_score",
            "document_id",
            "final_score",
            "knowledge_id",
            "rerank_score",
            "sparse_score",
            "text",
        ])
    );
}

/// A strict consumer reads only what it needs and requires the keys the engine
/// really writes. This is the shape Quarry's `VectorIndex` must mirror.
#[derive(Deserialize)]
struct MirrorCandidate {
    document_id: String,
    text: String,
    final_score: f32,
}

#[derive(Deserialize)]
struct MirrorSource {
    document_id: String,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize)]
struct MirrorResponse {
    candidates: Vec<MirrorCandidate>,
    sources: Vec<MirrorSource>,
}

#[test]
fn a_strict_consumer_resolves_each_hit_url_from_sources_by_document_id() {
    let mirror: MirrorResponse = serde_json::from_value(golden(RESPONSE))
        .expect("the golden carries every key a consumer needs");
    let url_of: HashMap<&str, &str> = mirror
        .sources
        .iter()
        .filter_map(|source| {
            source
                .url
                .as_deref()
                .map(|url| (source.document_id.as_str(), url))
        })
        .collect();

    // Rank is the returned order; a hit without a resolvable url is dropped.
    let hits: Vec<(&str, &str)> = mirror
        .candidates
        .iter()
        .filter_map(|c| {
            url_of
                .get(c.document_id.as_str())
                .map(|url| (c.document_id.as_str(), *url))
        })
        .collect();
    assert_eq!(
        hits,
        vec![(
            "doc-quarry-pricing",
            "https://docs.example.test/pricing?plan=pro"
        )]
    );
    assert_eq!(
        mirror.candidates.len(),
        3,
        "the other two hits exist but are not citable"
    );
    assert!(mirror
        .candidates
        .iter()
        .all(|c| !c.text.is_empty() && c.final_score > 0.0));
}

// ───────────────────────── the cache holds SourceRef too ─────────────────────────

#[test]
fn a_cached_entry_written_before_the_url_field_still_reads_back() {
    // Written by the previous build: `sources[]` has no `url` key. If this stopped
    // parsing, every pre-deploy entry would degrade to a miss for its TTL.
    let old: CachedRetrieval = serde_json::from_value(json!({
        "candidates": [],
        "sources": [{
            "document_id": "d",
            "title": "t",
            "source": "quarry",
            "type": "web_page"
        }],
        "index_version": "v2-current",
        "low_confidence": false,
        "context_pack": null
    }))
    .expect("an entry from the previous build must stay readable");
    assert_eq!(old.sources.len(), 1);
    assert_eq!(old.sources[0].url, None);
}

#[test]
fn a_cached_url_survives_the_round_trip() {
    let payload = CachedRetrieval {
        candidates: vec![],
        sources: sample_response().sources,
        index_version: "v2-current".into(),
        zdr_actions_applied: vec![],
        low_confidence: false,
        context_pack: None,
        suggested_next_tools: vec![],
    };
    let restored: CachedRetrieval =
        serde_json::from_value(serde_json::to_value(&payload).expect("serialisable"))
            .expect("readable back");
    let urls: Vec<Option<&str>> = restored.sources.iter().map(|s| s.url.as_deref()).collect();
    assert_eq!(
        urls,
        vec![
            Some("https://docs.example.test/pricing?plan=pro"),
            None,
            None
        ],
        "a cache hit must serve the same citations as the miss that wrote it"
    );
}

// ───────────────────────── the citation url policy ─────────────────────────

#[test]
fn a_citation_url_is_exposed_only_for_quarry_documents() {
    let url = Some("https://docs.example.test/page");
    assert_eq!(
        SourceRef::citation_url("quarry", url),
        Some("https://docs.example.test/page".to_owned())
    );
    // A connector's metadata url is tenant metadata nobody reviewed for this
    // response: it never rides along for another source.
    for source in [
        "sharepoint",
        "chat-upload",
        "wiki/hygiene",
        "Quarry",
        "quarry-import",
        "",
    ] {
        assert_eq!(
            SourceRef::citation_url(source, url),
            None,
            "source {source:?}"
        );
    }
    assert_eq!(SourceRef::citation_url("quarry", None), None);
}

#[test]
fn a_citation_url_must_be_a_plain_http_or_https_url() {
    for good in [
        "http://docs.example.test/a",
        "https://docs.example.test/a?x=1#frag",
        "HTTPS://DOCS.EXAMPLE.TEST/A",
        "https://docs.example.test:8443/a",
    ] {
        assert_eq!(
            SourceRef::citation_url("quarry", Some(good)),
            Some(good.to_owned()),
            "{good}"
        );
    }
    for bad in [
        "",
        "docs.example.test/a",
        "ftp://docs.example.test/a",
        "javascript:alert(1)",
        "data:text/html;base64,AAAA",
        "file:///etc/passwd",
        "https://",
        "https:///path",
        "https://?q=1",
        "https://user:secret@docs.example.test/a",
        "https://docs.example.test/a b",
        "https://docs.example.test/a\nSet-Cookie: x=1",
        " https://docs.example.test/a",
        "https://docs.example.test/a\u{0000}",
    ] {
        assert_eq!(
            SourceRef::citation_url("quarry", Some(bad)),
            None,
            "{bad:?}"
        );
    }
    let too_long = format!("https://docs.example.test/{}", "a".repeat(2100));
    assert_eq!(SourceRef::citation_url("quarry", Some(&too_long)), None);
}

// ───────────────────────── the request ─────────────────────────

/// Every leaf the golden carries must be present, with the same value, in the
/// engine's own serialisation of the request it parsed. A key the engine drops
/// (serde ignores unknown keys) or overwrites (`user_id`, `admin_read_all`,
/// `service_principal` are boundary-only) shows up as a miss.
fn misses(expected: &Value, actual: &Value, at: &str, out: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(want), Value::Object(have)) => {
            for (key, value) in want {
                match have.get(key) {
                    Some(found) => misses(value, found, &format!("{at}.{key}"), out),
                    None => out.push(format!("{at}.{key} is not a field the engine reads")),
                }
            }
        }
        (Value::Array(want), Value::Array(have)) if want.len() == have.len() => {
            for (index, (w, h)) in want.iter().zip(have).enumerate() {
                misses(w, h, &format!("{at}[{index}]"), out);
            }
        }
        _ if expected == actual => {}
        _ => out.push(format!("{at}: sent {expected}, the engine read {actual}")),
    }
}

fn request_misses(body: &Value) -> Vec<String> {
    let request: RetrievalRequest =
        serde_json::from_value(body.clone()).expect("the body parses as a retrieval request");
    let mut out = Vec::new();
    misses(
        body,
        &serde_json::to_value(&request).expect("serialisable"),
        "$",
        &mut out,
    );
    out
}

#[test]
fn the_request_golden_is_what_the_real_request_type_reads() {
    let body = golden(REQUEST);
    assert_eq!(
        body,
        canonical_request_body(),
        "the request golden is generated, never edited"
    );

    let request: RetrievalRequest = serde_json::from_value(body.clone()).expect("parses");
    assert_eq!(request.zdr_mode, Some(ZdrMode::Disabled));
    assert_eq!(request.sovereign_required, Some(false));
    assert_eq!(request.filters.sources, vec!["quarry".to_owned()]);
    assert_eq!(
        request.mode_mix.as_ref().and_then(|m| m.rerank),
        Some(false)
    );
    assert_eq!(request.top_n, Some(10));
    assert_eq!(
        request.user_id, None,
        "the viewer is never taken from a body"
    );
    assert!(!request.admin_read_all && !request.service_principal);

    assert_eq!(request_misses(&body), Vec::<String>::new());
}

#[test]
fn the_key_check_catches_a_field_the_engine_would_silently_ignore() {
    // Quarry's own name for the posture, and an invented alias for the url: both
    // parse without error today and do nothing, which is the failure this guards.
    let mut body = canonical_request_body();
    body["zdr"] = json!(true);
    body["filters"]["source_url"] = json!("quarry");
    let found = request_misses(&body);
    assert!(found.iter().any(|m| m.starts_with("$.zdr ")), "{found:?}");
    assert!(
        found.iter().any(|m| m.starts_with("$.filters.source_url ")),
        "{found:?}"
    );

    // Boundary-only fields in a body are overwritten by the auth layer, never read.
    let mut spoofed = canonical_request_body();
    spoofed["user_id"] = json!("someone-else");
    spoofed["admin_read_all"] = json!(true);
    spoofed["service_principal"] = json!(true);
    let found = request_misses(&spoofed);
    assert_eq!(found.len(), 3, "{found:?}");
}

// ───────────────────────── zdr_mode is strict ─────────────────────────

#[test]
fn zdr_mode_off_is_refused_and_so_is_every_other_spelling() {
    for spelling in REFUSED_ZDR_MODES {
        assert!(
            ZdrMode::from_str(spelling).is_err(),
            "{spelling:?} must not parse (the gRPC path uses FromStr)"
        );
        assert!(
            serde_json::from_value::<ZdrMode>(json!(spelling)).is_err(),
            "{spelling:?} must not deserialise (the HTTP path uses serde)"
        );
        let mut body = canonical_request_body();
        body["zdr_mode"] = json!(spelling);
        assert!(
            serde_json::from_value::<RetrievalRequest>(body).is_err(),
            "a body carrying zdr_mode {spelling:?} must be rejected whole"
        );
    }
    // Not a string at all.
    for value in [
        json!(true),
        json!(0),
        json!(["disabled"]),
        json!({"mode": "disabled"}),
    ] {
        assert!(
            serde_json::from_value::<ZdrMode>(value.clone()).is_err(),
            "{value}"
        );
    }
}

#[test]
fn exactly_the_three_documented_zdr_modes_are_accepted() {
    // Exhaustive on purpose: adding a variant breaks this match, and then the
    // golden and Quarry's typed map have to be extended with it.
    fn index(mode: ZdrMode) -> usize {
        match mode {
            ZdrMode::Disabled => 0,
            ZdrMode::Reject => 1,
            ZdrMode::Ephemeral => 2,
        }
    }
    assert_eq!(ALL_MODES.map(index), [0, 1, 2]);

    for (mode, spelling) in ALL_MODES.iter().zip(["disabled", "reject", "ephemeral"]) {
        assert_eq!(mode.as_str(), spelling);
        assert_eq!(ZdrMode::from_str(spelling), Ok(*mode));
        assert_eq!(
            serde_json::from_value::<ZdrMode>(json!(spelling)).expect("parses"),
            *mode
        );
        assert_eq!(
            serde_json::to_value(mode).expect("serialisable"),
            json!(spelling)
        );
    }
}

#[test]
fn the_zdr_modes_golden_is_derived_from_the_real_enum_and_rules() {
    let golden = golden(ZDR_MODES);
    assert_eq!(
        golden,
        zdr_modes_contract(),
        "regenerate deliberately and re-vendor into Quarry"
    );
    assert_eq!(golden["default_when_absent"], "disabled");
    assert_eq!(golden["sovereign_required_when_absent"], true);
    assert_eq!(golden["text_rerank_when_mode_mix_rerank_is_false"], false);
    let accepted: Vec<&str> = golden["accepted"]
        .as_array()
        .expect("accepted")
        .iter()
        .map(|entry| entry["value"].as_str().expect("value"))
        .collect();
    assert_eq!(accepted, ["disabled", "reject", "ephemeral"]);
}

/// The status a caller sees for a refused `zdr_mode`, with the real request type
/// behind the real axum JSON extractor (the engine's `retrieve` handler takes
/// `Json<RetrievalRequest>`). The handler body is a counter: it must never run.
#[tokio::test]
async fn an_unknown_zdr_mode_is_refused_by_the_json_extractor_before_any_handler_runs() {
    use axum::extract::State;
    use axum::routing::post;
    use axum::{Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    async fn handler(
        State(calls): State<Arc<AtomicUsize>>,
        Json(request): Json<RetrievalRequest>,
    ) -> Json<Value> {
        calls.fetch_add(1, Ordering::SeqCst);
        Json(json!({ "zdr_mode": request.zdr_mode }))
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/v1/retrieve", post(handler))
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let client = reqwest::Client::new();
    let post_body = |body: Value| {
        let client = client.clone();
        async move {
            client
                .post(format!("http://{addr}/v1/retrieve"))
                .json(&body)
                .send()
                .await
                .expect("request")
        }
    };

    let accepted = post_body(golden(REQUEST)).await;
    assert_eq!(accepted.status(), 200, "the request golden must be served");
    assert_eq!(
        accepted.json::<Value>().await.expect("json"),
        json!({ "zdr_mode": "disabled" })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let mut off = golden(REQUEST);
    off["zdr_mode"] = json!("off");
    let refused = post_body(off).await;
    assert_eq!(
        refused.status(),
        422,
        "documented in openapi/d4d5.yaml: an unknown zdr_mode is an unprocessable body"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a refused body must never reach the handler"
    );
}

// ───────────────────────── rerank and sovereignty defaults ─────────────────────────

#[test]
fn mode_mix_rerank_false_means_no_text_rerank() {
    // The vector leg sends `mode_mix.rerank=false`: no cross-encoder call, so
    // `rerank_used_count` stays 0 and the confidence gate is inactive.
    assert!(!text_rerank_requested(
        ZdrMode::Disabled,
        Some(false),
        false
    ));
    // Absent means on, an explicit true means on.
    assert!(text_rerank_requested(ZdrMode::Disabled, None, false));
    assert!(text_rerank_requested(ZdrMode::Disabled, Some(true), false));
    // A restrictive posture or sovereignty wins over an explicit opt-in.
    for mode in [ZdrMode::Reject, ZdrMode::Ephemeral] {
        assert!(!text_rerank_requested(mode, Some(true), false), "{mode:?}");
        assert!(!text_rerank_requested(mode, None, false), "{mode:?}");
    }
    assert!(!text_rerank_requested(ZdrMode::Disabled, Some(true), true));
    assert!(!text_rerank_requested(ZdrMode::Disabled, None, true));
}

#[test]
fn an_absent_sovereign_required_is_strict() {
    assert!(sovereign_required_or_strict(None));
    assert!(sovereign_required_or_strict(Some(true)));
    assert!(!sovereign_required_or_strict(Some(false)));
}

// ───────────────────────── the real SQL under the real RLS policy (DB-gated) ─────────────────────────

const ORG: &str = "org-qbt04-a";
const OTHER_ORG: &str = "org-qbt04-b";

/// A scratch database of its own, created from the admin connection in
/// `TEST_DATABASE_URL` and dropped at the end. The real RLS migration leaves RLS
/// ENABLED on every table it covers; doing that in a database shared with the
/// sibling suites would change what they see.
struct Scratch {
    admin: PgPool,
    name: String,
    pool: PgPool,
}

impl Scratch {
    async fn create() -> Self {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL is required for this ignored test");
        let options = PgConnectOptions::from_str(&url).expect("TEST_DATABASE_URL parses");
        let admin = PgPool::connect_with(options.clone())
            .await
            .expect("connect to the admin database");
        let name = format!("qbt04_{}_test", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .expect("create the scratch database");
        let pool = PgPool::connect_with(options.database(&name))
            .await
            .expect("connect to the scratch database");
        Self { admin, name, pool }
    }

    async fn drop_database(self) {
        self.pool.close().await;
        sqlx::query(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ))
        .execute(&self.admin)
        .await
        .expect("drop the scratch database");
    }
}

async fn setup_schema(pool: &PgPool) {
    // Only the columns the join reads, plus the ones the RLS migration needs.
    sqlx::query(
        "CREATE TABLE documents (
            document_id TEXT PRIMARY KEY,
            org_id      TEXT NOT NULL,
            source      TEXT NOT NULL,
            type        TEXT NOT NULL,
            title       TEXT NOT NULL,
            metadata    JSONB NOT NULL DEFAULT '{}',
            deleted_at  TIMESTAMPTZ
        )",
    )
    .execute(pool)
    .await
    .expect("create documents");
    sqlx::query(
        "CREATE TABLE wiki_pages (
            page_id TEXT PRIMARY KEY,
            org_id  TEXT NOT NULL,
            title   TEXT NOT NULL,
            path    TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create wiki_pages");
    // Creates the runtime role race-safely (roles are cluster-wide) and grants it
    // the tables above; must follow the CREATE TABLEs.
    common::grant_rls_runtime_role(pool).await;

    // The production RLS migration itself, not a copy of its policy: it enables
    // RLS on every listed table that exists and skips the rest. Read at runtime
    // so this file does not need the infra directory to compile.
    let migration = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../infra/postgres/migrations/20260809120000_org_rls_isolation.sql");
    let sql = std::fs::read_to_string(&migration)
        .unwrap_or_else(|error| panic!("the RLS migration is missing at {migration:?}: {error}"));
    sqlx::raw_sql(&sql)
        .execute(pool)
        .await
        .expect("apply the RLS migration");
}

async fn insert_document(
    pool: &PgPool,
    org: &str,
    id: &str,
    source: &str,
    kind: &str,
    metadata: Value,
) {
    sqlx::query(
        "INSERT INTO documents (document_id, org_id, source, type, title, metadata)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(org)
    .bind(source)
    .bind(kind)
    .bind(format!("title of {id}"))
    .bind(metadata)
    .execute(pool)
    .await
    .expect("insert document");
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn the_source_join_returns_a_url_only_for_http_quarry_documents() {
    let scratch = Scratch::create().await;
    let pool = &scratch.pool;
    setup_schema(pool).await;

    let page = "https://docs.example.test/page";
    for (org, id, source, kind, metadata) in [
        (ORG, "q-http", "quarry", "web_page", json!({"url": page})),
        (
            ORG,
            "q-ftp",
            "quarry",
            "web_page",
            json!({"url": "ftp://h/x"}),
        ),
        (
            ORG,
            "q-userinfo",
            "quarry",
            "web_page",
            json!({"url": "https://user:secret@docs.example.test/"}),
        ),
        (
            ORG,
            "q-nourl",
            "quarry",
            "web_page",
            json!({"fingerprint": "x"}),
        ),
        (ORG, "q-number", "quarry", "web_page", json!({"url": 42})),
        // A connector document carrying the same key must never expose it.
        (
            ORG,
            "sp",
            "sharepoint",
            "sharepoint_page",
            json!({"url": page}),
        ),
        (
            OTHER_ORG,
            "q-other-org",
            "quarry",
            "web_page",
            json!({"url": page}),
        ),
    ] {
        insert_document(pool, org, id, source, kind, metadata).await;
    }
    sqlx::query(
        "INSERT INTO wiki_pages (page_id, org_id, title, path) VALUES ('wiki-1', $1, 'Wiki', 'wiki/one')",
    )
    .bind(ORG)
    .execute(pool)
    .await
    .expect("insert wiki page");

    let ids: Vec<String> = [
        "q-http",
        "q-ftp",
        "q-userinfo",
        "q-nourl",
        "q-number",
        "sp",
        "wiki-1",
        "q-other-org",
    ]
    .iter()
    .map(|id| (*id).to_owned())
    .collect();
    let joined = fetch_source_refs(pool, ORG, &ids).await;
    // Drop the scratch database before asserting so a failure leaves nothing behind.
    scratch.drop_database().await;

    let refs = joined.expect("the join succeeds");
    let by_id: HashMap<&str, &SourceRef> =
        refs.iter().map(|r| (r.document_id.as_str(), r)).collect();

    assert_eq!(by_id["q-http"].url.as_deref(), Some(page));
    for id in ["q-ftp", "q-userinfo", "q-nourl", "q-number", "sp", "wiki-1"] {
        assert!(by_id[id].url.is_none(), "{id} must not expose a url");
    }
    assert_eq!(by_id["q-http"].source, "quarry");
    assert_eq!(by_id["wiki-1"].r#type, "wiki");
    assert!(
        !by_id.contains_key("q-other-org"),
        "another org's document, and its url, must resolve to nothing under RLS"
    );
}
