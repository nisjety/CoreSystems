//! Structured-data harvesting — the machine-readable facts a page
//! already carries, which prose extraction throws away.
//!
//! # Why this module exists
//!
//! A user asked for Oslo's population. The crawler fetched
//! `www.ssb.no/kommunefakta/oslo` successfully — 620,966 bytes — and
//! the answer was still "not found". The figure was never in the
//! prose: it lives in 29 `<script type="application/json">` hydration
//! payloads totalling 461,890 characters (~74% of the page), one of
//! which reads
//!
//! ```text
//! {"command":"hydrate","jsxPath":"KeyFigure","props":{"keyFigures":[
//!   {"number":"729 437","numberDescription":"personer",
//!    "time":"2. kvartal 2026","title":"Folketallet"}]}}
//! ```
//!
//! [`crate::readability`] strips `script` by design — correct for
//! prose, and NOT to be changed — so 130 characters survived and the
//! answer was discarded with the markup.
//!
//! This is therefore a **second, parallel extraction channel**, not a
//! modification of the first. Readability keeps producing prose; this
//! module produces facts. Consumers run both and merge.
//!
//! # Channels
//!
//! Each source is optional and independently fallible — a page with
//! no tables, malformed JSON-LD and one good hydration blob still
//! yields that blob's figures.
//!
//! 1. **Hydration/state payloads** — `script[type="application/json"]`
//!    (the SSB/Enonic XP pattern), `__NEXT_DATA__`, and assignment
//!    forms like `window.__NUXT__ = {…}`. Highest-value source; the
//!    one that answers the incident above.
//! 2. **JSON-LD** — `script[type="application/ld+json"]`. Note the SSB
//!    page HAS a JSON-LD block and it does NOT contain the figure, so
//!    JSON-LD can never be the only channel. (Schema.org *semantics* —
//!    headline, author, offers — belong to [`crate::json_ld`]; here we
//!    only harvest figures and keep the raw blobs, in the same single
//!    document parse as every other channel.)
//! 3. **Microdata / RDFa** — `itemscope` / `itemprop` / `property`.
//! 4. **HTML tables** — a table is structured data that readability
//!    flattens into unusable prose; we keep headers and rows.
//!
//! # Two hard rules
//!
//! **Bound everything.** Every cap below is a named const with its
//! reasoning. A harvester that hands half a megabyte of framework
//! state to a language model is worse than the bug it fixes.
//!
//! **Never panic, never fail wholesale.** One malformed blob, one
//! hostile depth bomb, one 10 MB script must degrade to "that source
//! contributed nothing" — never to an error or an abort. There is no
//! `unwrap`/`expect` on parsed content anywhere in this file.

use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Bounds
// ---------------------------------------------------------------------------
//
// Sized against the incident page: 461,890 chars of JSON across 29
// script tags, of which the useful part is four short strings. The
// budgets are deliberately an order of magnitude below "everything on
// the page" — the harvester's job is to find the needle, not to ship
// the haystack.

/// Ceiling on the total payload this module returns, counted as the
/// bytes of every string it keeps (figures, tables, microdata, blobs).
/// 32 KiB is roughly 8k tokens — a page's worth of facts still leaves
/// room for the prose extraction beside it in a model context.
pub const MAX_TOTAL_BYTES: usize = 32 * 1024;

/// Largest single JSON blob retained verbatim in [`StructuredData::blobs`].
/// The SSB hydration payloads are 10-100x this; they are still *walked*
/// for figures, they just aren't echoed back. Keeping raw JSON is for
/// small self-contained state objects a consumer may want to re-read.
pub const MAX_BLOB_BYTES: usize = 8 * 1024;

/// How many raw blobs to retain. Pages that hydrate per-component ship
/// dozens; after the figure walk, the marginal blob adds bulk, not facts.
pub const MAX_BLOBS: usize = 8;

/// Figure cap. The richest real pages (statistics portals, financial
/// reports) carry a few dozen headline numbers; beyond that we are
/// almost certainly scraping framework state, not editorial content.
pub const MAX_FIGURES: usize = 64;

/// Table caps. Cells are capped both per-table (rows x cols) and in
/// aggregate, because a single hostile page can carry thousands of
/// 1x1 tables used for layout.
pub const MAX_TABLES: usize = 8;
pub const MAX_TABLE_ROWS: usize = 50;
pub const MAX_TABLE_COLS: usize = 16;
pub const MAX_TABLE_CELLS: usize = 400;

/// Per-cell text cap. Anything longer is prose that readability
/// already owns, not a data point.
pub const MAX_CELL_CHARS: usize = 200;

/// A fact's label ("Folketallet") and its scalar ("729 437"). Values
/// are held to a short leash on purpose: the keep-rule below uses
/// "short and contains a digit" as its main signal for scalar-ness.
pub const MAX_LABEL_CHARS: usize = 120;
pub const MAX_VALUE_CHARS: usize = 64;

/// Unit ("personer") and period ("2. kvartal 2026") are short labels too.
pub const MAX_QUALIFIER_CHARS: usize = 64;

/// Traversal limits for parsed JSON. Depth bounds our own recursion
/// (serde_json's parser has its own limit, so a depth bomb fails at
/// parse time and is simply skipped); the node budget bounds total
/// work across all blobs on the page.
pub const MAX_JSON_DEPTH: usize = 12;
pub const MAX_JSON_NODES: usize = 20_000;

/// Largest single script we hand to serde_json, and the aggregate
/// across the page. The incident page needs ~462 KB of the aggregate
/// budget; 2 MiB gives comparable pages headroom while keeping a
/// tarpit page (500 x 1 MB of JSON) from turning a fetch into a stall.
pub const MAX_SCRIPT_BYTES: usize = 512 * 1024;
pub const MAX_PARSE_BYTES: usize = 2 * 1024 * 1024;

/// Bytes of an inline script scanned for a `window.__STATE__ = {…}`
/// assignment. Bundled app code routinely reaches megabytes; the state
/// assignment, when present, sits near the top.
pub const MAX_ASSIGNMENT_SCAN_BYTES: usize = 256 * 1024;

/// Microdata caps. An `itemscope` per product card is normal; a
/// thousand of them is a catalogue dump.
pub const MAX_MICRODATA_ITEMS: usize = 32;
pub const MAX_MICRODATA_PROPS: usize = 24;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Where a harvested item came from. Consumers weigh sources
/// differently — a downstream feature decides whether a figure
/// *authoritatively* answers a question, and "the publisher's own
/// hydration state" and "some JSON we found" are not the same claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FigureSource {
    /// Framework hydration/state payload (`application/json`,
    /// `__NEXT_DATA__`, `window.__NUXT__`, …).
    Hydration,
    /// schema.org JSON-LD.
    JsonLd,
    /// Microdata/RDFa attributes in the markup.
    Microdata,
}

/// A labelled scalar fact: the shape a question can actually be
/// answered from. `unit` and `period` are what let a consumer say
/// "729 437 personer, 2. kvartal 2026" instead of "729437".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyFigure {
    /// Human label — "Folketallet".
    pub label: String,
    /// Scalar as presented, separators intact — "729 437".
    pub value: String,
    /// Unit or number description — "personer", "%", "NOK".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Reference period or timestamp — "2. kvartal 2026".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    pub source: FigureSource,
}

impl KeyFigure {
    /// How well-qualified this figure is (0-2). Used when the figure
    /// budget is full: a figure carrying both a unit and a period
    /// outranks a bare label/number pair. The incident figure carries
    /// both, so it survives a page that floods us with weak ones.
    fn rank(&self) -> u8 {
        u8::from(self.unit.is_some()) + u8::from(self.period.is_some())
    }

    fn byte_cost(&self) -> usize {
        self.label.len()
            + self.value.len()
            + self.unit.as_ref().map_or(0, String::len)
            + self.period.as_ref().map_or(0, String::len)
    }
}

/// An HTML table, kept as headers + rows rather than flattened text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// Header cells, empty when the table has no `th` row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,
    /// Body rows; each row is at most [`MAX_TABLE_COLS`] cells and is
    /// NOT padded to the header width — ragged tables stay ragged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Vec<String>>,
}

/// One `itemscope` (or RDFa `typeof`) subtree, flattened to its
/// property/value pairs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MicrodataItem {
    /// `itemtype`/`typeof` value when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    /// `(property name, value)` in document order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<(String, String)>,
}

/// Provenance for a retained raw JSON blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlobSource {
    /// `<script type="application/json">` / `__NEXT_DATA__`.
    Hydration,
    /// `<script type="application/ld+json">`.
    JsonLd,
    /// `window.__NUXT__ = {…}` and friends.
    Assignment,
}

/// A small JSON object kept verbatim, for consumers that need a field
/// our keep-rule dropped. Bounded by [`MAX_BLOB_BYTES`]/[`MAX_BLOBS`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawBlob {
    pub source: BlobSource,
    pub json: Value,
}

/// Everything the harvester found. Always returned (possibly empty) —
/// consumers merge this with prose extraction and an empty harvest is
/// an ordinary outcome, not an error.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StructuredData {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub figures: Vec<KeyFigure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<Table>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub microdata: Vec<MicrodataItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blobs: Vec<RawBlob>,
    /// Set when any cap above was hit. Signals "the page carried more
    /// than we kept" — useful for deciding whether to escalate a page
    /// to a browser render rather than silently trusting a partial harvest.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

impl StructuredData {
    pub fn is_empty(&self) -> bool {
        self.figures.is_empty()
            && self.tables.is_empty()
            && self.microdata.is_empty()
            && self.blobs.is_empty()
    }

    /// Figures whose label contains `needle`, case-insensitively.
    /// The question-answering path wants "the figure labelled
    /// *folketall*", not a scan of everything we found.
    pub fn figures_labelled(&self, needle: &str) -> Vec<&KeyFigure> {
        let needle = needle.to_lowercase();
        self.figures
            .iter()
            .filter(|f| f.label.to_lowercase().contains(&needle))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

/// Running accounting for every cap in this module. Threaded through
/// extraction by `&mut` so one global budget covers all channels — a
/// page that spends it all on tables gets fewer blobs, not extra bytes.
#[derive(Debug, Default)]
struct Budget {
    bytes: usize,
    nodes: usize,
    parsed_bytes: usize,
    cells: usize,
    truncated: bool,
}

impl Budget {
    /// Reserve `n` output bytes; `false` means the total cap is spent.
    fn spend(&mut self, n: usize) -> bool {
        if self.bytes + n > MAX_TOTAL_BYTES {
            self.truncated = true;
            return false;
        }
        self.bytes += n;
        true
    }

    /// Charge one JSON node of traversal work.
    fn tick_node(&mut self) -> bool {
        self.nodes += 1;
        if self.nodes > MAX_JSON_NODES {
            self.truncated = true;
            return false;
        }
        true
    }

    /// Reserve parser input; keeps a tarpit page from monopolising CPU.
    fn allow_parse(&mut self, n: usize) -> bool {
        if n > MAX_SCRIPT_BYTES || self.parsed_bytes + n > MAX_PARSE_BYTES {
            self.truncated = true;
            return false;
        }
        self.parsed_bytes += n;
        true
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn script_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("script").expect("static selector"))
}

/// Harvest every structured-data channel from a raw HTML document.
///
/// Never errors and never panics: each channel is independently
/// fallible, and a source that fails (unparseable JSON, a table with
/// no cells, a script past the size cap) contributes nothing while the
/// others proceed.
pub fn extract(html: &str) -> StructuredData {
    let doc = Html::parse_document(html);
    let mut budget = Budget::default();
    let mut out = StructuredData::default();

    for el in doc.select(script_selector()) {
        let ty = el
            .value()
            .attr("type")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let id = el.value().attr("id").unwrap_or_default();

        if ty.contains("ld+json") {
            harvest_script_json(el, BlobSource::JsonLd, &mut budget, &mut out);
        } else if ty == "application/json"
            || ty.ends_with("+json")
            || (ty.is_empty() && is_state_id(id))
        {
            harvest_script_json(el, BlobSource::Hydration, &mut budget, &mut out);
        } else if ty.is_empty() || ty.contains("javascript") || ty == "module" {
            harvest_assignments(el, &mut budget, &mut out);
        }
    }

    harvest_microdata(&doc, &mut budget, &mut out);
    harvest_tables(&doc, &mut budget, &mut out);

    out.truncated = budget.truncated;
    out
}

/// Well-known ids used by frameworks that embed state in an untyped
/// script tag. Typed `application/json` scripts are caught by type.
fn is_state_id(id: &str) -> bool {
    matches!(
        id,
        "__NEXT_DATA__" | "__NUXT_DATA__" | "__INITIAL_STATE__" | "serverApp-state"
    )
}

fn figure_source_of(blob: BlobSource) -> FigureSource {
    match blob {
        BlobSource::JsonLd => FigureSource::JsonLd,
        BlobSource::Hydration | BlobSource::Assignment => FigureSource::Hydration,
    }
}

fn harvest_script_json(
    el: ElementRef<'_>,
    source: BlobSource,
    budget: &mut Budget,
    out: &mut StructuredData,
) {
    let raw = el.text().collect::<String>();
    let cleaned = clean_json_text(&raw);
    if cleaned.is_empty() {
        return;
    }
    if !budget.allow_parse(cleaned.len()) {
        return;
    }
    // A blob that doesn't parse is a blob we skip — publishers ship
    // subtly invalid JSON constantly, and one bad block must never
    // cost us the other 28 on the page.
    let Ok(value) = serde_json::from_str::<Value>(cleaned) else {
        return;
    };
    absorb_value(&value, source, budget, out);
}

/// Strip the wrappers publishers put around embedded JSON: a BOM, an
/// HTML comment fence (a 1990s trick that never died), CDATA.
fn clean_json_text(raw: &str) -> &str {
    raw.trim()
        .trim_start_matches('\u{feff}')
        .trim()
        .trim_start_matches("<!--")
        .trim_end_matches("-->")
        .trim()
        .trim_start_matches("<![CDATA[")
        .trim_end_matches("]]>")
        .trim()
}

fn absorb_value(value: &Value, source: BlobSource, budget: &mut Budget, out: &mut StructuredData) {
    walk_json(value, 0, figure_source_of(source), budget, out);
    retain_blob(value, source, budget, out);
}

/// Keep a blob verbatim only when it is small enough to be worth
/// echoing. Large state payloads have already been mined for figures;
/// re-emitting them is exactly the failure mode this module exists to
/// avoid.
fn retain_blob(value: &Value, source: BlobSource, budget: &mut Budget, out: &mut StructuredData) {
    if out.blobs.len() >= MAX_BLOBS {
        budget.truncated = true;
        return;
    }
    let Ok(encoded) = serde_json::to_string(value) else {
        return;
    };
    if encoded.len() > MAX_BLOB_BYTES {
        budget.truncated = true;
        return;
    }
    if !budget.spend(encoded.len()) {
        return;
    }
    out.blobs.push(RawBlob {
        source,
        json: value.clone(),
    });
}

// ---------------------------------------------------------------------------
// The keep/drop rule — the heart of this module
// ---------------------------------------------------------------------------
//
// Most hydration state is framework bookkeeping: routing tables,
// feature flags, asset manifests, analytics ids, build hashes. The
// facts are a rounding error inside it. The rule:
//
//   KEEP an object when it pairs a *label* with a *scalar that looks
//   presentable*, and prefer it when a unit or a period comes with it.
//
//   - label: a short, human-readable string under a naming key
//     (`title`, `label`, `name`, `heading`, …) that contains letters
//     and is not a URL, path, hash or config token.
//   - scalar: a number, or a short string containing a digit — "729 437",
//     "12,5 %", "NOK 1 299". Presentation separators are preserved
//     because they carry locale meaning a naive parse would destroy.
//   - unit/period: optional, from `numberDescription`/`unit`/`unitText`
//     and `time`/`period`/`year`/…. Their presence is what makes a
//     figure quotable, so it also decides survival when the cap is hit.
//
//   DROP: booleans, nulls, containers-as-values, ids (uuid/hex/long
//   digit runs/epoch-sized integers), URLs and paths, anything over the
//   length caps, and labels that are plainly machine knobs (`version`,
//   `zIndex`, `width`, …). Recursion additionally skips subtrees under
//   keys that are near-certainly framework bookkeeping.
//
// The rule is intentionally conservative in one direction: it is far
// worse to bury one real figure in 500 flags than to miss a figure
// stored under an unusual key. Pruning subtrees can in principle drop
// a fact, so the prune list stays short and obvious.

/// Keys whose value is the figure's human label.
const LABEL_KEYS: &[&str] = &[
    "title",
    "label",
    "heading",
    "name",
    "caption",
    "displayname",
    "term",
    "keyfiguretitle",
];

/// Keys whose value is the figure's scalar.
const VALUE_KEYS: &[&str] = &[
    "number", "value", "amount", "figure", "count", "total", "sum", "val",
];

/// Keys carrying the unit / number description.
const UNIT_KEYS: &[&str] = &[
    "numberdescription",
    "unit",
    "unittext",
    "unitcode",
    "uom",
    "measure",
    "suffix",
];

/// Keys carrying the reference period.
const PERIOD_KEYS: &[&str] = &[
    "time",
    "period",
    "timeperiod",
    "year",
    "quarter",
    "asof",
    "referencetime",
    "datepublished",
    "validfrom",
    "date",
];

/// Subtrees that are framework bookkeeping with near-certainty. Kept
/// short on purpose: every entry is a place a fact could theoretically
/// hide, so the bar is "no publisher puts editorial numbers here".
const PRUNE_KEYS: &[&str] = &[
    "buildid",
    "assetprefix",
    "runtimeconfig",
    "webpack",
    "manifest",
    "assets",
    "scripts",
    "stylesheets",
    "analytics",
    "gtm",
    "tracking",
    "featureflags",
    "experiments",
    "csrf",
    "session",
    "cookies",
    "router",
    "routes",
];

/// Labels that name a machine knob rather than a fact. A "Version: 3"
/// pair is structurally identical to "Folketallet: 729 437"; only the
/// label tells them apart.
const NON_FACT_LABELS: &[&str] = &[
    "version",
    "id",
    "index",
    "order",
    "sortorder",
    "width",
    "height",
    "zindex",
    "opacity",
    "priority",
    "port",
    "offset",
    "limit",
    "page",
    "step",
    "revision",
];

/// Normalise a key or label for comparison: lowercase, minus the
/// separators that distinguish `numberDescription` from `number_description`.
fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn lookup<'a>(map: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    for wanted in keys {
        for (k, v) in map {
            if normalize_key(k) == *wanted {
                return Some(v);
            }
        }
    }
    None
}

/// A presentable scalar, or `None`.
fn scalar_fact(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() || t.chars().count() > MAX_VALUE_CHARS {
                return None;
            }
            if !t.chars().any(|c| c.is_ascii_digit()) {
                return None;
            }
            if looks_like_identifier(t) {
                return None;
            }
            Some(t.to_string())
        }
        Value::Number(n) => {
            // Integers at epoch-millisecond scale (and beyond) are
            // timestamps and ids, never figures a page displays.
            if let Some(i) = n.as_i64() {
                if i.unsigned_abs() >= 1_000_000_000_000 {
                    return None;
                }
            }
            let s = n.to_string();
            if s.chars().count() > MAX_VALUE_CHARS {
                return None;
            }
            Some(s)
        }
        _ => None,
    }
}

/// URLs, paths, uuids, content hashes and long digit runs — the shapes
/// ids take. Cheap syntactic checks only; no regex, no allocation.
fn looks_like_identifier(s: &str) -> bool {
    if s.contains("://") || s.starts_with('/') || s.starts_with("www.") {
        return true;
    }
    let len = s.len();
    // Bare digit runs of timestamp/id length.
    if len >= 13 && s.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    // Content hashes: long and hex-only.
    if len >= 16 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return true;
    }
    // uuid shape: 8-4-4-4-12.
    if len == 36 {
        let groups: Vec<&str> = s.split('-').collect();
        if groups.len() == 5
            && groups.iter().map(|g| g.len()).eq([8usize, 4, 4, 4, 12])
            && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
        {
            return true;
        }
    }
    false
}

fn label_text(value: &Value) -> Option<String> {
    let Value::String(s) = value else {
        return None;
    };
    let t = s.trim();
    if t.is_empty() || t.chars().count() > MAX_LABEL_CHARS {
        return None;
    }
    if !t.chars().any(char::is_alphabetic) {
        return None;
    }
    if looks_like_identifier(t) {
        return None;
    }
    if NON_FACT_LABELS.contains(&normalize_key(t).as_str()) {
        return None;
    }
    Some(t.to_string())
}

fn qualifier_text(value: Option<&Value>) -> Option<String> {
    let Some(Value::String(s)) = value else {
        // A bare year (`"year": 2026`) is a legitimate period.
        if let Some(Value::Number(n)) = value {
            return Some(n.to_string());
        }
        return None;
    };
    let t = s.trim();
    if t.is_empty() || t.chars().count() > MAX_QUALIFIER_CHARS {
        return None;
    }
    Some(t.to_string())
}

fn figure_from_object(map: &Map<String, Value>, source: FigureSource) -> Option<KeyFigure> {
    let label = label_text(lookup(map, LABEL_KEYS)?)?;
    let value = scalar_fact(lookup(map, VALUE_KEYS)?)?;
    Some(KeyFigure {
        label,
        value,
        unit: qualifier_text(lookup(map, UNIT_KEYS)),
        period: qualifier_text(lookup(map, PERIOD_KEYS)),
        source,
    })
}

/// Depth- and node-bounded walk. Recursion depth can never exceed
/// [`MAX_JSON_DEPTH`], so this cannot blow the stack on hostile input.
fn walk_json(
    value: &Value,
    depth: usize,
    source: FigureSource,
    budget: &mut Budget,
    out: &mut StructuredData,
) {
    if depth > MAX_JSON_DEPTH {
        budget.truncated = true;
        return;
    }
    if !budget.tick_node() {
        return;
    }
    match value {
        Value::Object(map) => {
            if let Some(fig) = figure_from_object(map, source) {
                push_figure(fig, budget, out);
            }
            for (key, child) in map {
                if PRUNE_KEYS.contains(&normalize_key(key).as_str()) {
                    continue;
                }
                walk_json(child, depth + 1, source, budget, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_json(item, depth + 1, source, budget, out);
            }
        }
        _ => {}
    }
}

/// Insert a figure, deduplicating and respecting the cap.
///
/// When the cap is full we don't simply drop: a richer figure (one
/// carrying a unit and a period) displaces the weakest one held. The
/// incident figure is fully qualified and appears inside one of 29
/// blobs — dropping in arrival order would let a page of bare
/// label/number pairs push the only real answer out.
fn push_figure(fig: KeyFigure, budget: &mut Budget, out: &mut StructuredData) {
    if out.figures.contains(&fig) {
        return;
    }
    if out.figures.len() < MAX_FIGURES {
        if budget.spend(fig.byte_cost()) {
            out.figures.push(fig);
        }
        return;
    }
    budget.truncated = true;
    let Some((idx, weakest)) = out
        .figures
        .iter()
        .enumerate()
        .min_by_key(|(_, f)| f.rank())
        .map(|(i, f)| (i, f.rank()))
    else {
        return;
    };
    if fig.rank() > weakest {
        // Return the evicted figure's bytes before charging the new
        // one, so a long run of replacements can't drift past the cap.
        let freed = out.figures[idx].byte_cost();
        budget.bytes = budget.bytes.saturating_sub(freed);
        if budget.spend(fig.byte_cost()) {
            out.figures[idx] = fig;
        } else {
            budget.bytes += freed;
        }
    }
}

// ---------------------------------------------------------------------------
// Assignment-form state (`window.__NUXT__ = {…}`)
// ---------------------------------------------------------------------------

/// Globals that carry hydration state as a JS assignment rather than a
/// JSON script tag.
const ASSIGNMENT_MARKERS: &[&str] = &[
    "__NUXT__",
    "__INITIAL_STATE__",
    "__PRELOADED_STATE__",
    "__APOLLO_STATE__",
    "__INITIAL_DATA__",
    "__NEXT_DATA__",
];

fn harvest_assignments(el: ElementRef<'_>, budget: &mut Budget, out: &mut StructuredData) {
    let text = el.text().collect::<String>();
    if text.is_empty() {
        return;
    }
    let scan = clip_bytes(&text, MAX_ASSIGNMENT_SCAN_BYTES);
    for marker in ASSIGNMENT_MARKERS {
        let Some(pos) = scan.find(marker) else {
            continue;
        };
        let after = &scan[pos + marker.len()..];
        let Some(eq) = after.find('=') else {
            continue;
        };
        let rest = &after[eq + 1..];
        // Only a literal object/array is JSON. `__NUXT__=(function(a,b)
        // {…}(…))` and other IIFE forms are JS, not data — we skip them
        // rather than trying to evaluate anything.
        let Some(start) = rest.find(['{', '[']) else {
            continue;
        };
        let Some(slice) = balanced_json_slice(&rest[start..]) else {
            continue;
        };
        if !budget.allow_parse(slice.len()) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(slice) else {
            continue;
        };
        absorb_value(&value, BlobSource::Assignment, budget, out);
    }
}

/// Return the leading balanced `{…}`/`[…]` of `s`, string- and
/// escape-aware. Byte indexing is safe: every delimiter we cut on is
/// ASCII, so slices always land on char boundaries.
fn balanced_json_slice(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let open = bytes.first().copied()?;
    let close = match open {
        b'{' => b'}',
        b'[' => b']',
        _ => return None,
    };
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            x if x == open => depth += 1,
            x if x == close => {
                // Cannot underflow given the leading opener, but a
                // checked read costs nothing and this parses hostile input.
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Microdata / RDFa
// ---------------------------------------------------------------------------

fn itemscope_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("[itemscope], [typeof]").expect("static selector"))
}

fn itemprop_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("[itemprop], [property]").expect("static selector"))
}

fn harvest_microdata(doc: &Html, budget: &mut Budget, out: &mut StructuredData) {
    for scope in doc.select(itemscope_selector()) {
        if out.microdata.len() >= MAX_MICRODATA_ITEMS {
            budget.truncated = true;
            return;
        }
        let item_type = scope
            .value()
            .attr("itemtype")
            .or_else(|| scope.value().attr("typeof"))
            .map(|t| collapse_ws(t, MAX_LABEL_CHARS));
        let mut item = MicrodataItem {
            item_type,
            properties: Vec::new(),
        };

        for prop in scope.select(itemprop_selector()) {
            if item.properties.len() >= MAX_MICRODATA_PROPS {
                budget.truncated = true;
                break;
            }
            let Some(name) = prop
                .value()
                .attr("itemprop")
                .or_else(|| prop.value().attr("property"))
            else {
                continue;
            };
            let name = collapse_ws(name, MAX_LABEL_CHARS);
            // `content`/`datetime` hold the machine-readable form when
            // the visible text is formatted for humans; prefer them.
            let raw = prop
                .value()
                .attr("content")
                .or_else(|| prop.value().attr("datetime"))
                .map(str::to_string)
                .unwrap_or_else(|| prop.text().collect::<String>());
            let value = collapse_ws(&raw, MAX_CELL_CHARS);
            if name.is_empty() || value.is_empty() {
                continue;
            }
            if !budget.spend(name.len() + value.len()) {
                break;
            }
            // A microdata pair IS a labelled scalar when the value
            // looks like one — surface it as a figure too, so the
            // consumer has one place to look for facts.
            if let Some(scalar) = scalar_fact(&Value::String(value.clone())) {
                if let Some(label) = label_text(&Value::String(name.clone())) {
                    push_figure(
                        KeyFigure {
                            label,
                            value: scalar,
                            unit: None,
                            period: None,
                            source: FigureSource::Microdata,
                        },
                        budget,
                        out,
                    );
                }
            }
            item.properties.push((name, value));
        }

        if !item.properties.is_empty() {
            out.microdata.push(item);
        }
    }
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

fn table_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("table").expect("static selector"))
}

fn row_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("tr").expect("static selector"))
}

fn cell_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("th, td").expect("static selector"))
}

fn header_cell_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("th").expect("static selector"))
}

fn caption_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("caption").expect("static selector"))
}

fn harvest_tables(doc: &Html, budget: &mut Budget, out: &mut StructuredData) {
    for table in doc.select(table_selector()) {
        if out.tables.len() >= MAX_TABLES {
            budget.truncated = true;
            return;
        }
        if let Some(t) = table_from(table, budget) {
            out.tables.push(t);
        }
    }
}

fn table_from(el: ElementRef<'_>, budget: &mut Budget) -> Option<Table> {
    let caption = el
        .select(caption_selector())
        .next()
        .map(|c| collapse_ws(&c.text().collect::<String>(), MAX_CELL_CHARS))
        .filter(|c| !c.is_empty());

    let mut headers: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();

    for tr in el.select(row_selector()) {
        if rows.len() >= MAX_TABLE_ROWS {
            budget.truncated = true;
            break;
        }
        if budget.cells >= MAX_TABLE_CELLS {
            budget.truncated = true;
            break;
        }
        // The first row made of `th` is the header; later `th` rows are
        // section breaks inside the body and stay rows.
        let is_header = headers.is_empty()
            && tr.select(header_cell_selector()).next().is_some()
            && tr.select(cell_selector()).count() == tr.select(header_cell_selector()).count();

        let mut cells: Vec<String> = Vec::new();
        for cell in tr.select(cell_selector()) {
            if cells.len() >= MAX_TABLE_COLS {
                budget.truncated = true;
                break;
            }
            budget.cells += 1;
            if budget.cells > MAX_TABLE_CELLS {
                budget.truncated = true;
                break;
            }
            let text = collapse_ws(&cell.text().collect::<String>(), MAX_CELL_CHARS);
            if !budget.spend(text.len()) {
                break;
            }
            cells.push(text);
        }
        if cells.iter().all(String::is_empty) {
            continue;
        }
        if is_header {
            headers = cells;
        } else {
            rows.push(cells);
        }
    }

    if headers.is_empty() && rows.is_empty() {
        return None;
    }
    Some(Table {
        caption,
        headers,
        rows,
    })
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

/// Truncate to at most `max` bytes without ever splitting a UTF-8
/// character — slicing mid-character is a panic, and every string here
/// comes off the network.
fn clip_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Collapse runs of whitespace and cap length in *characters* (never
/// bytes — slicing a multi-byte char is a panic, and this module runs
/// on arbitrary network input).
fn collapse_ws(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for word in s.split_whitespace() {
        if out.chars().count() >= max_chars {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    if out.chars().count() > max_chars {
        out = out.chars().take(max_chars).collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The incident, reproduced: SSB/Enonic XP ships its key figures
    /// as a hydration command in a `type="application/json"` script.
    /// Readability strips it; this channel must not.
    const SSB_FIXTURE: &str = r##"<!doctype html><html lang="nb"><head>
<title>Oslo - kommunefakta - SSB</title>
<script type="application/ld+json">
{"@context":"https://schema.org","@type":"WebPage","name":"Kommunefakta Oslo",
 "description":"Tall og fakta om Oslo kommune"}
</script>
</head><body>
<h1>Kommunefakta Oslo</h1>
<p>Fakta om befolkning og areal.</p>
<script type="application/json">
{"command":"hydrate","jsxPath":"KeyFigure","props":{"keyFigures":[{"number":"729 437","numberDescription":"personer","time":"2. kvartal 2026","title":"Folketallet"}]}}
</script>
<script type="application/json">
{"command":"hydrate","jsxPath":"Menu","props":{"buildId":"a1b2c3d4e5f6a7b8","routes":[{"name":"Forside","value":"1"}],"locale":"nb","timestamp":1758000000000}}
</script>
</body></html>"##;

    #[test]
    fn harvests_the_ssb_population_figure() {
        let data = extract(SSB_FIXTURE);
        let fig = data
            .figures
            .iter()
            .find(|f| f.label == "Folketallet")
            .expect("the incident figure must survive extraction");
        assert_eq!(fig.value, "729 437");
        assert_eq!(fig.unit.as_deref(), Some("personer"));
        assert_eq!(fig.period.as_deref(), Some("2. kvartal 2026"));
        assert_eq!(fig.source, FigureSource::Hydration);
    }

    #[test]
    fn ssb_figure_is_findable_by_label() {
        let data = extract(SSB_FIXTURE);
        let hits = data.figures_labelled("folketall");
        assert_eq!(hits.len(), 1, "figures = {:?}", data.figures);
        assert_eq!(hits[0].value, "729 437");
    }

    #[test]
    fn drops_framework_bookkeeping_from_the_same_page() {
        let data = extract(SSB_FIXTURE);
        // `routes` is pruned, so its {name, value} pair never becomes a
        // figure; the build hash and the epoch timestamp are not facts.
        assert!(
            !data.figures.iter().any(|f| f.label == "Forside"),
            "pruned subtree leaked: {:?}",
            data.figures
        );
        assert!(!data
            .figures
            .iter()
            .any(|f| f.value.contains("1758000000000")));
        assert!(!data.figures.iter().any(|f| f.value.contains("a1b2c3d4")));
    }

    #[test]
    fn json_ld_alone_would_not_have_answered() {
        // Guard on the premise: the SSB JSON-LD block carries no
        // figure, so JSON-LD must never be the only channel.
        let ld_only = r##"<script type="application/ld+json">
        {"@context":"https://schema.org","@type":"WebPage","name":"Kommunefakta Oslo",
         "description":"Tall og fakta om Oslo kommune"}</script>"##;
        let data = extract(ld_only);
        assert!(data.figures.is_empty());
        assert_eq!(data.blobs.len(), 1);
        assert_eq!(data.blobs[0].source, BlobSource::JsonLd);
    }

    #[test]
    fn json_ld_figures_carry_their_source() {
        let html = r##"<script type="application/ld+json">
        {"@context":"https://schema.org","@type":"Product","name":"Vitamin C Serum",
         "offers":{"@type":"Offer","price":"299.00","priceCurrency":"NOK"},
         "weight":{"@type":"QuantitativeValue","name":"Vekt","value":"50","unitText":"ml"}}
        </script>"##;
        let data = extract(html);
        let fig = data
            .figures
            .iter()
            .find(|f| f.label == "Vekt")
            .expect("quantitative value");
        assert_eq!(fig.value, "50");
        assert_eq!(fig.unit.as_deref(), Some("ml"));
        assert_eq!(fig.source, FigureSource::JsonLd);
    }

    #[test]
    fn malformed_blob_does_not_sink_the_page() {
        let html = r##"
        <script type="application/json">{ this is not json at all ,,, }</script>
        <script type="application/json">{"title":"Areal","number":"454","unit":"km2","time":"2025"}</script>
        <script type="application/ld+json">{"broken":</script>"##;
        let data = extract(html);
        assert_eq!(data.figures.len(), 1, "figures = {:?}", data.figures);
        assert_eq!(data.figures[0].label, "Areal");
        assert_eq!(data.figures[0].unit.as_deref(), Some("km2"));
    }

    #[test]
    fn harvests_assignment_form_state() {
        let html = r#"<script>
        window.__NUXT__ = {"data":[{"title":"Innbyggere","value":"51 234","unit":"personer","year":2026}]};
        window.__OTHER__ = 1;
        </script>"#;
        let data = extract(html);
        let fig = data
            .figures
            .iter()
            .find(|f| f.label == "Innbyggere")
            .expect("nuxt figure");
        assert_eq!(fig.value, "51 234");
        assert_eq!(fig.period.as_deref(), Some("2026"));
    }

    #[test]
    fn iife_state_is_skipped_not_evaluated() {
        let html = r#"<script>window.__NUXT__=(function(a,b){return {title:a,value:b}}("x","1"));</script>"#;
        let data = extract(html);
        assert!(data.figures.is_empty());
        assert!(data.blobs.is_empty());
    }

    #[test]
    fn extracts_tables_with_headers_and_rows() {
        let html = r##"<table>
          <caption>Folkemengde etter bydel</caption>
          <thead><tr><th>Bydel</th><th>Innbyggere</th></tr></thead>
          <tbody>
            <tr><td>Frogner</td><td>59 269</td></tr>
            <tr><td>Grünerløkka</td><td>62 423</td></tr>
          </tbody>
        </table>"##;
        let data = extract(html);
        assert_eq!(data.tables.len(), 1);
        let t = &data.tables[0];
        assert_eq!(t.caption.as_deref(), Some("Folkemengde etter bydel"));
        assert_eq!(t.headers, vec!["Bydel", "Innbyggere"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[1], vec!["Grünerløkka", "62 423"]);
    }

    #[test]
    fn extracts_microdata_properties_and_figures() {
        let html = r##"<div itemscope itemtype="https://schema.org/City">
          <span itemprop="name">Oslo</span>
          <span itemprop="population" content="729437">729 437</span>
        </div>"##;
        let data = extract(html);
        assert_eq!(data.microdata.len(), 1);
        let item = &data.microdata[0];
        assert_eq!(item.item_type.as_deref(), Some("https://schema.org/City"));
        assert!(item
            .properties
            .iter()
            .any(|(k, v)| k == "population" && v == "729437"));
        let fig = data
            .figures
            .iter()
            .find(|f| f.label == "population")
            .expect("microdata figure");
        assert_eq!(fig.value, "729437");
        assert_eq!(fig.source, FigureSource::Microdata);
    }

    #[test]
    fn a_qualified_figure_survives_a_flood_of_bare_ones() {
        // The incident figure sits in one blob among 29. A page that
        // floods us with weak label/number pairs must not evict it.
        let mut html = String::from("<html><body>");
        for i in 0..(MAX_FIGURES * 3) {
            html.push_str(&format!(
                r#"<script type="application/json">{{"title":"Flagg {i}","value":"{i}"}}</script>"#
            ));
        }
        html.push_str(
            r#"<script type="application/json">{"title":"Folketallet","number":"729 437","numberDescription":"personer","time":"2. kvartal 2026"}</script>"#,
        );
        html.push_str("</body></html>");

        let data = extract(&html);
        assert!(data.truncated, "the cap must be reported as hit");
        assert!(data.figures.len() <= MAX_FIGURES);
        assert!(
            data.figures.iter().any(|f| f.label == "Folketallet"),
            "the fully-qualified figure was evicted by bare ones"
        );
    }

    #[test]
    fn total_output_stays_bounded_on_a_huge_page() {
        // Stand-in for the 461,890-character reality: many big blobs.
        let mut html = String::from("<html><body>");
        for i in 0..200 {
            let filler = "x".repeat(2000);
            html.push_str(&format!(
                r#"<script type="application/json">{{"title":"Tall {i}","value":"{i}","note":"{filler}"}}</script>"#
            ));
        }
        html.push_str("</body></html>");

        let data = extract(&html);
        let encoded = serde_json::to_string(&data).expect("serializable");
        assert!(
            encoded.len() < MAX_TOTAL_BYTES * 2,
            "harvest returned {} bytes",
            encoded.len()
        );
        assert!(data.figures.len() <= MAX_FIGURES);
        assert!(data.blobs.len() <= MAX_BLOBS);
    }

    #[test]
    fn hostile_input_does_not_panic() {
        let cases = [
            String::new(),
            "<html".to_string(),
            r#"<script type="application/json"></script>"#.to_string(),
            r#"<script type="application/json">null</script>"#.to_string(),
            r#"<script type="application/json">[[[[[[[[[[1]]]]]]]]]]</script>"#.to_string(),
            // Deep nesting: serde_json rejects it, we skip it.
            format!(
                r#"<script type="application/json">{}{}</script>"#,
                "[".repeat(5000),
                "]".repeat(5000)
            ),
            // Multi-byte text right on the cell/label boundaries.
            format!("<table><tr><td>{}</td></tr></table>", "æøå🇳🇴".repeat(500)),
            r#"<script>window.__NUXT__ = {"a":"unterminated</script>"#.to_string(),
            r#"<div itemscope><span itemprop="">  </span></div>"#.to_string(),
        ];
        for case in cases {
            let _ = extract(&case);
        }
    }

    #[test]
    fn rejects_ids_hashes_and_urls_as_values() {
        let html = r##"<script type="application/json">{"items":[
          {"title":"Session","value":"550e8400-e29b-41d4-a716-446655440000"},
          {"title":"Hash","value":"deadbeefdeadbeefdeadbeef"},
          {"title":"Lenke","value":"https://ssb.no/x/1"},
          {"title":"Epoke","value":"1758000000000"},
          {"title":"Ekte","value":"12,5","unit":"%"}]}</script>"##;
        let data = extract(html);
        assert_eq!(data.figures.len(), 1, "figures = {:?}", data.figures);
        assert_eq!(data.figures[0].label, "Ekte");
        assert_eq!(data.figures[0].unit.as_deref(), Some("%"));
    }

    #[test]
    fn empty_page_yields_empty_harvest() {
        let data = extract("<html><body><p>Bare tekst.</p></body></html>");
        assert!(data.is_empty());
        assert!(!data.truncated);
    }
}
