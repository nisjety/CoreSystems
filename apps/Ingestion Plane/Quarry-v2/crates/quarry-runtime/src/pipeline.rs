//! PageRunner: fetch → transform → diff → artifacts → events → optional ingest.
//!
//! Single-page execution unit. Batch/crawl composition lives higher
//! (edge for fast path, orchestrator for durable workflows).

use chrono::Utc;
use std::sync::Arc;
use url::Url;

use quarry_core::contracts::{ChunkRef, DataPlaneIngestRequest, SourceTrace};
use quarry_core::event::EventType;
use quarry_core::ids::kinds::RunKind;
use quarry_core::output::{
    ChangeInfo, DriverInfo, FormatRef, NormalizedOutput, OutputFormats, UrlTriple,
};
use quarry_core::privacy::PrivacyPolicy;
use quarry_core::zdr::{self, WriteKind, ZdrMode};
use quarry_core::{error::ErrorCode, QuarryError, QuarryResult};
use quarry_security::SecurityEngine;
use quarry_transform::{
    chunk, diff, fingerprint::content_fingerprint, links, metadata,
    readability::html_to_readable_markdown,
};
use serde_json::json;

use crate::artifact_store::ArtifactStore;
use crate::autoscale::AutoscaledPool;
use crate::driver::{Driver, FetchHints, RenderHints};
use crate::events::EventSink;
use crate::host_scheduler::{BadKind, HostScheduler};
use crate::ingest_client::DataPlaneIngest;
use crate::local_index::{LocalDocument, TantivyLocalIndex};
use crate::policy::RunPolicy;
use crate::robots_cache::origin_key;
use crate::robots_gate::{RobotsEnforceMode, RobotsEnforcement};
use crate::source_registrar::SourceRegistrar;

/// What a run returns besides its [`NormalizedOutput`]: the parts that cannot
/// be stored artifacts. Under Zero Data Retention no artifact is written, so
/// the extracted text is handed back inline instead.
#[derive(Debug, Clone, Default)]
pub struct InlineParts {
    /// The extracted text. `Some` when it was asked for, and by default when
    /// this run stores nothing (ZDR).
    pub markdown: Option<String>,
    /// The fetched document as text, when asked for. Not for a document
    /// (PDF, DOCX): its bytes are not text.
    pub html: Option<String>,
    pub raw_html: Option<String>,
    /// Fields cut at the inline cap (ZDR only: nothing stored to point at).
    pub truncated: Vec<String>,
    /// Fields left out for being over the cap (not ZDR: read the artifact).
    pub omitted: Vec<String>,
    pub warnings: Vec<quarry_core::scrape::ScrapeWarning>,
    /// WS13.4b: the screenshot inline. Under ZDR only.
    pub screenshot: Option<quarry_core::scrape::InlineCapture>,
    /// WS13.4b: the PDF inline. Under ZDR only.
    pub pdf: Option<quarry_core::scrape::InlineCapture>,
    /// WS13.4b: what the captures measured, in both modes.
    pub captures: Option<quarry_core::scrape::CapturesReport>,
    /// WS13.5: what each scrape-time action did, in request order (empty when
    /// the run had none). A capture an action took is referenced by artifact id
    /// (not ZDR) or carried as base64 under the inline cap (ZDR).
    pub actions: Vec<quarry_core::scrape::ActionOutcomeView>,
    /// INNH-1: the page text for the AI formats, at most
    /// [`InlineRequest::ai_source_max_chars`] chars (cut on a char boundary). A
    /// caller that must know the page was longer asks for one char more than it
    /// will use and cuts the rest itself.
    /// Filled whenever that is asked for, whether or not `markdown` was
    /// requested and however long the text is (the inline cap does not apply).
    /// Never serialized and never stored: the caller hands it to the AI
    /// engine and drops it.
    pub ai_source: Option<String>,
    /// INNH-7: the challenge classifier flagged the page as a bot wall (a CAPTCHA
    /// widget, a WAF interstitial). Set whatever `QUARRY_CHALLENGE_ENFORCE` says: in
    /// `log` mode (the default) the run still succeeds, and this tells a caller that
    /// the output is not content (the URL result cache never stores it). Never
    /// serialized.
    pub bot_wall_suspected: bool,
    /// INNH-7: `soft_404::detect` fired (a 2xx body that says "page not found").
    /// The run still succeeds; the URL result cache never stores such a page. Never
    /// serialized.
    pub soft_404: bool,
}

/// What [`PageRunner::persist_captures`] made of one run's captures. The refs
/// go on the output's `formats`. The inline parts and warnings join the
/// [`InlineParts`] the caller reads.
#[derive(Debug, Clone, Default)]
pub struct CaptureOutput {
    pub screenshot_ref: Option<FormatRef>,
    pub pdf_ref: Option<FormatRef>,
    pub inline_screenshot: Option<quarry_core::scrape::InlineCapture>,
    pub inline_pdf: Option<quarry_core::scrape::InlineCapture>,
    pub captures: Option<quarry_core::scrape::CapturesReport>,
    pub actions: Vec<quarry_core::scrape::ActionOutcomeView>,
    pub omitted: Vec<String>,
    pub warnings: Vec<quarry_core::scrape::ScrapeWarning>,
}

/// What a caller wants returned inline, and how much.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineRequest {
    pub markdown: bool,
    pub html: bool,
    pub raw_html: bool,
    pub max_bytes: usize,
    /// How the page becomes markdown. The default is the standard
    /// main-content extraction, byte-identical to what a run always did.
    pub clean: quarry_transform::clean_html::CleanOptions,
    /// INNH-1: when set, [`InlineParts::ai_source`] carries the page text cut to
    /// this many chars. `None` (the default) leaves it out.
    pub ai_source_max_chars: Option<usize>,
}

impl InlineRequest {
    /// What a run returned before request options existed: the text under ZDR,
    /// nothing otherwise, capped at the default.
    pub fn default_for(zdr: ZdrMode) -> Self {
        Self {
            markdown: zdr.is_active(),
            html: false,
            raw_html: false,
            max_bytes: quarry_core::scrape::DEFAULT_INLINE_MAX_BYTES,
            clean: quarry_transform::clean_html::CleanOptions::default(),
            ai_source_max_chars: None,
        }
    }
}

/// Put `text` inline if it fits the cap. Over the cap, ZDR cuts it on a
/// character boundary (and says so); otherwise the field is left out and named,
/// because a stored artifact holds it.
pub fn offer_inline(
    field: &str,
    text: &str,
    max_bytes: usize,
    zdr: bool,
    slot: &mut Option<String>,
    truncated: &mut Vec<String>,
    omitted: &mut Vec<String>,
) {
    if text.len() <= max_bytes {
        *slot = Some(text.to_owned());
    } else if zdr {
        let (cut, _) = quarry_core::scrape::truncate_on_char_boundary(text, max_bytes);
        *slot = Some(cut);
        truncated.push(field.to_owned());
    } else {
        omitted.push(field.to_owned());
    }
}

/// What the scrape pipeline does with the kind of body it fetched
/// (`quarry_transform::sniff`).
///
/// - `Off`: no detection; every body takes the HTML path, as before.
/// - `Shadow`: detect and record (`metadata.content_kind`, the `page_fetched`
///   event) but change nothing else.
/// - `Skip`: also refuse bodies Quarry has no extractor for (images, archives,
///   spreadsheets, ...) with a typed `UNSUPPORTED_MEDIA` (415) instead of
///   running readability over binary data.
/// - `On`: also route PDF and DOCX to their text extractors, and Office files
///   to imports-core when `QUARRY_EDGE__OFFICE_VIA_IMPORTS` is on (E3.6-6b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocRouting {
    Off,
    #[default]
    Shadow,
    Skip,
    On,
}

impl DocRouting {
    pub const ENV: &'static str = "QUARRY_EDGE__DOC_ROUTING";

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" | "0" | "false" => Ok(Self::Off),
            "" | "shadow" => Ok(Self::Shadow),
            "skip" => Ok(Self::Skip),
            "on" | "1" | "true" => Ok(Self::On),
            other => Err(format!(
                "{}={other:?} is not one of off, shadow, skip, on",
                Self::ENV
            )),
        }
    }

    /// Read from `QUARRY_EDGE__DOC_ROUTING`; unset is `Shadow`, an unknown
    /// value is an error naming the variable.
    pub fn from_env() -> Result<Self, String> {
        match std::env::var(Self::ENV) {
            Ok(raw) => Self::parse(&raw),
            Err(_) => Ok(Self::default()),
        }
    }

    /// The process-wide mode, read once. Boot validates the environment first,
    /// so the fallback only covers a value that was valid at start-up.
    pub fn cached() -> Self {
        static MODE: std::sync::OnceLock<DocRouting> = std::sync::OnceLock::new();
        *MODE.get_or_init(|| Self::from_env().unwrap_or_default())
    }

    fn detects(self) -> bool {
        self != Self::Off
    }

    fn rejects_unsupported(self) -> bool {
        matches!(self, Self::Skip | Self::On)
    }
}

/// E3.6-6b: the environment variable that sends Office documents (XLSX, PPTX,
/// OpenDocument and EPUB) to the imports-core extract client (E3.6-6a). Off by
/// default; it takes effect only under `QUARRY_EDGE__DOC_ROUTING=on`.
pub const OFFICE_VIA_IMPORTS_ENV: &str = "QUARRY_EDGE__OFFICE_VIA_IMPORTS";

/// E3.6-6b: whether Office documents take the imports-core extract client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OfficeViaImports {
    /// Office documents keep their existing path: a typed 415 where routing
    /// refuses unsupported bodies, the HTML path otherwise.
    #[default]
    Off,
    /// XLSX, PPTX, OpenDocument and EPUB go to the extract client, in the
    /// document lane (`DocRouting::On`).
    On,
}

impl OfficeViaImports {
    /// `off` / `false` / `0` / empty, or `on` / `true` / `1`, any case.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "off" | "false" | "0" => Ok(Self::Off),
            "on" | "true" | "1" => Ok(Self::On),
            other => Err(format!(
                "{OFFICE_VIA_IMPORTS_ENV}: unknown value {other:?} (expected off or on)"
            )),
        }
    }

    /// Read once per process. Unset is `Off`. An unreadable value is `Off` as
    /// well (and logged): this route sends document bytes to another service,
    /// so a bad value must not switch it on.
    pub fn cached() -> Self {
        static MODE: std::sync::OnceLock<OfficeViaImports> = std::sync::OnceLock::new();
        *MODE.get_or_init(|| match std::env::var(OFFICE_VIA_IMPORTS_ENV) {
            Err(_) => Self::Off,
            Ok(raw) => Self::parse(&raw).unwrap_or_else(|message| {
                tracing::warn!(%message, "unreadable Office route flag; the route stays off");
                Self::Off
            }),
        })
    }
}

/// E3.6-6b: the Office kinds the imports-core extract client serves. PDF and
/// DOCX keep their Rust extractors.
fn is_imports_office_kind(kind: quarry_transform::sniff::ContentKind) -> bool {
    use quarry_transform::sniff::ContentKind;
    matches!(
        kind,
        ContentKind::Xlsx | ContentKind::Pptx | ContentKind::Odf | ContentKind::Epub
    )
}

/// E3.6-6b: whether a body takes the imports-core extract client. It needs the
/// document lane (`DocRouting::On`, as PDF and DOCX do), the flag on, and an
/// Office kind. Every other body keeps its existing path.
fn takes_imports_route(
    doc_routing: DocRouting,
    flag: OfficeViaImports,
    kind: quarry_transform::sniff::ContentKind,
) -> bool {
    doc_routing == DocRouting::On && flag == OfficeViaImports::On && is_imports_office_kind(kind)
}

/// The Office flag a run reads. A test installs its own for the thread it runs
/// on, as for the PII stage.
fn office_via_imports_mode() -> OfficeViaImports {
    #[cfg(test)]
    if let Some(mode) = TEST_OFFICE_VIA_IMPORTS.with(|slot| *slot.borrow()) {
        return mode;
    }
    OfficeViaImports::cached()
}

#[cfg(test)]
thread_local! {
    // Per-thread test override for the flag.
    static TEST_OFFICE_VIA_IMPORTS: std::cell::RefCell<Option<OfficeViaImports>> =
        const { std::cell::RefCell::new(None) };
}

/// The reply a test gives the extract client on its thread: what the client
/// would have returned.
#[cfg(test)]
type TestOfficeReply = QuarryResult<crate::content_route::ParsedDocument>;

#[cfg(test)]
thread_local! {
    // Per-thread test override: a canned reply for the next extract call, which
    // then does not call the client.
    static TEST_OFFICE_REPLY: std::cell::RefCell<Option<TestOfficeReply>> =
        const { std::cell::RefCell::new(None) };
}

/// E3.6-6b: the one call into the E3.6-6a client (imports-core, ephemeral). The
/// bearer is minted for the run's org; the URL path only picks the ODF subtype.
async fn extract_office(
    org_id: &str,
    kind: quarry_transform::sniff::ContentKind,
    body: Vec<u8>,
    filename_hint: Option<&str>,
) -> QuarryResult<crate::content_route::ParsedDocument> {
    #[cfg(test)]
    if let Some(reply) = TEST_OFFICE_REPLY.with(|slot| slot.borrow_mut().take()) {
        return reply;
    }
    crate::content_route::extract_office_via_imports(org_id, kind, body, filename_hint).await
}

/// E3.6-4a: the environment variable that turns the fail-closed PII redaction
/// stage on (`off`, the default, or `on`).
pub const PII_REDACTION_ENV: &str = "QUARRY_EDGE__PII_REDACTION";
/// E3.6-4a: the org default. A comma list of org ids whose runs redact without
/// asking, or `*` for every org. It matters only while the stage is on.
pub const PII_REDACTION_DEFAULT_ORGS_ENV: &str = "QUARRY_EDGE__PII_REDACTION_DEFAULT_ORGS";

/// E3.6-4a: whether the pipeline's redaction stage can run at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PiiRedactionMode {
    /// The stage never runs: every run is byte-identical to before.
    #[default]
    Off,
    /// The stage runs for a run that asks (`redact_pii`) and for a run whose
    /// org default is on.
    On,
}

impl PiiRedactionMode {
    /// `off` / `false` / `0` / empty, or `on` / `true` / `1`, any case.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "off" | "false" | "0" => Ok(Self::Off),
            "on" | "true" | "1" => Ok(Self::On),
            other => Err(format!(
                "{PII_REDACTION_ENV}: unknown value {other:?} (expected off or on)"
            )),
        }
    }
}

/// E3.6-4a: which orgs redact by default, without asking per request. Quarry
/// has no per-org setting store yet, so the deployment names them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PiiOrgDefault {
    /// No org redacts unless the run asks.
    #[default]
    None,
    /// Every org redacts.
    All,
    /// These orgs redact.
    Orgs(Vec<String>),
}

impl PiiOrgDefault {
    /// A comma list of org ids; `*` anywhere in it means every org.
    pub fn parse(raw: &str) -> Self {
        let orgs: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|org| !org.is_empty())
            .map(str::to_owned)
            .collect();
        if orgs.iter().any(|org| org == "*") {
            Self::All
        } else if orgs.is_empty() {
            Self::None
        } else {
            Self::Orgs(orgs)
        }
    }

    fn covers(&self, org_id: Option<&str>) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Orgs(orgs) => org_id
                .filter(|org| !org.is_empty())
                .is_some_and(|org| orgs.iter().any(|listed| listed == org)),
        }
    }
}

/// E3.6-4a: the redaction stage's settings, from [`PII_REDACTION_ENV`] and
/// [`PII_REDACTION_DEFAULT_ORGS_ENV`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PiiRedactionSettings {
    pub mode: PiiRedactionMode,
    pub org_default: PiiOrgDefault,
}

impl PiiRedactionSettings {
    /// An unset mode is `Off`. A mode that cannot be read turns the stage on
    /// (fail closed: redaction only ever removes text).
    pub fn parse(mode: Option<&str>, default_orgs: Option<&str>) -> Self {
        let mode = match mode.map(PiiRedactionMode::parse) {
            None => PiiRedactionMode::Off,
            Some(Ok(mode)) => mode,
            Some(Err(message)) => {
                tracing::warn!(%message, "unreadable PII redaction mode; the stage is on (fail closed)");
                PiiRedactionMode::On
            }
        };
        Self {
            mode,
            org_default: default_orgs.map(PiiOrgDefault::parse).unwrap_or_default(),
        }
    }

    pub fn from_env() -> Self {
        Self::parse(
            std::env::var(PII_REDACTION_ENV).ok().as_deref(),
            std::env::var(PII_REDACTION_DEFAULT_ORGS_ENV).ok().as_deref(),
        )
    }

    /// Read once per process, like [`DocRouting::cached`].
    pub fn cached() -> &'static Self {
        static SETTINGS: std::sync::OnceLock<PiiRedactionSettings> = std::sync::OnceLock::new();
        SETTINGS.get_or_init(Self::from_env)
    }

    /// Whether a run redacts: the stage is on, and the run asked or its org's
    /// default is on.
    pub fn applies(&self, requested: bool, org_id: Option<&str>) -> bool {
        self.mode == PiiRedactionMode::On && (requested || self.org_default.covers(org_id))
    }
}

/// The settings a run reads. A test installs its own for the thread it runs on
/// instead of touching the process environment (as for the challenge config).
pub(crate) fn pii_redaction_settings() -> PiiRedactionSettings {
    #[cfg(test)]
    if let Some(settings) = TEST_PII_REDACTION.with(|slot| slot.borrow().clone()) {
        return settings;
    }
    PiiRedactionSettings::cached().clone()
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_PII_REDACTION: std::cell::RefCell<Option<PiiRedactionSettings>> =
        const { std::cell::RefCell::new(None) };
}

/// The `details.reason` of a page the redaction stage refused.
pub const PII_REDACTION_FAILED: &str = "pii_redaction_failed";

/// The most text the stage scans in one piece: the in-memory parse cap, so a
/// page the pipeline accepts is not refused for size alone, while a text past
/// it fails closed rather than going unscanned.
const PII_REDACT_MAX_BYTES: usize = 16 * 1024 * 1024;

/// How many scan-and-replace passes a text gets before a finding that is still
/// there fails the page. One pass covers every case the scanner knows; the
/// later ones catch a candidate an overlap rule dropped on the first.
const PII_REDACT_MAX_PASSES: usize = 3;

/// Why the stage refused a page. Each is a typed `details.cause`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PiiRedactionFailure {
    /// The text is over [`PII_REDACT_MAX_BYTES`].
    TooLarge,
    /// The scanner panicked.
    ScannerPanicked,
    /// Findings were still there after [`PII_REDACT_MAX_PASSES`] passes.
    ResidualPii,
    /// A structured field (metadata, links, branding) did not survive the
    /// round trip through its JSON form.
    ReencodeFailed,
    /// The run carries a capture (screenshot, PDF, action output) that text
    /// redaction cannot reach.
    CaptureNotRedactable,
}

impl PiiRedactionFailure {
    const fn label(self) -> &'static str {
        match self {
            Self::TooLarge => "too_large",
            Self::ScannerPanicked => "scanner_panicked",
            Self::ResidualPii => "residual_pii",
            Self::ReencodeFailed => "reencode_failed",
            Self::CaptureNotRedactable => "capture_not_redactable",
        }
    }
}

/// The typed per-page failure. Not retryable (the same page fails the same
/// way), and nothing of the page in it: the field name and the cause only.
pub(crate) fn pii_redaction_error(failure: PiiRedactionFailure, field: &'static str) -> QuarryError {
    QuarryError::new(
        ErrorCode::Forbidden,
        "the page could not be redacted for personal data, so nothing of it was kept or returned",
    )
    .with_details(json!({
        "reason": PII_REDACTION_FAILED,
        "cause": failure.label(),
        "field": field,
    }))
}

/// An error the redaction stage raised. Its `PageFailed` names the reason.
fn is_pii_redaction_failure(e: &QuarryError) -> bool {
    e.details
        .as_ref()
        .and_then(|d| d.get("reason"))
        .and_then(|r| r.as_str())
        == Some(PII_REDACTION_FAILED)
}

/// `text` with every finding replaced, or an error. Never a partial result:
/// the text comes back only when a scan of it finds nothing.
fn redact_text_fail_closed(text: &str) -> Result<String, PiiRedactionFailure> {
    if text.len() > PII_REDACT_MAX_BYTES {
        return Err(PiiRedactionFailure::TooLarge);
    }
    let scan_is_clean = |candidate: &str| {
        std::panic::catch_unwind(|| quarry_transform::pii::scan(candidate).is_empty())
            .map_err(|_| PiiRedactionFailure::ScannerPanicked)
    };
    let mut current = text.to_owned();
    for _ in 0..PII_REDACT_MAX_PASSES {
        if scan_is_clean(&current)? {
            return Ok(current);
        }
        let redacted = std::panic::catch_unwind(|| quarry_transform::pii::redact(&current))
            .map_err(|_| PiiRedactionFailure::ScannerPanicked)?;
        current = redacted;
    }
    if scan_is_clean(&current)? {
        Ok(current)
    } else {
        Err(PiiRedactionFailure::ResidualPii)
    }
}

/// A structured value redacted through its JSON text, so every string in it
/// (titles, descriptions, link text, URLs) is scanned. A marker that broke the
/// JSON fails the page rather than dropping the field.
fn redact_serde<T>(value: &T) -> Result<T, PiiRedactionFailure>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let text = serde_json::to_string(value).map_err(|_| PiiRedactionFailure::ReencodeFailed)?;
    let redacted = redact_text_fail_closed(&text)?;
    serde_json::from_str(&redacted).map_err(|_| PiiRedactionFailure::ReencodeFailed)
}

/// The capture a redacting run cannot keep, if it carries one. An image or a
/// PDF cannot be scanned, and an action's output may be either.
fn unredactable_capture(captures: Option<&crate::capture::PageCaptures>) -> Option<&'static str> {
    let captures = captures?;
    if captures.screenshot.is_some() {
        Some("screenshot")
    } else if captures.pdf.is_some() {
        Some("pdf")
    } else if !captures.actions.is_empty() || captures.action_outputs.iter().any(Option::is_some) {
        Some("actions")
    } else {
        None
    }
}

/// The page's text as the pipeline holds it before anything is kept.
struct PageText {
    markdown: String,
    body_text: String,
    cleaned_html: Option<String>,
    links: Vec<quarry_core::output::Link>,
    metadata: quarry_core::output::PageMetadata,
}

/// E3.6-4a: every text representation of the page redacted, or the typed
/// failure. All or nothing: one field that fails fails the page.
fn redact_page_text(page: PageText) -> QuarryResult<PageText> {
    let fail = |field: &'static str| move |failure: PiiRedactionFailure| pii_redaction_error(failure, field);
    let cleaned_html = match page.cleaned_html.as_deref() {
        Some(html) => Some(redact_text_fail_closed(html).map_err(fail("html"))?),
        None => None,
    };
    Ok(PageText {
        markdown: redact_text_fail_closed(&page.markdown).map_err(fail("markdown"))?,
        body_text: redact_text_fail_closed(&page.body_text).map_err(fail("html"))?,
        cleaned_html,
        links: redact_serde(&page.links).map_err(fail("links"))?,
        metadata: redact_serde(&page.metadata).map_err(fail("metadata"))?,
    })
}

pub struct PageRunner {
    pub driver: Arc<dyn Driver>,
    pub security: Arc<dyn SecurityEngine>,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub event_sink: EventSink,
    pub zdr: ZdrMode,
    /// Transport-agnostic Data Plane ingest. P2 / cluster #grpc. In
    /// production this points to either `IngestClient` (HTTP/JSON) or
    /// `GrpcIngestAdapter` (HTTP/2/protobuf) depending on
    /// `QUARRY_EDGE__DATA_PLANE_TRANSPORT`. `None` skips ingest
    /// entirely (used in test harnesses and dev mode).
    pub ingest: Option<Arc<dyn DataPlaneIngest>>,
    pub org_id: Option<String>,
    /// Initiating user id (verified Edge JWT `sub`, threaded through the
    /// orchestrator for crawl/batch or taken from `claims` for scrape). When
    /// set, every durable ingest from this run is forwarded with `x-user-id`
    /// so the Data Plane stamps the doc owner = this user, visibility = private
    /// (private-by-default). `None` = system ingest → org-visible (legacy).
    pub user_id: Option<String>,
    pub privacy: PrivacyPolicy,
    /// Optional cancellation token. When set, the spawned ingest task
    /// races against this token and aborts cleanly when the parent run
    /// is cancelled — without this, in-flight ingests continue running
    /// after their owning run is gone, wasting cycles and producing
    /// orphan documents in the Data Plane.
    pub cancel_token: Option<tokio_util::sync::CancellationToken>,
    /// Cycle 19 / cluster #16: when set, every successful scrape is
    /// indexed into the local Tantivy corpus so subsequent `/v1/search`
    /// hits return instantly without going to live SERPs.
    /// Indexing is best-effort + fire-and-forget — index failures never
    /// fail the scrape. Skipped entirely when `zdr=on` (the local index
    /// is a durable artifact; ZDR forbids durable writes), and skipped for
    /// this run when [`Self::org_id`] is unset: see [`LocalIndexWriteback`]
    /// for why an untenanted document cannot be stored at all.
    pub local_index: Option<TantivyLocalIndex>,
    /// Cycle 21 / cluster #2 — RunPolicy bag (Determinism + sub-policies).
    /// Defaults to `best_effort`. Stamped into output meta so consumers
    /// can audit which policy produced the artifact.
    pub policy: RunPolicy,
    /// Cycle 21 / cluster #3 — per-host adaptive scheduler. `None`
    /// disables throttling (used in test harnesses); production wires
    /// a single shared scheduler so all in-flight scrapes coordinate.
    pub scheduler: Option<Arc<HostScheduler>>,
    /// 2C / cluster #autoscale — process-wide concurrency governor layered
    /// above the per-host scheduler. Acquired before each fetch (global cap),
    /// AIMD-adjusted by fetch outcome. `None` disables it (tests/dev);
    /// production wires `global_autoscale()`.
    pub autoscale: Option<Arc<AutoscaledPool>>,
    /// Wave 6 — render hints from the public API (`ScrapeRequest.render`).
    /// Only the browser driver honours these; static / TLS-profile
    /// drivers ignore them. Default empty.
    pub render: RenderHints,
    /// Phase-2 visual RAG — optional page-image producer. When `Some`, each
    /// successfully-ingested web page is rendered to a PNG (in its own browser
    /// session), stored in the CAS, and a `page_images.created` event is emitted
    /// for the embedding-engine. `None` (default) disables the visual producer.
    pub page_renderer: Option<Arc<crate::page_renderer::PageRenderer>>,
    /// P0 fix — Aquatiq crawl-to-KB pipeline (2026-07-20): best-effort
    /// registrar that materializes a durable "tracked website" `Source` row
    /// on first successful ingest for a given host, so the Knowledge Base's
    /// "Tracked web sources" panel reflects crawl-originated content instead
    /// of staying empty forever (previously only the Ingestions page's
    /// manual registration flow ever wrote that row). `None` disables
    /// registration (ad-hoc single-page scrapes, test harnesses).
    pub source_registrar: Option<Arc<dyn SourceRegistrar>>,
    /// Best-effort Model Plane title enricher for the `page_extracted`
    /// event. `None` disables the model hop (the extraction then carries
    /// `title_source: html | host` only). Edge wires it from
    /// `QUARRY_EDGE__MODEL_PLANE_URL` + `QUARRY_EDGE__PAGE_TITLE_MODEL`;
    /// test harnesses and ad-hoc runners leave it `None`.
    pub title_enricher: Option<Arc<crate::page_extract::PageTitleEnricher>>,
    /// Content-kind routing mode; see [`DocRouting`]. Edge wires
    /// [`DocRouting::cached`].
    pub doc_routing: DocRouting,
    /// WS14.S4: the robots.txt gate and the mode this run applies it under. `None`
    /// (every ad-hoc scrape, every test harness, a crawl that ignores robots)
    /// checks nothing. `Observe` evaluates and counts without blocking or slowing
    /// a fetch; `Enforce` refuses a disallowed or unreachable URL with a 403
    /// `SECURITY_BLOCKED` `details.denial` and applies `Crawl-delay`.
    pub robots: Option<RobotsEnforcement>,
}

/// The challenge settings the scrape path runs under (`QUARRY_CHALLENGE_*`),
/// read per run like the agent path reads them per run. A test installs its own
/// for the thread it runs on instead of touching the process environment.
fn scrape_challenge_config() -> crate::challenge::ChallengeConfig {
    #[cfg(test)]
    if let Some(config) = TEST_CHALLENGE_CONFIG.with(|slot| slot.get()) {
        return config;
    }
    crate::challenge::ChallengeConfig::from_env()
}

#[cfg(test)]
thread_local! {
    static TEST_CHALLENGE_CONFIG: std::cell::Cell<Option<crate::challenge::ChallengeConfig>> =
        const { std::cell::Cell::new(None) };
}

/// Whether the body is something a bot wall can be: HTML, XML or text. An
/// image, a PDF or a JSON answer is not classified. No content type is read as
/// HTML, as everywhere else on this path.
fn is_markup_content_type(content_type: Option<&str>) -> bool {
    content_type.is_none_or(|ct| {
        let ct = ct.to_ascii_lowercase();
        ct.contains("html") || ct.contains("xml") || ct.starts_with("text/")
    })
}

#[cfg(test)]
fn page_event_persistence_allowed(zdr_mode: ZdrMode) -> bool {
    zdr::guard(zdr_mode, WriteKind::Event).is_ok()
}

/// A successfully fetched and extracted page, ready for the local corpus.
///
/// Deliberately not `NormalizedOutput`: the point of this type is that any
/// path which ends up holding a URL, a title and some markdown can write back
/// without first assembling a full pipeline output. The answer pipeline and
/// the `/v1/search` page-read path have exactly that much and no more.
pub struct IndexedPage {
    /// Final (post-redirect) URL. Doubles as the corpus primary key, so it
    /// must be the URL we actually read, not the one that was requested.
    pub url: String,
    pub title: Option<String>,
    pub markdown: String,
    pub fingerprint: String,
    pub fetched_at: chrono::DateTime<Utc>,
    /// Verified tenant. `None` means "not known here" — such a page is
    /// dropped, never stored under a placeholder. See [`WritebackDecision`].
    pub org_id: Option<String>,
}

/// Why a page was, or was not, written back to the local corpus.
///
/// Split out from the spawn so the rules are unit-testable without an index,
/// a runtime, or a fetch — and so a future caller adding a write-back site
/// cannot accidentally implement a *different* set of rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritebackDecision {
    Index,
    /// No local index configured (test harnesses, dev runs without a corpus).
    SkippedNoIndex,
    /// The local corpus is a durable artifact and ZDR forbids durable writes.
    SkippedZdr,
    /// No verified tenant for this page. Indexing it would either invent an
    /// empty-string tenant or require an unscoped read to retrieve it — both
    /// of which are cross-tenant leaks. Dropping the page is the only safe
    /// option; a thinner corpus is the correct trade.
    SkippedUnknownOrg,
    /// Nothing worth indexing: extraction produced no text.
    SkippedEmptyBody,
}

/// Fire-and-forget write-back into the local Tantivy corpus.
///
/// Every path that successfully fetches and extracts a page should submit it
/// here. Previously only the scrape pipeline did, which is why the local tier
/// stayed thin no matter how heavily `/v1/search` and `/v1/answer` were used:
/// those paths fetch and extract pages too, and threw the text away.
///
/// The write is spawned, never awaited, and never surfaces an error to the
/// caller — a corpus write must not add latency to, or fail, a user-facing
/// request. That also means it must not be given a bounded channel or any
/// other backpressure that could push back into the request path.
///
/// The scrape pipeline submits here. The answer pipeline and the `/v1/search`
/// page-read path do not yet: their fetch goes through
/// [`crate::answer::MarkdownFetcher`], whose `(url, zdr)` signature carries no
/// tenant, and a write-back with no tenant is exactly what this hook refuses.
/// Closing that gap means threading the verified `org_id` to those fetch sites;
/// the hook is ready for them as-is.
#[derive(Clone, Default)]
pub struct LocalIndexWriteback {
    index: Option<TantivyLocalIndex>,
    zdr: ZdrMode,
}

/// The URL to put in a log field: none while Zero Data Retention is active,
/// because a log line is a durable copy of a page the organization asked us
/// not to keep.
fn log_url(zdr: ZdrMode, url: &str) -> Option<&str> {
    if zdr.is_active() {
        None
    } else {
        Some(url)
    }
}

impl LocalIndexWriteback {
    pub fn new(index: Option<TantivyLocalIndex>, zdr: ZdrMode) -> Self {
        Self { index, zdr }
    }

    /// Whether a submission could reach an index at all. Callers can use this
    /// to skip assembling an [`IndexedPage`] they know will be dropped.
    pub fn is_enabled(&self) -> bool {
        self.index.is_some() && zdr::guard(self.zdr, WriteKind::Artifact).is_ok()
    }

    /// Classify a page without performing any write.
    pub fn decide(&self, page: &IndexedPage) -> WritebackDecision {
        if self.index.is_none() {
            return WritebackDecision::SkippedNoIndex;
        }
        if zdr::guard(self.zdr, WriteKind::Artifact).is_err() {
            return WritebackDecision::SkippedZdr;
        }
        match page.org_id.as_deref().map(str::trim) {
            Some(org) if !org.is_empty() => {}
            _ => return WritebackDecision::SkippedUnknownOrg,
        }
        if page.markdown.trim().is_empty() {
            return WritebackDecision::SkippedEmptyBody;
        }
        WritebackDecision::Index
    }

    /// Submit a page. Returns the decision so callers and tests can observe
    /// it synchronously; the actual index write happens on a detached task.
    pub fn submit(&self, page: IndexedPage) -> WritebackDecision {
        let decision = self.decide(&page);
        if decision != WritebackDecision::Index {
            // zdr-log-ok: log_url() is None while ZDR is active
            tracing::debug!(
                url = log_url(self.zdr, &page.url),
                decision = ?decision,
                "local index write-back skipped"
            );
            return decision;
        }
        // `decide` already established both of these, but re-deriving them by
        // pattern rather than by `expect` keeps a future edit to `decide` from
        // turning a skip into a panic on a user-facing request path.
        let Some(idx) = self.index.clone() else {
            return WritebackDecision::SkippedNoIndex;
        };
        let org_id = match page.org_id.as_deref().map(str::trim) {
            Some(org) if !org.is_empty() => org.to_string(),
            _ => return WritebackDecision::SkippedUnknownOrg,
        };
        let host = Url::parse(&page.url)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_default();
        let doc = LocalDocument {
            url: page.url,
            title: page.title.unwrap_or_default(),
            body: page.markdown,
            host,
            org_id,
            fingerprint: page.fingerprint,
            fetched_at: page.fetched_at,
        };
        tokio::spawn(async move {
            if let Err(e) = idx.add_document(doc).await {
                // zdr-log-ok: `decide` skips the write-back when the ZDR guard blocks an artifact write, so a ZDR request never reaches this detached task
                tracing::warn!(error = %e, "local index add_document failed (non-fatal)");
            }
        });
        decision
    }
}

impl PageRunner {
    /// The corpus write-back hook for this run, carrying this run's ZDR
    /// posture. Built per call rather than stored so it can never drift out
    /// of sync with `self.zdr`.
    fn local_index_writeback(&self) -> LocalIndexWriteback {
        LocalIndexWriteback::new(self.local_index.clone(), self.zdr)
    }

    /// WS14.S4: check `url` against robots.txt when this run has a gate. `stage` is
    /// `"request"` (before anything is fetched) or `"final_url"` (after a redirect
    /// to another origin). `Observe` counts and logs a count, and changes nothing.
    /// `Enforce` refuses a denial with `PageBlocked` and a typed error, and returns
    /// the host's `Crawl-delay` (zero when it sets none) for an allowed URL.
    /// `None` when there is no gate, the mode is `Off`, or the run only observes.
    async fn robots_check(
        &self,
        run_id: &RunKind,
        url: &Url,
        stage: &'static str,
    ) -> QuarryResult<Option<std::time::Duration>> {
        let Some(robots) = self
            .robots
            .as_ref()
            .filter(|r| r.mode != RobotsEnforceMode::Off)
        else {
            return Ok(None);
        };
        let decision = robots
            .gate
            .check(
                url,
                self.org_id.as_deref().unwrap_or_default(),
                self.zdr.is_active(),
            )
            .await;
        robots.gate.record(robots.mode, &decision);
        if !decision.is_denial() {
            return Ok(match (&decision, robots.mode) {
                (crate::robots_gate::RobotsDecision::Allowed { crawl_delay }, RobotsEnforceMode::Enforce) => {
                    Some(crawl_delay.unwrap_or_default())
                }
                _ => None,
            });
        }
        if robots.mode == RobotsEnforceMode::Observe {
            tracing::info!(stage, "robots.txt would deny this fetch (observe mode)");
            return Ok(None);
        }
        let denial = decision.denial_json().unwrap_or_else(|| json!({}));
        let code = denial["code"].as_str().unwrap_or("robots_disallowed").to_string();
        let message = denial["message"].as_str().unwrap_or("robots.txt refused").to_string();
        let mut payload = denial.clone();
        payload["url"] = json!(url.to_string());
        payload["reason"] = json!(code);
        payload["source"] = json!("fetch");
        payload["stage"] = json!(stage);
        self.emit_page_event(
            run_id.clone(),
            EventType::PageBlocked,
            payload,
            format!("{}:{}:robots_blocked", run_id, url),
        )
        .await;
        Err(QuarryError::new(ErrorCode::SecurityBlocked, message)
            .with_details(json!({ "denial": denial })))
    }

    /// WS14.S4, D2-161: the robots decision for `url` before a page that is about to be
    /// served from the page cache. The route reads the cache before any fetch, so without
    /// this a cached page was served and metered without the fetch-time check ever running.
    /// It is the same decision as `run` makes (one `record`, the same typed refusal and
    /// `page_blocked` event), so a page gets one decision whether it is cached or fetched:
    /// a run that fetches does not repeat it. `Ok` also covers observe and `Off`.
    pub async fn precheck_robots(&self, run_id: &RunKind, url: &Url) -> QuarryResult<()> {
        self.robots_check(run_id, url, "request").await.map(|_| ())
    }

    async fn emit_page_event(
        &self,
        run_id: RunKind,
        event_type: EventType,
        payload: serde_json::Value,
        idempotency_key: String,
    ) {
        self.event_sink
            .emit_for_zdr(self.zdr, run_id, event_type, payload, idempotency_key)
            .await;
    }

    pub async fn run(
        &self,
        run_id: &RunKind,
        requested_url: &Url,
        prev_fingerprint: Option<String>,
    ) -> QuarryResult<NormalizedOutput> {
        self.run_with_inline(run_id, requested_url, prev_fingerprint)
            .await
            .map(|(output, _inline)| output)
    }

    /// Like [`Self::run`], also returning the inline parts (the extracted text
    /// under ZDR, and warnings). `/v1/scrape` uses this; crawl and batch keep
    /// `run`.
    pub async fn run_with_inline(
        &self,
        run_id: &RunKind,
        requested_url: &Url,
        prev_fingerprint: Option<String>,
    ) -> QuarryResult<(NormalizedOutput, InlineParts)> {
        self.run_with_inline_request(
            run_id,
            requested_url,
            prev_fingerprint,
            &InlineRequest::default_for(self.zdr),
        )
        .await
    }

    /// [`Self::run_with_inline`] returning exactly the inline parts asked for.
    pub async fn run_with_inline_request(
        &self,
        run_id: &RunKind,
        requested_url: &Url,
        prev_fingerprint: Option<String>,
        inline_request: &InlineRequest,
    ) -> QuarryResult<(NormalizedOutput, InlineParts)> {
        self.run_with_inline_request_redacting(
            run_id,
            requested_url,
            prev_fingerprint,
            inline_request,
            false,
        )
        .await
    }

    /// E3.6-4a: [`Self::run_with_inline_request`] for a run that asked for PII
    /// redaction (`redact_pii: on`). The stage runs only while
    /// `QUARRY_EDGE__PII_REDACTION` is on; then a run also redacts without
    /// asking when its org's default is on. A redacting run keeps, emits and
    /// returns only redacted text; a page that cannot be redacted fails with a
    /// typed `pii_redaction_failed` and leaves nothing behind.
    pub async fn run_with_inline_request_redacting(
        &self,
        run_id: &RunKind,
        requested_url: &Url,
        prev_fingerprint: Option<String>,
        inline_request: &InlineRequest,
        redact_pii: bool,
    ) -> QuarryResult<(NormalizedOutput, InlineParts)> {
        match self
            .run_inner(
                run_id,
                requested_url,
                prev_fingerprint,
                inline_request,
                redact_pii,
            )
            .await
        {
            Ok(parts) => Ok(parts),
            Err(e) if is_robots_denial(&e) => Err(e),
            Err(e) => {
                let mut payload = json!({
                    "url": requested_url.to_string(),
                    "error": e.to_string(),
                });
                // E3.6-4a: the typed per-page reason a crawl or batch counts
                // under `pages_failed`. The cause and field name only.
                if is_pii_redaction_failure(&e) {
                    if let Some(details) = e.details.as_ref() {
                        payload["reason"] = json!(PII_REDACTION_FAILED);
                        payload["cause"] = details["cause"].clone();
                        payload["field"] = details["field"].clone();
                    }
                }
                self.emit_page_event(
                    run_id.clone(),
                    EventType::PageFailed,
                    payload,
                    format!("{}:{}:failed", run_id, requested_url),
                )
                .await;
                Err(e)
            }
        }
    }

    /// WS13.4b: store or inline what the browser captured for this run.
    /// Not ZDR: each capture is an artifact under a label of its own
    /// (`final.screenshot`, `final.pdf`), with one content-free
    /// `ArtifactWritten` per capture. ZDR: nothing is written or emitted, and
    /// the bytes travel inline under the cap. A capture over the cap is omitted
    /// with `capture_exceeds_inline_cap`.
    async fn persist_captures(
        &self,
        run_id: &RunKind,
        resp: &crate::fetch::FetchResponse,
        fingerprint: &str,
    ) -> QuarryResult<CaptureOutput> {
        use base64::Engine as _;
        let mut out = CaptureOutput::default();
        let Some(caps) = resp.captures.as_deref() else {
            return Ok(out);
        };
        out.warnings = caps
            .warnings
            .iter()
            .map(|w| quarry_core::scrape::ScrapeWarning::new(&w.code, w.message.clone()))
            .collect();
        let mut report = quarry_core::scrape::CapturesReport {
            blocked_requests: caps.blocked_requests,
            ..Default::default()
        };
        let store = zdr::guard(self.zdr, WriteKind::Artifact).is_ok();
        let org = self.org_id.clone().unwrap_or_default();
        let inline_cap = self
            .render
            .capture_inline_max_bytes
            .unwrap_or(crate::capture::DEFAULT_INLINE_CAPTURE_MAX_BYTES)
            .min(crate::capture::MAX_INLINE_CAPTURE_BYTES);
        let mut inline_used = 0usize;

        if let Some(image) = caps.screenshot.as_ref() {
            let (kind, content_type) = match image.format {
                quarry_core::capture::ImageFormat::Png => ("screenshot", "image/png"),
                quarry_core::capture::ImageFormat::Jpeg => ("screenshot_jpeg", "image/jpeg"),
            };
            report.screenshot = Some(quarry_core::scrape::ScreenshotReport {
                content_type: content_type.to_owned(),
                width: image.width_px,
                height: image.height_px,
                bytes: image.bytes.len() as u64,
                tiles: image.tiles,
                stitched: image.stitched,
                truncated: image.truncated,
            });
            if store {
                let label = quarry_core::artifact::page_hash(
                    resp.final_url.as_ref(),
                    &format!("{fingerprint}#cap:final.{kind}"),
                );
                let handle = self
                    .artifacts
                    .put(&org, run_id, &label, kind, image.bytes.clone())
                    .await?;
                self.event_sink
                    .emit(
                        run_id.clone(),
                        EventType::ArtifactWritten,
                        json!({
                            "url": resp.final_url.to_string(),
                            "format": "screenshot",
                            "artifact_id": handle.artifact_id.to_string(),
                            "bytes": handle.bytes,
                        }),
                        format!("{}:{}:artifact:screenshot", run_id, resp.final_url),
                    )
                    .await;
                out.screenshot_ref = Some(FormatRef {
                    artifact_id: handle.artifact_id,
                    bytes: handle.bytes,
                });
            } else if image.bytes.len() <= inline_cap.saturating_sub(inline_used) {
                inline_used += image.bytes.len();
                out.inline_screenshot = Some(quarry_core::scrape::InlineCapture {
                    content_type: content_type.to_owned(),
                    bytes: image.bytes.len() as u64,
                    width: Some(image.width_px),
                    height: Some(image.height_px),
                    data_base64: base64::engine::general_purpose::STANDARD.encode(&image.bytes),
                });
            } else {
                out.omitted.push("screenshot".to_owned());
                out.warnings.push(quarry_core::scrape::ScrapeWarning::new(
                    "capture_exceeds_inline_cap",
                    "the screenshot is larger than the inline cap and is left out",
                ));
            }
        }

        if let Some(pdf) = caps.pdf.as_ref() {
            report.pdf = Some(quarry_core::scrape::PdfReport {
                bytes: pdf.bytes.len() as u64,
            });
            if store {
                let label = quarry_core::artifact::page_hash(
                    resp.final_url.as_ref(),
                    &format!("{fingerprint}#cap:final.pdf"),
                );
                let handle = self
                    .artifacts
                    .put(&org, run_id, &label, "pdf", pdf.bytes.clone())
                    .await?;
                self.event_sink
                    .emit(
                        run_id.clone(),
                        EventType::ArtifactWritten,
                        json!({
                            "url": resp.final_url.to_string(),
                            "format": "pdf",
                            "artifact_id": handle.artifact_id.to_string(),
                            "bytes": handle.bytes,
                        }),
                        format!("{}:{}:artifact:pdf", run_id, resp.final_url),
                    )
                    .await;
                out.pdf_ref = Some(FormatRef {
                    artifact_id: handle.artifact_id,
                    bytes: handle.bytes,
                });
            } else if pdf.bytes.len() <= inline_cap.saturating_sub(inline_used) {
                out.inline_pdf = Some(quarry_core::scrape::InlineCapture {
                    content_type: "application/pdf".to_owned(),
                    bytes: pdf.bytes.len() as u64,
                    width: None,
                    height: None,
                    data_base64: base64::engine::general_purpose::STANDARD.encode(&pdf.bytes),
                });
            } else {
                out.omitted.push("pdf".to_owned());
                out.warnings.push(quarry_core::scrape::ScrapeWarning::new(
                    "capture_exceeds_inline_cap",
                    "the PDF is larger than the inline cap and is left out",
                ));
            }
        }

        // WS13.5: what the scrape-time actions took. Each output goes through the same
        // path as a final capture: stored under a label of its own (not ZDR), or inline
        // under the cap that the final captures also draw on (ZDR). The outcome list is
        // index-aligned with the output bytes.
        for (position, outcome) in caps.actions.iter().enumerate() {
            let mut view = quarry_core::scrape::ActionOutcomeView::from_outcome(outcome);
            let bytes = caps
                .action_outputs
                .get(position)
                .and_then(Option::as_ref)
                .filter(|held| !held.bytes.is_empty());
            if let (Some(output), Some(held)) = (view.output.as_mut(), bytes) {
                if store {
                    let kind = if output.kind == "pdf" { "pdf" } else { "screenshot" };
                    let label = quarry_core::artifact::page_hash(
                        resp.final_url.as_ref(),
                        &format!("{fingerprint}#cap:action.{}.{}", outcome.index, output.kind),
                    );
                    let handle = self
                        .artifacts
                        .put(&org, run_id, &label, kind, held.bytes.clone())
                        .await?;
                    self.event_sink
                        .emit(
                            run_id.clone(),
                            EventType::ArtifactWritten,
                            json!({
                                "url": resp.final_url.to_string(),
                                "format": format!("action_{}", output.kind),
                                "action_index": outcome.index,
                                "artifact_id": handle.artifact_id.to_string(),
                                "bytes": handle.bytes,
                            }),
                            format!(
                                "{}:{}:artifact:action.{}",
                                run_id, resp.final_url, outcome.index
                            ),
                        )
                        .await;
                    output.bytes = handle.bytes;
                    output.artifact_id = Some(handle.artifact_id.to_string());
                } else if held.bytes.len() <= inline_cap.saturating_sub(inline_used) {
                    inline_used += held.bytes.len();
                    output.bytes = held.bytes.len() as u64;
                    output.data_base64 =
                        Some(base64::engine::general_purpose::STANDARD.encode(&held.bytes));
                } else {
                    out.omitted.push(format!("actions[{}].output", outcome.index));
                    out.warnings.push(quarry_core::scrape::ScrapeWarning::new(
                        "capture_exceeds_inline_cap",
                        format!(
                            "the output of actions[{}] is larger than the inline cap and is left out",
                            outcome.index
                        ),
                    ));
                }
            }
            out.actions.push(view);
        }

        // The report is the final captures' measurements; a run that only ran actions
        // and saw nothing blocked has none to give.
        if self.render.capture.is_some() || report != quarry_core::scrape::CapturesReport::default() {
            out.captures = Some(report);
        }
        Ok(out)
    }

    async fn run_inner(
        &self,
        run_id: &RunKind,
        requested_url: &Url,
        prev_fingerprint: Option<String>,
        inline_request: &InlineRequest,
        redact_requested: bool,
    ) -> QuarryResult<(NormalizedOutput, InlineParts)> {
        // Preflight
        let verdict = self.security.preflight(requested_url).await;
        if verdict.decision == quarry_security::Decision::Block {
            self.emit_page_event(
                run_id.clone(),
                EventType::PageBlocked,
                json!({
                    "url": requested_url.to_string(),
                    "reasons": verdict.reasons.clone(),
                }),
                format!("{}:{}:blocked", run_id, requested_url),
            )
            .await;
            return Err(quarry_core::error::QuarryError::new(
                quarry_core::error::ErrorCode::SecurityBlocked,
                verdict.reasons.join("; "),
            ));
        }

        // WS14.S4: robots.txt, after the preflight and before DNS resolution and the
        // scheduler slot, so a denial never reaches `record_bad` (which reads an
        // unknown error as a server error and halves the host's concurrency).
        if let Some(delay) = self.robots_check(run_id, requested_url, "request").await? {
            if let Some(s) = self.scheduler.as_ref() {
                s.set_min_interval(requested_url.host_str().unwrap_or("unknown"), delay)
                    .await;
            }
        }

        // Resolve and inspect the target before we hand it to a driver. Static
        // fetch receives this exact address set through FetchHints and pins its
        // transport resolver to it, so the connection cannot perform a second
        // hostname lookup after this public-address check.
        let resolved_target = if !self.security.allow_private_hosts() {
            Some(crate::dns_guard::resolve_public_url(requested_url).await?)
        } else {
            None
        };

        // Fetch — gated by the per-host scheduler when configured.
        // The slot permit is held only for the duration of the fetch
        // and drops eagerly (`drop(_slot)` before any other work) so
        // we don't hold scarce per-host concurrency during DOM cleanup
        // / markdown conversion / artifact storage.
        let host_for_scheduler = requested_url.host_str().unwrap_or("unknown").to_string();
        // WS14.S4, D2-161: a Crawl-delay wait is spent before the process-wide permit is taken,
        // so a request that waits for its start time holds no global permit and other hosts keep
        // theirs. The per-host slot is still taken after the global permit.
        let start_ticket = match self.scheduler.as_ref() {
            Some(s) => {
                let ticket = s.reserve_start(&host_for_scheduler).await;
                ticket.wait().await;
                Some(ticket)
            }
            None => None,
        };
        // 2C — global concurrency permit, acquired before the per-host slot
        // and held for the fetch duration (drops with `_slot`).
        let _global_slot = match self.autoscale.as_ref() {
            Some(p) => Some(p.acquire().await),
            None => None,
        };
        let _slot = match (self.scheduler.as_ref(), start_ticket) {
            (Some(s), Some(ticket)) => Some(s.acquire_reserved(ticket).await),
            _ => None,
        };
        // Use `fetch_conditional` so render-hint plumbing (waitFor) and
        // future ETag/IMS validators flow through a single path. The
        // default trait impl on drivers that don't override it just
        // delegates to `fetch`, so static / TLS-profile drivers keep
        // their existing behaviour.
        let fetch_hints = FetchHints {
            render: self.render.clone(),
            org_id: self.org_id.clone().unwrap_or_default(),
            privacy: self.privacy.clone().with_zdr(self.zdr),
            resolved_target,
            ..FetchHints::default()
        };
        let fetch_result = self
            .driver
            .fetch_conditional(requested_url, &fetch_hints)
            .await;

        // Inform the scheduler before we propagate the error or move
        // on. mark_bad shrinks the host's target on rate-limit / block /
        // server / timeout; mark_good (after success) feeds the EWMA
        // and may grow the target.
        if let (Some(s), Err(err)) = (self.scheduler.as_ref(), fetch_result.as_ref()) {
            let kind = match err.code {
                quarry_core::error::ErrorCode::RateLimited => BadKind::RateLimited,
                quarry_core::error::ErrorCode::UpstreamBlocked
                | quarry_core::error::ErrorCode::Forbidden => BadKind::Blocked,
                quarry_core::error::ErrorCode::Timeout => BadKind::Timeout,
                _ => BadKind::Server,
            };
            s.record_bad(&host_for_scheduler, kind).await;
        }
        // 2C — global pool backs off on block/rate-limit/timeout-class errors.
        if let (Some(p), Err(err)) = (self.autoscale.as_ref(), fetch_result.as_ref()) {
            if matches!(
                err.code,
                quarry_core::error::ErrorCode::RateLimited
                    | quarry_core::error::ErrorCode::UpstreamBlocked
                    | quarry_core::error::ErrorCode::Forbidden
            ) {
                p.record_overload();
            }
        }
        let resp = fetch_result?;
        if let Some(s) = self.scheduler.as_ref() {
            // Use the driver-reported duration so the scheduler EWMA
            // reflects the upstream call latency, not Rust-side
            // post-processing time.
            s.record_good(
                &host_for_scheduler,
                std::time::Duration::from_millis(resp.duration_ms),
            )
            .await;
        }
        // 2C — global pool: grow on a clean fetch, back off on a block status
        // (FallbackDriver may surface a block resp after rotating its chain).
        if let Some(p) = self.autoscale.as_ref() {
            if crate::fingerprint_rotation::is_block_status(resp.status) {
                p.record_overload();
            } else {
                p.record_ok();
            }
        }
        drop(_slot);
        drop(_global_slot);

        // WS7.G: a redirect can land the fetch on a host the requested URL's preflight never
        // saw. When the engine asks for it, the final URL is preflighted too (host rules,
        // blocklists, URL reputation). A block ends the run here: nothing is parsed, stored,
        // cached or ingested. A final URL equal to the requested one is not asked twice.
        if self.security.recheck_final_url() && resp.final_url != *requested_url {
            let verdict = self.security.preflight(&resp.final_url).await;
            if verdict.decision == quarry_security::Decision::Block {
                self.emit_page_event(
                    run_id.clone(),
                    EventType::PageBlocked,
                    json!({
                        "url": resp.final_url.to_string(),
                        "reason": "redirect_target_blocked",
                        "reasons": verdict.reasons.clone(),
                    }),
                    format!("{}:{}:redirect_blocked", run_id, resp.final_url),
                )
                .await;
                return Err(quarry_core::error::QuarryError::new(
                    quarry_core::error::ErrorCode::SecurityBlocked,
                    verdict.reasons.join("; "),
                ));
            }
        }

        // WS14.S4: a redirect to another origin is checked against that origin's
        // robots.txt before anything is parsed, stored, cached or ingested.
        if resp.final_url != *requested_url
            && origin_key(&resp.final_url) != origin_key(requested_url)
        {
            self.robots_check(run_id, &resp.final_url, "final_url").await?;
        }

        // Defensive body-size cap. `scraper::Html::parse_document`
        // (used by readability, branding, metadata) builds a full
        // in-memory DOM — fine on normal pages, can blow up on a
        // 50 MB malicious HTML response. We cap before the parse
        // step. 16 MB is well above any real HTML page (Hacker News
        // front page is ~50 KB; the largest legitimate page we've
        // observed is ~5 MB on a JS-rendered single-page-app).
        const MAX_HTML_BYTES: usize = 16 * 1024 * 1024;
        if resp.body.len() > MAX_HTML_BYTES {
            self.emit_page_event(
                run_id.clone(),
                EventType::PageBlocked,
                json!({
                    "url": resp.final_url.to_string(),
                    "reason": "body_too_large",
                    "bytes": resp.body.len(),
                    "max_bytes": MAX_HTML_BYTES,
                }),
                format!("{}:{}:body_too_large", run_id, resp.final_url),
            )
            .await;
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                format!(
                    "body {} bytes exceeds max {} for in-memory parse",
                    resp.body.len(),
                    MAX_HTML_BYTES
                ),
            ));
        }
        let ct = resp
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.clone());
        // What the body is, decided by its first bytes before its header
        // (many document links are served as octet-stream or text/html). Only
        // successful responses are classified: an error page keeps the legacy
        // path whatever it contains.
        let detected = (self.doc_routing.detects() && (200..300).contains(&resp.status))
            .then(|| quarry_transform::sniff::detect(ct.as_deref(), &resp.body));
        // PageFetched payload includes `content_type` so downstream
        // consumers (e.g. the onboarding wizard) can pick the right
        // card style without re-parsing the HTML.
        self.emit_page_event(
            run_id.clone(),
            EventType::PageFetched,
            json!({
                "url": resp.final_url.to_string(),
                "status": resp.status,
                "duration_ms": resp.duration_ms,
                "content_type": ct.clone(),
                "content_kind": detected.map(|d| d.kind.label()),
                "content_kind_source": detected.map(|d| d.source.label()),
            }),
            format!("{}:{}:fetched", run_id, resp.final_url),
        )
        .await;

        // A body Quarry has no extractor for is refused with a typed error
        // rather than decoded as text: an image or an archive run through
        // readability yields mojibake that then gets ingested.
        if let Some(found) = detected {
            // E3.6-6b: an Office body the flag routes to imports-core is supported.
            let supported = found.kind.is_text_pipeline()
                || found.kind.is_parseable_document()
                || takes_imports_route(self.doc_routing, office_via_imports_mode(), found.kind);
            if self.doc_routing.rejects_unsupported() && !supported {
                let details = json!({
                    "reason": "unsupported_media",
                    "kind": found.kind.label(),
                    "subtype": found.subtype,
                    "content_type": ct,
                    "bytes": resp.body.len(),
                    "source": found.source.label(),
                    "supported": ["html", "text", "json", "xml", "pdf", "docx"],
                });
                self.emit_page_event(
                    run_id.clone(),
                    EventType::PageBlocked,
                    json!({
                        "url": resp.final_url.to_string(),
                        "reason": "unsupported_media",
                        "kind": found.kind.label(),
                        "content_type": ct,
                        "bytes": resp.body.len(),
                    }),
                    format!("{}:{}:unsupported_media", run_id, resp.final_url),
                )
                .await;
                return Err(QuarryError::new(
                    ErrorCode::UnsupportedMedia,
                    unsupported_media_message(found.kind.label()),
                )
                .with_details(details));
            }
        }

        // Transform
        //
        // Charset detection: HTTP `Content-Type: charset=…` is often a
        // lie on CMS-driven sites — declared utf-8 but served as
        // windows-1252, ISO-8859-1, etc. We sniff and re-decode here
        // so downstream readability + extraction see correct text.
        // Falls through to UTF-8 lossy when no signal is available
        // (preserves previous behaviour for well-behaved servers).
        // PDF and DOCX go to their text extractors (bounded, off-thread) when
        // routing is `on`; Office files go to the imports-core client when the
        // E3.6-6b flag is on; everything else takes the HTML path below. A file
        // the parser cannot read is a typed 415 with a reason, not mojibake.
        let document = match detected {
            Some(found)
                if self.doc_routing == DocRouting::On && found.kind.is_parseable_document() =>
            {
                match crate::content_route::parse_document(found.kind, resp.body.clone()).await {
                    Ok(parsed) => Some(parsed),
                    Err(error) => {
                        self.emit_page_event(
                            run_id.clone(),
                            EventType::PageBlocked,
                            json!({
                                "url": resp.final_url.to_string(),
                                "reason": "document_parse_failed",
                                "kind": found.kind.label(),
                                "code": format!("{:?}", error.code),
                                "bytes": resp.body.len(),
                            }),
                            format!("{}:{}:document_parse_failed", run_id, resp.final_url),
                        )
                        .await;
                        return Err(error);
                    }
                }
            }
            // E3.6-6b: an Office body the flag sends to imports-core. The extract
            // is ephemeral; its text takes the document path below, as PDF and
            // DOCX text does.
            Some(found)
                if takes_imports_route(self.doc_routing, office_via_imports_mode(), found.kind) =>
            {
                match extract_office(
                    self.org_id.as_deref().unwrap_or_default(),
                    found.kind,
                    resp.body.clone(),
                    Some(resp.final_url.path()),
                )
                .await
                {
                    Ok(parsed) => Some(parsed),
                    Err(error) => {
                        self.emit_page_event(
                            run_id.clone(),
                            EventType::PageBlocked,
                            json!({
                                "url": resp.final_url.to_string(),
                                "reason": "document_parse_failed",
                                "route": "imports",
                                "kind": found.kind.label(),
                                "code": format!("{:?}", error.code),
                                "bytes": resp.body.len(),
                            }),
                            format!(
                                "{}:{}:document_parse_failed:imports",
                                run_id, resp.final_url
                            ),
                        )
                        .await;
                        return Err(error);
                    }
                }
            }
            _ => None,
        };
        let is_doc = document.is_some();
        let declared_charset = ct
            .as_deref()
            .and_then(quarry_transform::charset::charset_from_content_type);
        // A document body is binary: do not charset-decode megabytes of it.
        let body_decoded = quarry_transform::charset::decode(
            if is_doc { &[] } else { &resp.body },
            declared_charset,
        );
        if !body_decoded.from_header && body_decoded.encoding != "UTF-8" {
            // zdr-log-ok: log_url() is None while ZDR is active
            tracing::debug!(
                url = log_url(self.zdr, resp.final_url.as_str()),
                detected = body_decoded.encoding,
                "charset sniff overrode declared header"
            );
        }
        let body_text = body_decoded.text;

        // Soft-404 check: a 200 body that says "page not found" still
        // shouldn't be ingested as content. Flag here and emit a
        // dedicated event so downstream consumers can drop it from
        // indexing without a content heuristic at every callsite.
        let mut soft_404_flagged = false;
        if let Some(reason) = (!is_doc)
            .then(|| quarry_transform::soft_404::detect(resp.status, resp.body.len(), &body_text))
            .flatten()
        {
            soft_404_flagged = true;
            self.emit_page_event(
                run_id.clone(),
                EventType::PageBlocked,
                json!({
                    "url": resp.final_url.to_string(),
                    "reason": "soft_404",
                    "matched": reason,
                }),
                format!("{}:{}:soft_404", run_id, resp.final_url),
            )
            .await;
        }

        // Cleaning options only apply to a page; a document's text is its own.
        let mut cleaned_html: Option<String> = None;
        let mut clean_warnings = Vec::new();
        let md = match &document {
            Some(parsed) => parsed.text.clone(),
            None if inline_request.clean.is_default() => html_to_readable_markdown(&body_text),
            None => {
                let cleaned = quarry_transform::clean_html::to_markdown(&body_text, &inline_request.clean);
                cleaned_html = Some(cleaned.html);
                clean_warnings = cleaned.warnings;
                cleaned.markdown
            }
        };
        let link_list = if is_doc {
            Vec::new()
        } else {
            links::extract(&body_text, &resp.final_url)
        };
        let mut meta = if is_doc {
            quarry_core::output::PageMetadata {
                content_type: ct.clone(),
                ..Default::default()
            }
        } else {
            metadata::extract(&body_text, ct.clone())
        };
        if let Some(found) = detected {
            meta.content_kind = Some(found.kind.label().to_owned());
            meta.content_kind_source = Some(found.source.label().to_owned());
        }

        // WS3.J: a page that is itself a bot wall (a CAPTCHA widget, a WAF
        // interstitial, two independent block markers) is not content. Its
        // markdown, links and metadata are computed above (the title for the
        // classifier comes from the metadata pass) and dropped here: nothing of
        // the wall is stored, indexed, ingested or returned, and the scrape fails with a typed
        // `CHALLENGE_DETECTED` whose details carry the block reason and nothing of
        // the page. A status alone never gets here (see `classify_fetched`), and a
        // run under `QUARRY_CHALLENGE_CLASSIFIER=legacy` is not classified at all.
        // `QUARRY_CHALLENGE_ENFORCE=log` counts the page by reason and lets it
        // through. Nothing here logs the URL or the host, ZDR or not.
        let mut bot_wall_flagged = false;
        if !is_doc && is_markup_content_type(ct.as_deref()) {
            let challenge_config = scrape_challenge_config();
            if let Some(reason) = crate::challenge::classify_fetched(
                challenge_config,
                resp.final_url.as_str(),
                resp.status,
                meta.title.as_deref(),
                &resp.body,
            )
            .and_then(|signal| signal.block_reason)
            {
                let reason = reason.as_str();
                // INNH-7: remembered in every mode; in `log` mode the run goes on and
                // the caller (the URL result cache) must still know this is a wall.
                bot_wall_flagged = true;
                if challenge_config.enforce == crate::challenge::EnforceMode::Enforce {
                    tracing::info!(reason, status = resp.status, "quarry.challenge.scrape_refused");
                    self.emit_page_event(
                        run_id.clone(),
                        EventType::PageBlocked,
                        json!({
                            "url": resp.final_url.to_string(),
                            "reason": "challenge_detected",
                            "block_reason": reason,
                            "status": resp.status,
                        }),
                        format!("{}:{}:challenge_detected", run_id, resp.final_url),
                    )
                    .await;
                    return Err(QuarryError::challenge_detected_on_fetch(reason));
                }
                // Validation period: say what would have happened.
                tracing::warn!(reason, status = resp.status, "quarry.challenge.would_refuse_scrape");
            }
        }

        // E3.6-4a: the fail-closed PII redaction stage, before anything of the
        // page is kept, emitted or returned (the fingerprint and change events,
        // branding, artifacts, the corpus, ingest, page_extracted, the inline
        // parts). It runs under ZDR too. With the stage on, every text form of
        // the page is redacted or the page fails with a typed
        // `pii_redaction_failed`: nothing has been written by then, so a failed
        // page leaves nothing behind, and a partial or unredacted text is never
        // used. A capture (screenshot, PDF, action output) cannot be redacted, so
        // a redacting run that carries one fails the same way. The wall check
        // above reads the raw text and keeps nothing of it.
        let redacting = pii_redaction_settings().applies(redact_requested, self.org_id.as_deref());
        let (md, body_text, cleaned_html, link_list, mut meta) = if redacting {
            if let Some(field) = unredactable_capture(resp.captures.as_deref()) {
                return Err(pii_redaction_error(
                    PiiRedactionFailure::CaptureNotRedactable,
                    field,
                ));
            }
            let page = redact_page_text(PageText {
                markdown: md,
                body_text,
                cleaned_html,
                links: link_list,
                metadata: meta,
            })?;
            (
                page.markdown,
                page.body_text,
                page.cleaned_html,
                page.links,
                page.metadata,
            )
        } else {
            (md, body_text, cleaned_html, link_list, meta)
        };

        // Body-content language detection when the HTML lang
        // attribute is missing or generic. Helps downstream LLM
        // prompts pick the right language and consumers filter by
        // detected language even when publisher tags lie.
        if meta.lang.is_none() || meta.lang.as_deref() == Some("") {
            if let Some(d) = quarry_transform::lang::detect(&md) {
                meta.lang = Some(quarry_transform::lang::iso_639_1(d.code).to_string());
            }
        }

        // Fingerprint + diff. `fp` is the byte fingerprint (canonical for
        // artifact addressing); `text_fp` is the whitespace/case-normalized
        // fingerprint included in change events so consumers can suppress
        // boilerplate-only churn at the subscription layer.
        let fp = content_fingerprint(&resp.body);
        let text_fp = quarry_transform::fingerprint::text_fingerprint(&md);
        let prev_fp = prev_fingerprint.map(quarry_transform::Fingerprint);
        let change = diff::compare(prev_fp.as_ref(), &fp);
        {
            use quarry_core::output::ChangeStatus;
            let (evt, key_suffix) = match change {
                ChangeStatus::New | ChangeStatus::Changed => {
                    (EventType::ChangeDetected, "change_detected")
                }
                ChangeStatus::Unchanged => (EventType::ChangeUnchanged, "change_unchanged"),
            };
            self.emit_page_event(
                run_id.clone(),
                evt,
                json!({
                    "url": resp.final_url.to_string(),
                    "fingerprint": fp.0,
                    "text_fingerprint": text_fp.0,
                    "prev": prev_fp.as_ref().map(|p| p.0.clone()),
                }),
                format!("{}:{}:{}", run_id, resp.final_url, key_suffix),
            )
            .await;
        }

        // Per-page branding signals (favicon, theme color, palette, logo
        // candidate, font family, og:image). Emitted on the runtime's
        // local event sink AND surfaced on the NormalizedOutput so the
        // /v1/internal/run_page HTTP response carries it back to the
        // orchestrator (which then re-emits a `branding_extracted`
        // event to control's per-job event log). The second path is
        // what reaches verevon's onboarding wizard when NATS fan-out is
        // disabled. UTF-8 conversion is best-effort — binary responses
        // (images, PDFs) skip both paths cleanly.
        let branding_json: Option<serde_json::Value> = match if is_doc {
            Err(())
        } else {
            std::str::from_utf8(&resp.body).map_err(|_| ())
        } {
            // E3.6-4a: a redacting run emits and keeps the branding only after
            // every string in it has been through the stage.
            Ok(html_str) if redacting => {
                let branding =
                    quarry_transform::branding_rendered::extract(html_str, &resp.final_url);
                let value = serde_json::to_value(&branding)
                    .map_err(|_| PiiRedactionFailure::ReencodeFailed)
                    .and_then(|value| redact_serde(&value))
                    .map_err(|failure| pii_redaction_error(failure, "branding"))?;
                self.emit_page_event(
                    run_id.clone(),
                    EventType::BrandingExtracted,
                    json!({
                        "url": resp.final_url.to_string(),
                        "branding": value.clone(),
                    }),
                    format!("{}:{}:branding", run_id, resp.final_url),
                )
                .await;
                Some(value)
            }
            Ok(html_str) => {
                let branding =
                    quarry_transform::branding_rendered::extract(html_str, &resp.final_url);
                self.emit_page_event(
                    run_id.clone(),
                    EventType::BrandingExtracted,
                    json!({
                        "url": resp.final_url.to_string(),
                        "branding": branding,
                    }),
                    format!("{}:{}:branding", run_id, resp.final_url),
                )
                .await;
                // The emit above and the NormalizedOutput.branding
                // field below should always agree; if to_value
                // ever fails (it shouldn't for our derived
                // Serialize impl) we log it explicitly so we don't
                // diverge silently.
                match serde_json::to_value(&branding) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        // zdr-log-ok: log_url() is None while ZDR is active
                        tracing::warn!(
                            error = %e,
                            url = log_url(self.zdr, resp.final_url.as_str()),
                            "branding serialization failed; NormalizedOutput.branding will be None despite a successful event emit"
                        );
                        None
                    }
                }
            }
            Err(_) => None,
        };

        // Canonical page hash combines normalized URL with content fingerprint,
        // giving a stable artifact key that is independent of run_id.
        let page_hash = quarry_core::artifact::page_hash(resp.final_url.as_ref(), &fp.0);
        // WS13.4b: the captures this run took (no-op unless the request asked).
        let mut captured = self.persist_captures(run_id, &resp, &fp.0).await?;

        // Artifacts — skip durable writes when ZDR is active. Every artifact is
        // stamped with the run's org so `GET /v1/artifacts/{id}` can bind the
        // read back to the same tenant.
        let artifact_org = self.org_id.as_deref().unwrap_or_default();
        // A document's original bytes are stored as `raw`, never as `html`.
        let source_label = if is_doc { "raw" } else { "html" };
        let (html_fmt, md_fmt) = if zdr::guard(self.zdr, WriteKind::Artifact).is_ok() {
            // E3.6-4a: a redacting run keeps the redacted text of a page as its
            // source, and no source at all for a document (its bytes cannot be
            // redacted; its redacted text is the markdown artifact).
            let source_bytes = if !redacting {
                Some(resp.body.clone())
            } else if is_doc {
                None
            } else {
                Some(body_text.as_bytes().to_vec())
            };
            let html_art = match source_bytes {
                Some(bytes) => Some(
                    self.artifacts
                        .put(artifact_org, run_id, &page_hash, source_label, bytes)
                        .await?,
                ),
                None => None,
            };
            let md_art = self
                .artifacts
                .put(
                    artifact_org,
                    run_id,
                    &page_hash,
                    "markdown",
                    md.as_bytes().to_vec(),
                )
                .await?;
            if let Some(html_art) = html_art.as_ref() {
                self.event_sink
                    .emit(
                        run_id.clone(),
                        EventType::ArtifactWritten,
                        json!({
                            "url": resp.final_url.to_string(),
                            "format": source_label,
                            "artifact_id": html_art.artifact_id.to_string(),
                            "bytes": html_art.bytes,
                        }),
                        format!("{}:{}:artifact:{source_label}", run_id, resp.final_url),
                    )
                    .await;
            }
            self.event_sink
                .emit(
                    run_id.clone(),
                    EventType::ArtifactWritten,
                    json!({
                        "url": resp.final_url.to_string(),
                        "format": "markdown",
                        "artifact_id": md_art.artifact_id.to_string(),
                        "bytes": md_art.bytes,
                    }),
                    format!("{}:{}:artifact:markdown", run_id, resp.final_url),
                )
                .await;

            use quarry_core::output::DriverKind;
            // Attribute from the response, not `self.driver.kind()`:
            // `self.driver` may be a FallbackDriver whose kind() reports the
            // planned primary even when a chain rotation meant a different
            // driver served this fetch.
            let plan = match resp.served_by {
                DriverKind::Static => crate::driver_plan::DriverPlan::static_fetch("default"),
                DriverKind::Tls => crate::driver_plan::DriverPlan::tls_fetch(
                    self.driver.tls_profile().unwrap_or_default(),
                    "driver-selected",
                ),
                DriverKind::Browser => crate::driver_plan::DriverPlan::browser_fetch("default"),
            };
            let meta_bytes = serde_json::to_vec(&plan)
                .map_err(|e| QuarryError::new(ErrorCode::Internal, e.to_string()))?;
            let meta_art = self
                .artifacts
                .put(artifact_org, run_id, &page_hash, "meta.json", meta_bytes)
                .await?;
            self.event_sink
                .emit(
                    run_id.clone(),
                    EventType::ArtifactWritten,
                    json!({ "url": resp.final_url.to_string(), "format": "meta", "artifact_id": meta_art.artifact_id.to_string(), "bytes": meta_art.bytes }),
                    format!("{}:{}:artifact:meta", run_id, resp.final_url),
                )
                .await;

            (
                html_art.map(|html_art| FormatRef {
                    artifact_id: html_art.artifact_id,
                    bytes: html_art.bytes,
                }),
                Some(FormatRef {
                    artifact_id: md_art.artifact_id,
                    bytes: md_art.bytes,
                }),
            )
        } else {
            (None, None)
        };

        let fetched_at = Utc::now();

        let output = NormalizedOutput {
            run_id: run_id.clone(),
            url: UrlTriple {
                requested: requested_url.to_string(),
                final_url: resp.final_url.to_string(),
                canonical: None,
            },
            status: resp.status,
            fetched_at,
            fingerprint: fp.0.clone(),
            // E3.5-4: the normalised text fingerprint rides next to the raw one.
            text_fp: Some(text_fp.0.clone()),
            formats: OutputFormats {
                html: if is_doc { None } else { html_fmt.clone() },
                markdown: md_fmt.clone(),
                raw: if is_doc { html_fmt.clone() } else { None },
                links: link_list,
                screenshot: captured.screenshot_ref.clone(),
                pdf: captured.pdf_ref.clone(),
                extract: None,
            },
            change: ChangeInfo {
                status: change,
                prev_fingerprint: prev_fp.map(|p| p.0),
            },
            branding: branding_json.clone(),
            // Caching headers for conditional re-fetches. Persisted on
            // the NormalizedOutput so a future re-crawl can pull the
            // previous ETag/Last-Modified and send conditional GET
            // headers — saving the bandwidth and parse cost of pages
            // that haven't changed.
            etag: resp
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
                .map(|(_, v)| v.clone()),
            last_modified: resp
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("last-modified"))
                .map(|(_, v)| v.clone()),
            metadata: meta,
            driver: {
                let meta = self.driver.browser_meta();
                DriverInfo {
                    // `resp.served_by`, not `self.driver.kind()` — the wrapper
                    // reports the planned primary even after chain rotation.
                    kind: resp.served_by,
                    duration_ms: resp.duration_ms,
                    profile: self.driver.tls_profile().map(|p| match p {
                        quarry_tls::TlsProfile::Chrome => "chrome".to_string(),
                        quarry_tls::TlsProfile::Firefox => "firefox".to_string(),
                        quarry_tls::TlsProfile::Safari => "safari".to_string(),
                    }),
                    version: None,
                    session_id: meta.as_ref().and_then(|m| m.session_id.clone()),
                    live_view_url: meta.as_ref().and_then(|m| m.live_view_url.clone()),
                    recording_id: meta.as_ref().and_then(|m| m.recording_id.clone()),
                }
            },
            // Cycle 21 / cluster #2 — stamp policy + identity so the
            // artifact is self-describing for audit. Two equal stamps
            // on two outputs proves they ran under the same policy;
            // a Strict-mode stamp with mismatched fingerprints is a
            // reproducibility-claim violation.
            determinism: Some({
                let identity =
                    crate::policy::record_determinism_inputs(requested_url.as_str(), &self.policy);
                quarry_core::output::DeterminismStamp {
                    mode: match self.policy.determinism {
                        crate::policy::Determinism::Strict => "strict".into(),
                        crate::policy::Determinism::BestEffort => "best_effort".into(),
                        crate::policy::Determinism::Off => "off".into(),
                    },
                    policy_fp: identity.policy_fp,
                    identity_id: identity.id,
                }
            }),
            privacy: Some(self.privacy.clone().with_zdr(self.zdr)),
            extraction: None,
        };

        // Page-level extraction → `page_extracted` event. Runs AFTER
        // transform (unlike `page_fetched`, which fires pre-transform with
        // only url/status/duration/content_type) so it can carry the title,
        // a plain-text excerpt of the markdown, the word count, the driver
        // that served the fetch, and — when the HTML title is missing or
        // generic — a clean short title proposed by Model Plane. Best-effort
        // end to end: the model hop is time-boxed, ZDR-gated, cached by
        // content fingerprint, and never fails the page. The event is routed
        // through `emit_for_zdr`, so under ZDR it reaches only live SSE
        // subscribers and never the durable publisher or NATS.
        let output = {
            let mut output = output;
            let is_html = ct
                .as_deref()
                .map(|c| c.contains("html") || c.contains("xml"))
                // Browser snapshots and header-less responses are HTML
                // in practice; only an explicit non-HTML MIME opts out.
                .unwrap_or(true);
            if is_html {
                let mut extraction =
                    crate::page_extract::build_extraction(crate::page_extract::ExtractionInput {
                        url: &resp.final_url,
                        html_title: output.metadata.title.as_deref(),
                        markdown: &md,
                        lang: output.metadata.lang.as_deref(),
                        driver: resp.served_by,
                    });
                if extraction.title_source == quarry_core::output::TitleSource::Host {
                    if let Some(enricher) = self.title_enricher.as_ref() {
                        if crate::page_extract::model_hop_allowed(self.zdr, &self.privacy) {
                            if let Some(model_title) = enricher
                                .enrich(
                                    self.org_id.as_deref().filter(|o| !o.is_empty()),
                                    &resp.final_url,
                                    extraction.lang.as_deref(),
                                    &extraction.excerpt,
                                    &fp.0,
                                    !self.zdr.is_active(),
                                )
                                .await
                            {
                                crate::page_extract::apply_model_title(
                                    &mut extraction,
                                    model_title,
                                );
                            }
                        } else {
                            tracing::debug!("page-title model hop skipped by ZDR/privacy posture");
                        }
                    }
                }
                if self.zdr.is_active() {
                    // A ZDR run logs the shape of the page, never which page.
                    tracing::info!(
                        driver = ?resp.served_by,
                        excerpt_chars = extraction.excerpt.chars().count(),
                        word_count = extraction.word_count,
                        "page extracted"
                    );
                } else {
                    // zdr-log-ok: the non-ZDR branch of the `if self.zdr.is_active()` above
                    tracing::info!(
                        url = %resp.final_url,
                        driver = ?resp.served_by,
                        title = %extraction.title,
                        title_source = extraction.title_source.as_str(),
                        excerpt_chars = extraction.excerpt.chars().count(),
                        word_count = extraction.word_count,
                        "page extracted"
                    );
                }
                self.emit_page_event(
                    run_id.clone(),
                    EventType::PageExtracted,
                    crate::page_extract::event_payload(&extraction, ct.as_deref(), &fp.0),
                    format!("{}:{}:extracted", run_id, resp.final_url),
                )
                .await;
                output.extraction = Some(extraction);
            }
            output
        };

        // What is returned inline besides the output. Under ZDR nothing was
        // stored, so the text itself comes back here (capped); a document with
        // no extractable text, or one cut at the parser cap, says so.
        let inline = {
            let mut parts = InlineParts::default();
            parts.bot_wall_suspected = bot_wall_flagged;
            parts.soft_404 = soft_404_flagged;
            let zdr = self.zdr.is_active();
            parts.screenshot = captured.inline_screenshot.take();
            parts.pdf = captured.inline_pdf.take();
            parts.captures = captured.captures.take();
            parts.actions = std::mem::take(&mut captured.actions);
            parts.omitted.append(&mut captured.omitted);
            parts.warnings.append(&mut captured.warnings);
            let cap = inline_request.max_bytes;
            if let Some(max_chars) = inline_request.ai_source_max_chars {
                // The AI formats read the text itself: not subject to the inline
                // cap, and present even when markdown was not asked for.
                let end = md.char_indices().nth(max_chars).map_or(md.len(), |(byte, _)| byte);
                parts.ai_source = Some(md[..end].to_owned());
            }
            if inline_request.markdown {
                offer_inline(
                    "markdown",
                    &md,
                    cap,
                    zdr,
                    &mut parts.markdown,
                    &mut parts.truncated,
                    &mut parts.omitted,
                );
            }
            if is_doc {
                // A document's bytes are not text; its extracted text is the
                // markdown format.
                for (wanted, field) in [(inline_request.html, "html"), (inline_request.raw_html, "raw_html")] {
                    if wanted {
                        parts.omitted.push(field.to_owned());
                    }
                }
                if inline_request.html || inline_request.raw_html {
                    parts.warnings.push(quarry_core::scrape::ScrapeWarning::new(
                        "format_unavailable_for_documents",
                        "html and rawHtml are not available for a PDF or DOCX; use markdown",
                    ));
                }
            } else {
                if inline_request.html {
                    // The HTML the markdown was made from: after exclusion,
                    // inclusion and cleaning, or the main content by default.
                    let html_text = cleaned_html.clone().unwrap_or_else(|| {
                        quarry_transform::clean_html::to_markdown(&body_text, &inline_request.clean).html
                    });
                    offer_inline(
                        "html",
                        &html_text,
                        cap,
                        zdr,
                        &mut parts.html,
                        &mut parts.truncated,
                        &mut parts.omitted,
                    );
                }
                if inline_request.raw_html {
                    offer_inline(
                        "raw_html",
                        &body_text,
                        cap,
                        zdr,
                        &mut parts.raw_html,
                        &mut parts.truncated,
                        &mut parts.omitted,
                    );
                }
            }
            for warning in &clean_warnings {
                parts
                    .warnings
                    .push(quarry_core::scrape::ScrapeWarning::new(warning.code, warning.message));
            }
            if let Some(parsed) = &document {
                if parsed.truncated {
                    parts.warnings.push(quarry_core::scrape::ScrapeWarning::new(
                        "document_text_truncated",
                        "the document's text was cut at the parser's output cap",
                    ));
                }
                if md.trim().is_empty() {
                    parts.warnings.push(quarry_core::scrape::ScrapeWarning::new(
                        "document_has_no_text_layer",
                        "no text could be extracted; the file may be a scan (OCR is not applied)",
                    ));
                }
            }
            parts
        };
        // An empty document is not worth ingesting.
        let skip_ingest = is_doc && md.trim().is_empty();

        // Cycle 19 / cluster #16: index successful scrape into the local
        // Tantivy corpus so subsequent `/v1/search` queries hit warm.
        // Everything about the decision — ZDR, tenant, empty body — lives in
        // the shared hook; see [`LocalIndexWriteback`].
        self.local_index_writeback().submit(IndexedPage {
            url: resp.final_url.to_string(),
            title: output.metadata.title.clone(),
            markdown: md.clone(),
            fingerprint: fp.0.clone(),
            fetched_at,
            org_id: self.org_id.clone(),
        });

        if let (Some(ingest), Some(org_id)) = (&self.ingest, &self.org_id) {
            if zdr::guard(self.zdr, WriteKind::Artifact).is_ok() && !skip_ingest {
                let chunks: Vec<ChunkRef> = chunk::paragraph_chunks(&md, 1000)
                    .into_iter()
                    .map(|c| ChunkRef {
                        start: c.start,
                        end: c.end,
                        text: c.text,
                    })
                    .collect();

                let source_trace = SourceTrace {
                    source_url: resp.final_url.to_string(),
                    fetched_at,
                    fingerprint: fp.0.clone(),
                    field_traces: vec![
                        quarry_core::contracts::FieldTrace {
                            field: "title".into(),
                            source_url: resp.final_url.to_string(),
                            selector: Some("meta[property='og:title'], title, h1".into()),
                        },
                        quarry_core::contracts::FieldTrace {
                            field: "description".into(),
                            source_url: resp.final_url.to_string(),
                            selector: Some(
                                "meta[property='og:description'], meta[name='description']".into(),
                            ),
                        },
                        quarry_core::contracts::FieldTrace {
                            field: "content".into(),
                            source_url: resp.final_url.to_string(),
                            selector: Some("readability → markdown".into()),
                        },
                    ],
                };

                // Preserve full SEO + link metadata for Firecrawl parity.
                // Previously this was `json!({})`, dropping description/author/dates/lang.
                let ingest_metadata = {
                    let mut m = serde_json::Map::new();
                    if let Some(desc) = &output.metadata.description {
                        m.insert(
                            "description".into(),
                            serde_json::Value::String(desc.clone()),
                        );
                    }
                    if let Some(author) = &output.metadata.author {
                        m.insert("author".into(), serde_json::Value::String(author.clone()));
                    }
                    if let Some(lang) = &output.metadata.lang {
                        m.insert("language".into(), serde_json::Value::String(lang.clone()));
                    }
                    if let Some(ct) = &output.metadata.content_type {
                        m.insert("content_type".into(), serde_json::Value::String(ct.clone()));
                    }
                    if let Some(pub_at) = &output.metadata.published_at {
                        m.insert(
                            "published_at".into(),
                            serde_json::Value::String(pub_at.clone()),
                        );
                    }
                    if let Some(mod_at) = &output.metadata.modified_at {
                        m.insert(
                            "modified_at".into(),
                            serde_json::Value::String(mod_at.clone()),
                        );
                    }
                    if let Some(canonical) = &output.metadata.canonical_url {
                        m.insert(
                            "canonical_url".into(),
                            serde_json::Value::String(canonical.clone()),
                        );
                    } else if let Some(canonical) = &output.url.canonical {
                        m.insert(
                            "canonical_url".into(),
                            serde_json::Value::String(canonical.clone()),
                        );
                    }
                    if let Some(keywords) = &output.metadata.keywords {
                        m.insert(
                            "keywords".into(),
                            serde_json::Value::String(keywords.clone()),
                        );
                    }
                    if let Some(og_image) = &output.metadata.og_image {
                        m.insert(
                            "og_image".into(),
                            serde_json::Value::String(og_image.clone()),
                        );
                    }
                    if let Some(robots) = &output.metadata.robots {
                        m.insert("robots".into(), serde_json::Value::String(robots.clone()));
                    }
                    // Persist extracted links (Firecrawl parity: links array)
                    if !output.formats.links.is_empty() {
                        if let Ok(links_val) = serde_json::to_value(&output.formats.links) {
                            m.insert("links".into(), links_val);
                        }
                    }
                    // Persist branding signals (logo, colors, fonts) for UI
                    if let Some(branding) = &branding_json {
                        m.insert("branding".into(), branding.clone());
                    }
                    // Persist driver provenance for debugging
                    m.insert(
                        "driver_kind".into(),
                        serde_json::Value::String(format!("{:?}", output.driver.kind)),
                    );
                    m.insert(
                        "fetched_at".into(),
                        serde_json::Value::String(fetched_at.to_rfc3339()),
                    );
                    serde_json::Value::Object(m)
                };

                let ingest_req = DataPlaneIngestRequest {
                    run_id: run_id.clone(),
                    org_id: org_id.clone(),
                    source_url: resp.final_url.to_string(),
                    title: output.metadata.title.clone(),
                    markdown: Some(md),
                    html_ref: if is_doc {
                        None
                    } else {
                        html_fmt.as_ref().map(|f| f.artifact_id.clone())
                    },
                    raw_ref: if is_doc {
                        html_fmt.as_ref().map(|f| f.artifact_id.clone())
                    } else {
                        None
                    },
                    chunks,
                    metadata: ingest_metadata,
                    fingerprint: fp.0.clone(),
                    zdr: self.zdr,
                    retention_policy: self.privacy.retention_policy.clone(),
                    privacy_policy: Some(self.privacy.clone().with_zdr(self.zdr)),
                    source_trace: Some(source_trace),
                    // Private-by-default: forward the initiating user so the Data
                    // Plane stamps owner=user, visibility=private. `None` (system
                    // ingest) → org-visible. We don't set `visibility` explicitly
                    // here; documents-api derives it from viewer presence.
                    initiator_user_id: self.user_id.clone(),
                    visibility: None,
                };

                let ingest = ingest.clone();
                let event_sink = self.event_sink.clone();
                let ingest_run_id = run_id.clone();
                let ingest_url = resp.final_url.to_string();
                let cancel_token = self.cancel_token.clone();
                // Phase-2 visual RAG producer — captured into the ingest task so
                // the page render fires once the DP2 document_id is known.
                let page_renderer = self.page_renderer.clone();
                let render_org = org_id.clone();
                let render_title = output.metadata.title.clone();
                let render_zdr = self.zdr;
                let source_registrar = self.source_registrar.clone();
                let registrar_org = org_id.clone();
                tokio::spawn(async move {
                    let ingest_fut = async {
                        match ingest.ingest(&ingest_req).await {
                            Ok(resp) => {
                                tracing::info!(
                                    document_id = %resp.document_id,
                                    index_status = ?resp.index_status,
                                    "data plane ingest succeeded"
                                );
                                // Visual arm: render the page → CAS → emit
                                // page_images.created. No-op when ZDR is on or the
                                // producer is unconfigured; never fatal to ingest.
                                if let Some(renderer) = &page_renderer {
                                    if let Err(e) = renderer
                                        .render_and_emit(
                                            &render_org,
                                            &resp.document_id,
                                            &ingest_url,
                                            render_title.clone(),
                                            render_zdr,
                                        )
                                        .await
                                    {
                                        tracing::warn!(error_class = e.code.wire_name(), document_id = %resp.document_id, "page-image render/emit failed (non-fatal)");
                                    }
                                }
                                event_sink
                                    .emit(
                                        ingest_run_id,
                                        EventType::StoreRecordWritten,
                                        json!({
                                            "url": ingest_url.clone(),
                                            "document_id": resp.document_id,
                                            "index_status": resp.index_status,
                                        }),
                                        format!("{}:ingest:written", ingest_req.run_id),
                                    )
                                    .await;

                                // P0 fix — Aquatiq crawl-to-KB pipeline
                                // (2026-07-20), part 2: on a successful
                                // ingest, best-effort materialize a durable
                                // "tracked website" Source row for this
                                // host so the Knowledge Base's "Tracked web
                                // sources" panel isn't permanently empty for
                                // crawl-originated content. Keyed on the
                                // page's host root (not the full page URL)
                                // so a multi-page crawl of one site collapses
                                // into a single row via the receiver's
                                // upsert-by-(org_id, url). Registration
                                // failure is logged and never fails the
                                // ingest that already succeeded above.
                                if let Some(registrar) = &source_registrar {
                                    if let Ok(parsed) = url::Url::parse(&ingest_url) {
                                        if let Some(host) = parsed.host_str() {
                                            let root = format!("{}://{}", parsed.scheme(), host);
                                            if let Err(e) = registrar
                                                .register_source(
                                                    &registrar_org,
                                                    host,
                                                    &root,
                                                    "crawl",
                                                )
                                                .await
                                            {
                                                tracing::warn!(
                                                    error_class = e.code.wire_name(),
                                                    "tracked-source registration failed (non-fatal)"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error_class = e.code.wire_name(), "data plane ingest failed (non-fatal)");
                                // P0 fix — Aquatiq crawl-to-KB pipeline (2026-07-20): a
                                // failed Data Plane write was previously ONLY logged,
                                // never surfaced on the job's event stream, so a crawl
                                // could report "completed"/"indexed" while zero pages
                                // were ever durably persisted. Emit a distinguishable
                                // event on the SAME sink `StoreRecordWritten` uses so
                                // consumers (gateway, SPA status label) can tell real
                                // ingest failure apart from a page that was never asked
                                // to ingest in the first place.
                                event_sink
                                    .emit(
                                        ingest_run_id,
                                        EventType::StoreRecordFailed,
                                        json!({
                                            "url": ingest_url,
                                            "error": e.to_string(),
                                        }),
                                        format!("{}:ingest:failed", ingest_req.run_id),
                                    )
                                    .await;
                            }
                        }
                    };

                    // Race ingest against the cancellation token. If the
                    // parent run is cancelled (e.g., via Temporal cancel
                    // signal), abort the ingest cleanly so we don't leave
                    // orphan documents in the Data Plane.
                    match cancel_token {
                        Some(tok) => {
                            tokio::select! {
                                _ = ingest_fut => {}
                                _ = tok.cancelled() => {
                                    tracing::info!(
                                        "ingest aborted by cancellation token"
                                    );
                                }
                            }
                        }
                        None => ingest_fut.await,
                    }
                });
            }
        }

        Ok((output, inline))
    }
}

/// WS14.S4: an error that is a robots.txt refusal. Its `PageBlocked` event is
/// already out, so the caller does not add a `PageFailed`.
fn is_robots_denial(e: &QuarryError) -> bool {
    e.code == ErrorCode::SecurityBlocked
        && e.details
            .as_ref()
            .and_then(|d| d.get("denial"))
            .and_then(|d| d.get("code"))
            .and_then(|c| c.as_str())
            .is_some_and(|c| c.starts_with("robots_"))
}

/// "the page is an image file ...", "a PDF file ..." - the article follows the label.
fn unsupported_media_message(label: &str) -> String {
    let article = if label.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    };
    format!("the page is {article} {label} file, which Quarry cannot extract text from")
}

#[cfg(test)]
mod unsupported_media_message_tests {
    use super::unsupported_media_message;

    #[test]
    fn the_article_follows_the_label() {
        assert!(unsupported_media_message("image").starts_with("the page is an image file"));
        assert!(unsupported_media_message("archive").starts_with("the page is an archive file"));
        assert!(unsupported_media_message("spreadsheet").starts_with("the page is a spreadsheet file"));
    }
}

#[cfg(test)]
mod zdr_event_tests {
    use super::*;

    #[test]
    fn zdr_page_events_are_ephemeral_only() {
        assert!(!page_event_persistence_allowed(ZdrMode::On));
        assert!(page_event_persistence_allowed(ZdrMode::Off));
    }
}

#[cfg(test)]
mod page_extracted_tests {
    use super::*;
    use crate::artifact_store::InMemoryStore;
    use crate::driver::Driver;
    use crate::fetch::FetchResponse;
    use async_trait::async_trait;
    use quarry_core::output::{DriverKind, TitleSource};
    use tokio::sync::mpsc;

    /// Canned HTML page whose `<title>` is just the host name — the exact
    /// aquatiq.com symptom — with real body text.
    struct ShellTitleDriver;

    const PAGE: &str = "<html lang=\"no\"><head><title>aquatiq.com</title></head><body><main><h1>Hygiene for matindustrien</h1><p>Vi leverer rengjøringsløsninger, kjemikalier og kompetanse til næringsmiddelindustrien i hele Norden. Trygg mat starter med rent utstyr.</p></main></body></html>";

    #[async_trait]
    impl Driver for ShellTitleDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Browser
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
                body: PAGE.as_bytes().to_vec(),
                duration_ms: 3,
                served_by: DriverKind::Browser,
                captures: None,
            })
        }
    }

    fn runner(tx: mpsc::Sender<quarry_core::event::Event>) -> PageRunner {
        PageRunner {
            driver: Arc::new(ShellTitleDriver),
            security: Arc::new(
                quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true),
            ),
            artifacts: Arc::new(InMemoryStore::new()),
            event_sink: EventSink::new(tx),
            zdr: ZdrMode::Off,
            ingest: None,
            org_id: Some("org_test".into()),
            user_id: None,
            privacy: PrivacyPolicy::default(),
            cancel_token: None,
            local_index: None,
            policy: RunPolicy::default(),
            scheduler: None,
            autoscale: None,
            render: RenderHints::default(),
            page_renderer: None,
            source_registrar: None,
            title_enricher: None,
            doc_routing: DocRouting::Off,
            robots: None,
        }
    }

    #[tokio::test]
    async fn emits_page_extracted_with_excerpt_and_host_title_when_no_enricher() {
        let (tx, mut rx) = mpsc::channel(64);
        let runner = runner(tx);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://www.aquatiq.com/".parse().unwrap();
        let out = runner.run(&run_id, &url, None).await.expect("run ok");

        let extraction = out.extraction.expect("extraction present for html");
        assert_eq!(extraction.title, "aquatiq.com");
        assert_eq!(extraction.title_source, TitleSource::Host);
        assert_eq!(extraction.driver, DriverKind::Browser);
        assert!(
            extraction.excerpt.starts_with("Hygiene for matindustrien"),
            "{}",
            extraction.excerpt
        );
        assert!(extraction.word_count >= 15);
        assert_eq!(extraction.lang.as_deref(), Some("no"));

        let mut saw_extracted = None;
        let mut order = Vec::new();
        while let Ok(evt) = rx.try_recv() {
            order.push(evt.event_type);
            if evt.event_type == EventType::PageExtracted {
                saw_extracted = Some(evt.payload);
            }
        }
        let payload = saw_extracted.expect("page_extracted emitted");
        assert_eq!(payload["title_source"], "host");
        assert_eq!(payload["driver"], "browser");
        assert_eq!(payload["url"], "https://www.aquatiq.com/");
        assert!(payload["excerpt"].as_str().unwrap().contains("Vi leverer"));
        // Emitted after the pre-transform page_fetched, as documented.
        let fetched = order
            .iter()
            .position(|t| *t == EventType::PageFetched)
            .unwrap();
        let extracted = order
            .iter()
            .position(|t| *t == EventType::PageExtracted)
            .unwrap();
        assert!(extracted > fetched);
    }

    #[tokio::test]
    async fn zdr_run_keeps_page_extracted_off_the_durable_channel() {
        let (tx, mut rx) = mpsc::channel(64);
        let mut runner = runner(tx);
        runner.zdr = ZdrMode::On;
        let run_id: RunKind = quarry_core::ids::Id::new();
        let mut live = runner.event_sink.subscribe(&run_id);
        let url: Url = "https://www.aquatiq.com/".parse().unwrap();
        let out = runner.run(&run_id, &url, None).await.expect("run ok");
        assert!(
            out.extraction.is_some(),
            "ZDR still yields an in-memory extraction"
        );

        let mut durable_types = Vec::new();
        while let Ok(evt) = rx.try_recv() {
            durable_types.push(evt.event_type);
        }
        assert!(
            !durable_types.contains(&EventType::PageExtracted),
            "ZDR page_extracted must not reach the durable publisher: {durable_types:?}"
        );
        let mut live_types = Vec::new();
        while let Ok(evt) = live.try_recv() {
            live_types.push(evt.event_type);
        }
        assert!(
            live_types.contains(&EventType::PageExtracted),
            "{live_types:?}"
        );
    }
}

#[cfg(test)]
mod local_index_writeback_tests {
    use super::*;
    use crate::artifact_store::InMemoryStore;
    use crate::driver::Driver;
    use crate::fetch::FetchResponse;
    use crate::local_index::TantivyLocalIndex;
    use crate::serp::{SearchOptions, SearchProvider};
    use async_trait::async_trait;
    use quarry_core::output::DriverKind;
    use tokio::sync::mpsc;

    const PAGE: &str = "<html lang=\"en\"><head><title>Tokio runtime guide</title></head><body><main><h1>Tokio runtime guide</h1><p>The runtime drives futures to completion across a work-stealing scheduler, with timers and IO drivers attached to each worker thread.</p></main></body></html>";

    struct StaticPageDriver;

    #[async_trait]
    impl Driver for StaticPageDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
                body: PAGE.as_bytes().to_vec(),
                duration_ms: 2,
                served_by: DriverKind::Static,
                captures: None,
            })
        }
    }

    fn runner(
        tx: mpsc::Sender<quarry_core::event::Event>,
        org_id: Option<&str>,
        index: Option<TantivyLocalIndex>,
        zdr: ZdrMode,
    ) -> PageRunner {
        PageRunner {
            driver: Arc::new(StaticPageDriver),
            security: Arc::new(
                quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true),
            ),
            artifacts: Arc::new(InMemoryStore::new()),
            event_sink: EventSink::new(tx),
            zdr,
            ingest: None,
            org_id: org_id.map(|o| o.to_string()),
            user_id: None,
            privacy: PrivacyPolicy::default(),
            cancel_token: None,
            local_index: index,
            policy: RunPolicy::default(),
            scheduler: None,
            autoscale: None,
            render: RenderHints::default(),
            page_renderer: None,
            source_registrar: None,
            title_enricher: None,
            doc_routing: DocRouting::Off,
            robots: None,
        }
    }

    fn page(org_id: Option<&str>) -> IndexedPage {
        IndexedPage {
            url: "https://example.com/doc".into(),
            title: Some("Doc".into()),
            markdown: "# Doc\n\nSome real body text.".into(),
            fingerprint: "blake3:deadbeef".into(),
            fetched_at: Utc::now(),
            org_id: org_id.map(|o| o.to_string()),
        }
    }

    /// Wait for the detached write-back task to land, so the assertions below
    /// test the write rather than the scheduler.
    async fn await_pending(idx: &TantivyLocalIndex, expected: u64) {
        for _ in 0..200 {
            if idx.pending_writes() >= expected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!(
            "write-back did not reach the index: pending={}",
            idx.pending_writes()
        );
    }

    // ── decision rules ──────────────────────────────────────────────────────

    #[test]
    fn unknown_org_is_never_indexed() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let hook = LocalIndexWriteback::new(Some(idx), ZdrMode::Off);
        // Absent, empty, and whitespace-only all mean the same thing: we do
        // not know whose page this is.
        for org in [None, Some(""), Some("   ")] {
            assert_eq!(
                hook.decide(&page(org)),
                WritebackDecision::SkippedUnknownOrg,
                "org {org:?} must not be indexed"
            );
        }
        assert_eq!(
            hook.decide(&page(Some("org_a"))),
            WritebackDecision::Index,
            "a page with a verified tenant is still indexed"
        );
    }

    #[test]
    fn zdr_blocks_the_write_back() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let hook = LocalIndexWriteback::new(Some(idx), ZdrMode::On);
        assert_eq!(
            hook.decide(&page(Some("org_a"))),
            WritebackDecision::SkippedZdr
        );
        assert!(!hook.is_enabled());
    }

    #[test]
    fn no_index_and_empty_body_are_distinguished() {
        let hook = LocalIndexWriteback::new(None, ZdrMode::Off);
        assert_eq!(
            hook.decide(&page(Some("org_a"))),
            WritebackDecision::SkippedNoIndex
        );

        let idx = TantivyLocalIndex::in_memory().unwrap();
        let hook = LocalIndexWriteback::new(Some(idx), ZdrMode::Off);
        let mut empty = page(Some("org_a"));
        empty.markdown = "   \n  ".into();
        assert_eq!(hook.decide(&empty), WritebackDecision::SkippedEmptyBody);
    }

    #[test]
    fn zdr_is_checked_before_the_tenant() {
        // Ordering matters for the reason a skip is reported, and ZDR is the
        // stronger statement: under ZDR nothing is written regardless of who
        // the page belongs to.
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let hook = LocalIndexWriteback::new(Some(idx), ZdrMode::On);
        assert_eq!(hook.decide(&page(None)), WritebackDecision::SkippedZdr);
    }

    // ── through the pipeline ────────────────────────────────────────────────

    #[tokio::test]
    async fn successful_scrape_with_a_tenant_reaches_the_corpus() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let (tx, _rx) = mpsc::channel(64);
        let runner = runner(tx, Some("org_a"), Some(idx.clone()), ZdrMode::Off);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.com/tokio".parse().unwrap();
        runner.run(&run_id, &url, None).await.expect("run ok");

        await_pending(&idx, 1).await;
        idx.flush().await.unwrap();
        assert_eq!(idx.doc_count(), 1);

        let opts = SearchOptions {
            org_id: Some("org_a".into()),
            ..Default::default()
        };
        let hits = idx.search("tokio runtime", &opts).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].url.contains("/tokio"));
    }

    #[tokio::test]
    async fn scrape_without_a_tenant_indexes_nothing() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let (tx, _rx) = mpsc::channel(64);
        let runner = runner(tx, None, Some(idx.clone()), ZdrMode::Off);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.com/tokio".parse().unwrap();
        runner.run(&run_id, &url, None).await.expect("run ok");

        // Nothing was submitted, so there is nothing to wait for; give any
        // stray task a chance to run before asserting the corpus is empty.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(idx.pending_writes(), 0);
        idx.flush().await.unwrap();
        assert_eq!(idx.doc_count(), 0);

        // And the document is not reachable by an unscoped read either —
        // the failure mode this whole guard exists to prevent.
        let unscoped = idx
            .search_all_orgs("tokio runtime", &SearchOptions::default())
            .await
            .unwrap();
        assert!(unscoped.is_empty(), "leaked into the shared corpus");
    }

    #[tokio::test]
    async fn zdr_scrape_indexes_nothing() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let (tx, _rx) = mpsc::channel(64);
        let runner = runner(tx, Some("org_a"), Some(idx.clone()), ZdrMode::On);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.com/tokio".parse().unwrap();
        runner.run(&run_id, &url, None).await.expect("run ok");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        idx.flush().await.unwrap();
        assert_eq!(idx.doc_count(), 0, "ZDR run must leave no durable trace");
    }

    #[tokio::test]
    async fn re_scraping_the_same_url_does_not_accumulate_copies() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let url: Url = "https://example.com/tokio".parse().unwrap();
        for _ in 0..3 {
            let (tx, _rx) = mpsc::channel(64);
            let runner = runner(tx, Some("org_a"), Some(idx.clone()), ZdrMode::Off);
            let run_id: RunKind = quarry_core::ids::Id::new();
            runner.run(&run_id, &url, None).await.expect("run ok");
            await_pending(&idx, 1).await;
            idx.flush().await.unwrap();
        }
        assert_eq!(idx.doc_count(), 1, "re-fetch must upsert, not append");
    }
}

#[cfg(test)]
mod content_routing_tests {
    use super::*;
    use crate::artifact_store::InMemoryStore;
    use crate::driver::Driver;
    use crate::fetch::FetchResponse;
    use async_trait::async_trait;
    use quarry_core::output::DriverKind;
    use tokio::sync::mpsc;

    /// Serves one canned body with a chosen status and Content-Type.
    struct BodyDriver {
        status: u16,
        content_type: Option<&'static str>,
        body: Vec<u8>,
    }

    #[async_trait]
    impl Driver for BodyDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            Ok(FetchResponse {
                status: self.status,
                final_url: url.clone(),
                headers: self
                    .content_type
                    .map(|ct| vec![("content-type".into(), ct.into())])
                    .unwrap_or_default(),
                body: self.body.clone(),
                duration_ms: 1,
                served_by: DriverKind::Static,
                captures: None,
            })
        }
    }

    const HTML: &str = "<html><head><title>Hei</title></head><body><main><h1>Hei</h1><p>En vanlig nettside med nok tekst til at den teller som innhold.</p></main></body></html>";
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01";

    fn runner_for(
        mode: DocRouting,
        status: u16,
        content_type: Option<&'static str>,
        body: &[u8],
    ) -> (PageRunner, mpsc::Receiver<quarry_core::event::Event>) {
        let (tx, rx) = mpsc::channel(64);
        let runner = PageRunner {
            driver: Arc::new(BodyDriver {
                status,
                content_type,
                body: body.to_vec(),
            }),
            security: Arc::new(
                quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true),
            ),
            artifacts: Arc::new(InMemoryStore::new()),
            event_sink: EventSink::new(tx),
            zdr: ZdrMode::Off,
            ingest: None,
            org_id: Some("org_test".into()),
            user_id: None,
            privacy: PrivacyPolicy::default(),
            cancel_token: None,
            local_index: None,
            policy: RunPolicy::default(),
            scheduler: None,
            autoscale: None,
            render: RenderHints::default(),
            page_renderer: None,
            source_registrar: None,
            title_enricher: None,
            doc_routing: mode,
            robots: None,
        };
        (runner, rx)
    }

    fn url() -> Url {
        "https://docs.example.test/file".parse().unwrap()
    }

    fn drain(rx: &mut mpsc::Receiver<quarry_core::event::Event>) -> Vec<quarry_core::event::Event> {
        let mut events = Vec::new();
        while let Ok(evt) = rx.try_recv() {
            events.push(evt);
        }
        events
    }

    #[tokio::test]
    async fn png_body_is_skipped_with_unsupported_media_and_a_page_blocked_event() {
        let (runner, mut rx) = runner_for(DocRouting::Skip, 200, Some("image/png"), PNG);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let error = runner
            .run(&run_id, &url(), None)
            .await
            .expect_err("an image must be refused");
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        assert_eq!(error.code.http_status(), 415);
        assert!(!error.retryable);
        let details = error.details.expect("typed details");
        assert_eq!(details["reason"], "unsupported_media");
        assert_eq!(details["kind"], "image");
        assert_eq!(details["subtype"], "png");
        assert_eq!(details["source"], "magic");
        assert_eq!(details["bytes"], PNG.len());

        let events = drain(&mut rx);
        let blocked = events
            .iter()
            .find(|e| e.event_type == EventType::PageBlocked)
            .expect("page_blocked emitted");
        assert_eq!(blocked.payload["reason"], "unsupported_media");
        assert_eq!(blocked.payload["kind"], "image");
        let fetched = events
            .iter()
            .find(|e| e.event_type == EventType::PageFetched)
            .expect("page_fetched emitted");
        assert_eq!(fetched.payload["content_kind"], "image");
    }

    #[tokio::test]
    async fn shadow_records_kind_but_never_skips() {
        let (runner, mut rx) = runner_for(DocRouting::Shadow, 200, Some("image/png"), PNG);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner
            .run(&run_id, &url(), None)
            .await
            .expect("shadow never refuses");
        assert_eq!(out.metadata.content_kind.as_deref(), Some("image"));
        assert_eq!(out.metadata.content_kind_source.as_deref(), Some("magic"));
        let fetched = drain(&mut rx)
            .into_iter()
            .find(|e| e.event_type == EventType::PageFetched)
            .unwrap();
        assert_eq!(fetched.payload["content_kind"], "image");
    }

    #[tokio::test]
    async fn off_detects_nothing_and_changes_nothing() {
        let (runner, mut rx) = runner_for(DocRouting::Off, 200, Some("image/png"), PNG);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner
            .run(&run_id, &url(), None)
            .await
            .expect("legacy path");
        assert!(out.metadata.content_kind.is_none());
        let fetched = drain(&mut rx)
            .into_iter()
            .find(|e| e.event_type == EventType::PageFetched)
            .unwrap();
        assert!(fetched.payload["content_kind"].is_null());
    }

    #[tokio::test]
    async fn non_2xx_binary_keeps_legacy_path() {
        let (runner, _rx) = runner_for(DocRouting::Skip, 404, Some("image/png"), PNG);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner
            .run(&run_id, &url(), None)
            .await
            .expect("an error status is not classified");
        assert!(out.metadata.content_kind.is_none());
    }

    #[tokio::test]
    async fn a_pdf_body_is_recorded_and_not_skipped_in_skip_mode() {
        let (runner, _rx) = runner_for(
            DocRouting::Skip,
            200,
            Some("application/octet-stream"),
            b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\n",
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner
            .run(&run_id, &url(), None)
            .await
            .expect("pdf takes the legacy path until routing is on");
        assert_eq!(out.metadata.content_kind.as_deref(), Some("pdf"));
        assert_eq!(out.metadata.content_kind_source.as_deref(), Some("magic"));
    }

    #[tokio::test]
    async fn html_output_is_identical_across_off_shadow_and_skip() {
        fn comparable(out: &NormalizedOutput) -> serde_json::Value {
            let mut value = serde_json::to_value(out).unwrap();
            let object = value.as_object_mut().unwrap();
            for volatile in ["run_id", "fetched_at", "driver", "determinism"] {
                object.remove(volatile);
            }
            if let Some(meta) = object.get_mut("metadata").and_then(|m| m.as_object_mut()) {
                meta.remove("content_kind");
                meta.remove("content_kind_source");
            }
            // Artifact ids are minted per run.
            if let Some(formats) = object.get_mut("formats").and_then(|m| m.as_object_mut()) {
                formats.remove("html");
                formats.remove("markdown");
            }
            value
        }
        let mut outputs = Vec::new();
        for mode in [DocRouting::Off, DocRouting::Shadow, DocRouting::Skip] {
            let (runner, _rx) =
                runner_for(mode, 200, Some("text/html; charset=utf-8"), HTML.as_bytes());
            let run_id: RunKind = quarry_core::ids::Id::new();
            let out = runner.run(&run_id, &url(), None).await.expect("html ok");
            outputs.push((mode, out));
        }
        let baseline = comparable(&outputs[0].1);
        for (mode, out) in &outputs[1..] {
            assert_eq!(
                comparable(out),
                baseline,
                "{mode:?} changed the HTML output"
            );
            assert_eq!(out.metadata.content_kind.as_deref(), Some("html"));
        }
        // The fingerprint (what change monitors compare) never moves.
        assert!(outputs
            .iter()
            .all(|(_, o)| o.fingerprint == outputs[0].1.fingerprint));
    }

    // ---- routing PDF and DOCX to their text extractors (mode `on`) ----

    /// A valid one-page PDF whose text layer is `text`, with a correct xref table.
    fn minimal_pdf(text: &str) -> Vec<u8> {
        let content = format!("BT /F1 24 Tf 100 700 Td ({text}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref_at = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in entries {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(data).unwrap();
            }
            zw.finish().unwrap();
        }
        buf
    }

    fn docx_with(text: &str) -> Vec<u8> {
        zip_bytes(&[
            (
                "[Content_Types].xml",
                b"<Types>application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml</Types>",
            ),
            (
                "word/document.xml",
                format!("<w:document><w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:body></w:document>").as_bytes(),
            ),
        ])
    }

    async fn stored_text(runner: &PageRunner, out: &NormalizedOutput) -> String {
        let id = out
            .formats
            .markdown
            .as_ref()
            .expect("a markdown artifact")
            .artifact_id
            .clone();
        String::from_utf8(runner.artifacts.get("org_test", &id).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn pdf_body_routes_to_pdf_parser_not_readability() {
        let pdf = minimal_pdf("Hallo dokument fra Verevon");
        let (runner, _rx) = runner_for(DocRouting::On, 200, Some("application/octet-stream"), &pdf);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner.run(&run_id, &url(), None).await.expect("pdf parsed");
        assert_eq!(out.metadata.content_kind.as_deref(), Some("pdf"));
        assert!(stored_text(&runner, &out)
            .await
            .contains("Hallo dokument fra Verevon"));
        // The original bytes are the `raw` format, never `html`.
        assert!(out.formats.html.is_none());
        let raw = out.formats.raw.as_ref().expect("raw format");
        let bytes = runner
            .artifacts
            .get("org_test", &raw.artifact_id)
            .await
            .unwrap();
        assert_eq!(bytes, pdf);
        assert!(out.formats.links.is_empty());
        // The fingerprint stays the hash of the raw body.
        assert_eq!(
            out.fingerprint,
            quarry_transform::fingerprint::content_fingerprint(&pdf).0
        );
    }

    #[tokio::test]
    async fn docx_zip_routes_to_docx_parser() {
        let docx = docx_with("Kontrakt mellom partene");
        let (runner, _rx) = runner_for(DocRouting::On, 200, Some("application/zip"), &docx);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner
            .run(&run_id, &url(), None)
            .await
            .expect("docx parsed");
        assert_eq!(out.metadata.content_kind.as_deref(), Some("docx"));
        assert!(stored_text(&runner, &out)
            .await
            .contains("Kontrakt mellom partene"));
    }

    #[tokio::test]
    async fn plain_zip_and_xlsx_are_unsupported_media() {
        let plain = zip_bytes(&[("a.txt", b"hello"), ("b.txt", b"world")]);
        let xlsx = zip_bytes(&[(
            "[Content_Types].xml",
            b"<Types>spreadsheetml.sheet.main</Types>",
        )]);
        for (body, kind) in [(plain, "archive"), (xlsx, "xlsx")] {
            let (runner, _rx) =
                runner_for(DocRouting::On, 200, Some("application/octet-stream"), &body);
            let run_id: RunKind = quarry_core::ids::Id::new();
            let error = runner.run(&run_id, &url(), None).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::UnsupportedMedia, "{kind}");
            assert_eq!(error.details.unwrap()["kind"], kind);
        }
    }

    #[tokio::test]
    async fn a_corrupt_pdf_is_a_typed_415_with_a_parse_reason() {
        let (runner, mut rx) = runner_for(
            DocRouting::On,
            200,
            Some("application/pdf"),
            b"%PDF-1.4 not really a pdf",
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let error = runner.run(&run_id, &url(), None).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        assert_eq!(error.details.unwrap()["reason"], "pdf_parse_failed");
        let blocked = drain(&mut rx)
            .into_iter()
            .find(|e| e.event_type == EventType::PageBlocked)
            .expect("page_blocked for the parse failure");
        assert_eq!(blocked.payload["reason"], "document_parse_failed");
    }

    #[tokio::test]
    async fn skip_mode_does_not_parse_documents() {
        let pdf = minimal_pdf("Hallo");
        let (runner, _rx) = runner_for(DocRouting::Skip, 200, Some("application/pdf"), &pdf);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner
            .run(&run_id, &url(), None)
            .await
            .expect("legacy path");
        // Legacy path: the PDF is stored as html, exactly as before.
        assert!(out.formats.html.is_some());
        assert!(out.formats.raw.is_none());
    }

    #[tokio::test]
    async fn zdr_pdf_writes_no_artifact_and_returns_inline_markdown() {
        let pdf = minimal_pdf("Hemmelig dokument");
        let (mut runner, _rx) = runner_for(DocRouting::On, 200, Some("application/pdf"), &pdf);
        runner.zdr = ZdrMode::On;
        let store = Arc::new(InMemoryStore::new());
        runner.artifacts = store.clone();
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (out, inline) = runner
            .run_with_inline(&run_id, &url(), None)
            .await
            .expect("zdr pdf parsed");
        assert!(
            out.formats.markdown.is_none()
                && out.formats.raw.is_none()
                && out.formats.html.is_none()
        );
        let listed = store
            .list("org_test", &quarry_core::pagination::ListFilter::default())
            .await
            .unwrap();
        assert!(listed.items.is_empty(), "ZDR must write no artifact");
        let text = inline.markdown.expect("inline text under ZDR");
        assert!(text.contains("Hemmelig dokument"), "{text}");
        assert!(inline.truncated.is_empty());
    }

    #[tokio::test]
    async fn zdr_html_returns_inline_markdown_and_non_zdr_does_not() {
        let (mut zdr_runner, _rx) =
            runner_for(DocRouting::Off, 200, Some("text/html"), HTML.as_bytes());
        zdr_runner.zdr = ZdrMode::On;
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (_, inline) = zdr_runner
            .run_with_inline(&run_id, &url(), None)
            .await
            .unwrap();
        assert!(inline.markdown.unwrap().contains("En vanlig nettside"));

        let (runner, _rx) = runner_for(DocRouting::Off, 200, Some("text/html"), HTML.as_bytes());
        let (_, inline) = runner.run_with_inline(&run_id, &url(), None).await.unwrap();
        assert!(
            inline.markdown.is_none(),
            "stored runs return no inline text"
        );
    }

    #[tokio::test]
    async fn a_pdf_without_a_text_layer_warns_and_skips_ingest() {
        let pdf = minimal_pdf("");
        let (runner, _rx) = runner_for(DocRouting::On, 200, Some("application/pdf"), &pdf);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (_, inline) = runner.run_with_inline(&run_id, &url(), None).await.unwrap();
        assert!(inline
            .warnings
            .iter()
            .any(|w| w.code == "document_has_no_text_layer"));
    }

    #[tokio::test]
    async fn html_output_is_unchanged_with_routing_on() {
        let (off, _r1) = runner_for(
            DocRouting::Off,
            200,
            Some("text/html; charset=utf-8"),
            HTML.as_bytes(),
        );
        let (on, _r2) = runner_for(
            DocRouting::On,
            200,
            Some("text/html; charset=utf-8"),
            HTML.as_bytes(),
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let a = off.run(&run_id, &url(), None).await.unwrap();
        let b = on.run(&run_id, &url(), None).await.unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_eq!(a.metadata.title, b.metadata.title);
        assert_eq!(a.formats.links.len(), b.formats.links.len());
        assert_eq!(
            a.formats.markdown.as_ref().map(|f| f.bytes),
            b.formats.markdown.as_ref().map(|f| f.bytes)
        );
        assert!(b.formats.raw.is_none() && b.formats.html.is_some());
    }

    #[test]
    fn doc_routing_parses_known_values_and_names_the_variable_on_error() {
        assert_eq!(DocRouting::parse("off"), Ok(DocRouting::Off));
        assert_eq!(DocRouting::parse(" Shadow "), Ok(DocRouting::Shadow));
        assert_eq!(DocRouting::parse(""), Ok(DocRouting::Shadow));
        assert_eq!(DocRouting::parse("SKIP"), Ok(DocRouting::Skip));
        assert_eq!(DocRouting::parse("on"), Ok(DocRouting::On));
        let error = DocRouting::parse("maybe").unwrap_err();
        assert!(error.contains("QUARRY_EDGE__DOC_ROUTING"), "{error}");
        assert_eq!(DocRouting::default(), DocRouting::Shadow);
    }
    // ---- E3.6-6b: Office bodies through the imports-core client

    /// Installs the Office flag and, optionally, a canned client reply for this
    /// thread. Both are removed when the guard is dropped.
    struct OfficeGuard;

    impl Drop for OfficeGuard {
        fn drop(&mut self) {
            TEST_OFFICE_VIA_IMPORTS.with(|slot| *slot.borrow_mut() = None);
            TEST_OFFICE_REPLY.with(|slot| *slot.borrow_mut() = None);
        }
    }

    fn office_flag(mode: OfficeViaImports, reply: Option<TestOfficeReply>) -> OfficeGuard {
        TEST_OFFICE_VIA_IMPORTS.with(|slot| *slot.borrow_mut() = Some(mode));
        TEST_OFFICE_REPLY.with(|slot| *slot.borrow_mut() = reply);
        OfficeGuard
    }

    /// True while the canned reply is still waiting, that is, the client was not
    /// called.
    fn reply_pending() -> bool {
        TEST_OFFICE_REPLY.with(|slot| slot.borrow().is_some())
    }

    fn text_reply(text: &str) -> TestOfficeReply {
        Ok(crate::content_route::ParsedDocument {
            text: text.to_owned(),
            truncated: false,
        })
    }

    fn xlsx_body() -> Vec<u8> {
        zip_bytes(&[(
            "[Content_Types].xml",
            b"<Types>spreadsheetml.sheet.main</Types>",
        )])
    }

    #[test]
    fn the_office_flag_is_off_unless_it_says_on() {
        assert_eq!(OfficeViaImports::default(), OfficeViaImports::Off);
        assert_eq!(OfficeViaImports::parse(""), Ok(OfficeViaImports::Off));
        assert_eq!(OfficeViaImports::parse(" off "), Ok(OfficeViaImports::Off));
        assert_eq!(OfficeViaImports::parse("0"), Ok(OfficeViaImports::Off));
        assert_eq!(OfficeViaImports::parse("On"), Ok(OfficeViaImports::On));
        assert_eq!(OfficeViaImports::parse(" TRUE "), Ok(OfficeViaImports::On));
        let error = OfficeViaImports::parse("maybe").unwrap_err();
        assert!(error.contains("QUARRY_EDGE__OFFICE_VIA_IMPORTS"), "{error}");
    }

    #[test]
    fn the_office_route_needs_the_document_lane_the_flag_and_an_office_kind() {
        use quarry_transform::sniff::ContentKind as Kind;
        for kind in [Kind::Xlsx, Kind::Pptx, Kind::Odf, Kind::Epub] {
            assert!(
                takes_imports_route(DocRouting::On, OfficeViaImports::On, kind),
                "{kind:?}"
            );
            assert!(
                !takes_imports_route(DocRouting::On, OfficeViaImports::Off, kind),
                "{kind:?}"
            );
            for mode in [DocRouting::Off, DocRouting::Shadow, DocRouting::Skip] {
                assert!(
                    !takes_imports_route(mode, OfficeViaImports::On, kind),
                    "{mode:?} {kind:?}"
                );
            }
        }
        // PDF and DOCX keep their Rust extractors; nothing else is an Office kind.
        let others = [
            Kind::Pdf,
            Kind::Docx,
            Kind::Archive,
            Kind::Image,
            Kind::Html,
            Kind::OleCompound,
        ];
        for kind in others {
            assert!(
                !takes_imports_route(DocRouting::On, OfficeViaImports::On, kind),
                "{kind:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_xlsx_takes_the_imports_client_when_the_flag_is_on() {
        let _flag = office_flag(
            OfficeViaImports::On,
            Some(text_reply("Kostnader per kvartal")),
        );
        let (runner, _rx) = runner_for(
            DocRouting::On,
            200,
            Some("application/octet-stream"),
            &xlsx_body(),
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner
            .run(&run_id, &url(), None)
            .await
            .expect("xlsx extracted by imports");
        assert_eq!(out.metadata.content_kind.as_deref(), Some("xlsx"));
        assert!(stored_text(&runner, &out)
            .await
            .contains("Kostnader per kvartal"));
        // The original bytes are the raw format, never html, as for a PDF.
        assert!(out.formats.html.is_none());
        assert!(!reply_pending(), "the client must have been called");
    }

    #[tokio::test]
    async fn with_the_flag_off_an_xlsx_is_still_a_typed_415() {
        let _flag = office_flag(OfficeViaImports::Off, Some(text_reply("unused")));
        let (runner, _rx) = runner_for(
            DocRouting::On,
            200,
            Some("application/octet-stream"),
            &xlsx_body(),
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let error = runner.run(&run_id, &url(), None).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        assert_eq!(error.details.unwrap()["kind"], "xlsx");
        assert!(
            reply_pending(),
            "the client must not be called with the flag off"
        );
    }

    #[tokio::test]
    async fn skip_refuses_and_shadow_leaves_an_xlsx_alone_with_the_flag_on() {
        let _flag = office_flag(OfficeViaImports::On, Some(text_reply("unused")));
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (skip, _skip_rx) = runner_for(
            DocRouting::Skip,
            200,
            Some("application/octet-stream"),
            &xlsx_body(),
        );
        let error = skip.run(&run_id, &url(), None).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        let (shadow, _shadow_rx) = runner_for(
            DocRouting::Shadow,
            200,
            Some("application/octet-stream"),
            &xlsx_body(),
        );
        let _ = shadow.run(&run_id, &url(), None).await;
        assert!(reply_pending(), "neither mode routes an Office body");
    }

    #[tokio::test]
    async fn an_imports_refusal_is_the_pages_typed_error_on_the_imports_route() {
        let refusal =
            QuarryError::new(ErrorCode::UnsupportedMedia, "imports-core refused the file");
        let _flag = office_flag(OfficeViaImports::On, Some(Err(refusal)));
        let (runner, mut rx) = runner_for(
            DocRouting::On,
            200,
            Some("application/octet-stream"),
            &xlsx_body(),
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let error = runner.run(&run_id, &url(), None).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        let blocked = drain(&mut rx)
            .into_iter()
            .find(|e| e.event_type == EventType::PageBlocked)
            .expect("page_blocked for the refusal");
        assert_eq!(blocked.payload["reason"], "document_parse_failed");
        assert_eq!(blocked.payload["route"], "imports");
        assert_eq!(blocked.payload["kind"], "xlsx");
    }

    #[tokio::test]
    async fn a_pdf_keeps_the_rust_extractor_with_the_flag_on() {
        let _flag = office_flag(OfficeViaImports::On, Some(text_reply("unused")));
        let pdf = minimal_pdf("Hallo dokument fra Verevon");
        let (runner, _rx) = runner_for(DocRouting::On, 200, Some("application/pdf"), &pdf);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner.run(&run_id, &url(), None).await.expect("pdf parsed");
        assert_eq!(out.metadata.content_kind.as_deref(), Some("pdf"));
        assert!(stored_text(&runner, &out)
            .await
            .contains("Hallo dokument fra Verevon"));
        assert!(reply_pending(), "a PDF never reaches the imports client");
    }

    #[tokio::test]
    async fn a_plain_zip_stays_a_typed_415_with_the_flag_on() {
        let _flag = office_flag(OfficeViaImports::On, Some(text_reply("unused")));
        let plain = zip_bytes(&[("a.txt", b"hello"), ("b.txt", b"world")]);
        let (runner, _rx) = runner_for(
            DocRouting::On,
            200,
            Some("application/octet-stream"),
            &plain,
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let error = runner.run(&run_id, &url(), None).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        assert_eq!(error.details.unwrap()["kind"], "archive");
        assert!(reply_pending(), "an archive is not an Office kind");
    }

    #[tokio::test]
    async fn zdr_office_text_is_returned_inline_and_nothing_is_stored() {
        let _flag = office_flag(OfficeViaImports::On, Some(text_reply("Hemmelig ark")));
        let (mut runner, _rx) = runner_for(
            DocRouting::On,
            200,
            Some("application/octet-stream"),
            &xlsx_body(),
        );
        runner.zdr = ZdrMode::On;
        let store = Arc::new(InMemoryStore::new());
        runner.artifacts = store.clone();
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (out, inline) = runner
            .run_with_inline(&run_id, &url(), None)
            .await
            .expect("zdr xlsx extracted");
        assert!(
            out.formats.markdown.is_none()
                && out.formats.raw.is_none()
                && out.formats.html.is_none()
        );
        let listed = store
            .list("org_test", &quarry_core::pagination::ListFilter::default())
            .await
            .unwrap();
        assert!(listed.items.is_empty(), "ZDR must write no artifact");
        let text = inline.markdown.expect("inline text under ZDR");
        assert!(text.contains("Hemmelig ark"), "{text}");
    }

    // ---- WS5.D: what a caller can ask to get inline -------------------------

    fn want(markdown: bool, html: bool, raw_html: bool, max_bytes: usize) -> InlineRequest {
        InlineRequest {
            markdown,
            html,
            raw_html,
            max_bytes,
            clean: Default::default(),
            ai_source_max_chars: None,
        }
    }

    #[tokio::test]
    async fn html_and_raw_html_come_back_only_when_asked_for() {
        let (runner, _rx) = runner_for(DocRouting::Off, 200, Some("text/html"), HTML.as_bytes());
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (_, none) = runner
            .run_with_inline_request(&run_id, &url(), None, &want(false, false, false, 1 << 20))
            .await
            .unwrap();
        assert!(none.markdown.is_none() && none.html.is_none() && none.raw_html.is_none());
        assert!(none.truncated.is_empty() && none.omitted.is_empty());

        let (_, both) = runner
            .run_with_inline_request(&run_id, &url(), None, &want(true, true, true, 1 << 20))
            .await
            .unwrap();
        assert!(both.markdown.unwrap().contains("En vanlig nettside"));
        // html is what the markdown came from; raw_html is the document as fetched.
        assert!(both.html.unwrap().contains("En vanlig nettside"));
        assert_eq!(both.raw_html.as_deref(), Some(HTML));
    }

    #[tokio::test]
    async fn an_over_cap_field_is_omitted_outside_zdr_and_cut_under_it() {
        let (runner, _rx) = runner_for(DocRouting::Off, 200, Some("text/html"), HTML.as_bytes());
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (out, inline) = runner
            .run_with_inline_request(&run_id, &url(), None, &want(true, true, false, 8))
            .await
            .unwrap();
        assert!(inline.markdown.is_none() && inline.html.is_none());
        assert_eq!(inline.omitted, vec!["markdown", "html"]);
        assert!(out.formats.markdown.is_some(), "the stored artifact still holds it");

        let (mut zdr, _rx) = runner_for(DocRouting::Off, 200, Some("text/html"), HTML.as_bytes());
        zdr.zdr = ZdrMode::On;
        let (_, inline) = zdr
            .run_with_inline_request(&run_id, &url(), None, &want(true, true, false, 8))
            .await
            .unwrap();
        assert!(inline.markdown.as_ref().unwrap().len() <= 8);
        assert!(inline.html.as_ref().unwrap().len() <= 8);
        assert_eq!(inline.truncated, vec!["markdown", "html"]);
        assert!(inline.omitted.is_empty());
    }

    #[tokio::test]
    async fn a_multibyte_character_straddling_the_cap_is_cut_whole_not_split() {
        // "æøå" is six bytes; a cap of 5 must keep two characters, not panic.
        let page = "<html><body><p>æøåæøå</p></body></html>";
        let (mut runner, _rx) = runner_for(DocRouting::Off, 200, Some("text/html"), page.as_bytes());
        runner.zdr = ZdrMode::On;
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (_, inline) = runner
            .run_with_inline_request(&run_id, &url(), None, &want(false, false, true, 5))
            .await
            .unwrap();
        let cut = inline.raw_html.expect("cut raw html");
        assert!(cut.len() <= 5 && std::str::from_utf8(cut.as_bytes()).is_ok());
        assert_eq!(inline.truncated, vec!["raw_html"]);
    }

    #[tokio::test]
    async fn a_document_has_markdown_inline_but_no_html() {
        let pdf = minimal_pdf("Dokumenttekst");
        let (runner, _rx) = runner_for(DocRouting::On, 200, Some("application/pdf"), &pdf);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (_, inline) = runner
            .run_with_inline_request(&run_id, &url(), None, &want(true, true, true, 1 << 20))
            .await
            .expect("pdf parsed");
        assert!(inline.markdown.unwrap().contains("Dokumenttekst"));
        assert!(inline.html.is_none() && inline.raw_html.is_none());
        assert_eq!(inline.omitted, vec!["html", "raw_html"]);
        assert!(inline
            .warnings
            .iter()
            .any(|warning| warning.code == "format_unavailable_for_documents"));
    }

    // ---- INNH-1: the page text for the AI formats ---------------------------

    #[tokio::test]
    async fn ai_source_is_there_without_markdown_and_over_the_inline_cap() {
        let (runner, _rx) = runner_for(DocRouting::Off, 200, Some("text/html"), HTML.as_bytes());
        let run_id: RunKind = quarry_core::ids::Id::new();
        let mut request = want(false, false, false, 8);
        request.ai_source_max_chars = Some(80_000);
        let (out, inline) = runner
            .run_with_inline_request(&run_id, &url(), None, &request)
            .await
            .unwrap();
        // markdown was not asked for and 8 bytes could not hold it, yet the AI formats get it whole.
        assert!(inline.markdown.is_none() && inline.omitted.is_empty());
        let source = inline.ai_source.expect("ai source");
        assert!(source.contains("En vanlig nettside"), "{source}");
        assert!(out.formats.markdown.is_some());

        // The same text a markdown request returns.
        let (_, with_markdown) = runner
            .run_with_inline_request(&run_id, &url(), None, &want(true, false, false, 1 << 20))
            .await
            .unwrap();
        assert_eq!(Some(source), with_markdown.markdown);
    }

    #[tokio::test]
    async fn ai_source_is_cut_on_a_char_boundary_and_left_out_unless_asked() {
        let page = "<html><body><p>æøåæøåæøå</p></body></html>";
        let (runner, _rx) = runner_for(DocRouting::Off, 200, Some("text/html"), page.as_bytes());
        let run_id: RunKind = quarry_core::ids::Id::new();
        let mut request = want(false, false, false, 1 << 20);
        request.ai_source_max_chars = Some(4);
        let (_, inline) = runner
            .run_with_inline_request(&run_id, &url(), None, &request)
            .await
            .unwrap();
        assert_eq!(inline.ai_source.as_deref(), Some("æøåæ"));

        let (_, off) = runner
            .run_with_inline_request(&run_id, &url(), None, &want(true, true, true, 1 << 20))
            .await
            .unwrap();
        assert!(off.ai_source.is_none(), "not asked for, not returned");
    }

    #[tokio::test]
    async fn ai_source_comes_back_under_zdr_too() {
        let (mut runner, _rx) = runner_for(DocRouting::Off, 200, Some("text/html"), HTML.as_bytes());
        runner.zdr = ZdrMode::On;
        let run_id: RunKind = quarry_core::ids::Id::new();
        let mut request = want(false, false, false, 8);
        request.ai_source_max_chars = Some(1_000);
        let (_, inline) = runner
            .run_with_inline_request(&run_id, &url(), None, &request)
            .await
            .unwrap();
        assert!(inline.ai_source.is_some());
        // InlineParts is not a wire type (it derives no Serialize), so the text
        // cannot reach a response by accident; the edge copies named fields only.
    }

    #[tokio::test]
    async fn the_default_inline_request_is_the_old_behaviour() {
        assert_eq!(
            InlineRequest::default_for(ZdrMode::On),
            want(true, false, false, quarry_core::scrape::DEFAULT_INLINE_MAX_BYTES)
        );
        assert_eq!(
            InlineRequest::default_for(ZdrMode::Off),
            want(false, false, false, quarry_core::scrape::DEFAULT_INLINE_MAX_BYTES)
        );
    }

}

#[cfg(test)]
mod scrape_challenge_tests {
    use super::*;
    use crate::artifact_store::InMemoryStore;
    use crate::challenge::{ChallengeConfig, EnforceMode};
    use crate::driver::Driver;
    use crate::fetch::FetchResponse;
    use async_trait::async_trait;
    use quarry_core::output::DriverKind;
    use tokio::sync::mpsc;

    struct BodyDriver {
        status: u16,
        content_type: Option<&'static str>,
        body: Vec<u8>,
    }

    #[async_trait]
    impl Driver for BodyDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            Ok(FetchResponse {
                status: self.status,
                final_url: url.clone(),
                headers: self
                    .content_type
                    .map(|ct| vec![("content-type".into(), ct.into())])
                    .unwrap_or_default(),
                body: self.body.clone(),
                duration_ms: 1,
                served_by: DriverKind::Static,
                captures: None,
            })
        }
    }

    const CF_INTERSTITIAL: &str = r#"<!DOCTYPE html><html lang="en-US"><head><title>Just a moment...</title></head><body class="no-js"><h2 id="challenge-running">Checking if the site connection is secure</h2><div id="challenge-stage"></div><script>window._cf_chl_opt={cvId:'3'};var a=document.createElement('script');a.src='/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=8abc';</script><script src="https://challenges.cloudflare.com/turnstile/v0/g/abc/api.js"></script></body></html>"#;
    const TURNSTILE_PAGE: &str = r#"<html><head><title>Turnstile Test</title></head><body><h1>Demo</h1><div class="cf-turnstile" data-sitekey="1x00000000000000000000AA"></div><script src="https://challenges.cloudflare.com/turnstile/v0/api.js" async defer></script></body></html>"#;
    const NGINX_403: &str = "<html>\r\n<head><title>403 Forbidden</title></head>\r\n<body>\r\n<center><h1>403 Forbidden</h1></center>\r\n<hr><center>nginx</center>\r\n</body>\r\n</html>";
    const LOGIN_PAGE: &str = r#"<html><head><title>Sign in</title></head><body><form action="/session" method="post"><label>Email <input type="email" name="email"></label><label>Password <input type="password" name="password"></label><button>Sign in</button></form></body></html>"#;
    const ARTICLE: &str = "<html><head><title>How CAPTCHAs work</title></head><body><main><h1>How CAPTCHAs work</h1><p>A CAPTCHA is a test that tells computers and humans apart. This article explains what a CAPTCHA is and why access to a page is sometimes forbidden to a bot.</p></main></body></html>";
    const CONTACT_WITH_WIDGET: &str = r#"<html><head><title>Contact us</title></head><body><form action="/send" method="post"><label>Name <input type="text" name="n"></label><label>Email <input type="email" name="e"></label><textarea name="m"></textarea><div class="g-recaptcha" data-sitekey="k"></div><button>Send</button></form><script src="https://www.google.com/recaptcha/api.js"></script></body></html>"#;

    /// Makes per-thread tracing subscribers (`set_default`) reliable for every test in
    /// this module, whatever order the parallel tests reach a callsite in.
    ///
    /// tracing-core caches an "interest" per callsite, computed when the callsite is
    /// first hit. While only ONE dispatcher is registered process-wide it takes a
    /// shortcut and asks only the CURRENT THREAD's default dispatcher. A test thread
    /// with no subscriber that is first to hit a callsite (the two challenge lines in
    /// `PageRunner::run`) therefore caches "never", and can overwrite the "sometimes"
    /// that a concurrent `set_default` in the log test had just computed. The log test
    /// then sees zero events: the flake of R8b (assertion left 0, right 1). Two
    /// dispatchers that live for the whole process turn the shortcut off for good: the
    /// interest is then always computed over every registered dispatcher, so a callsite
    /// is never cached as "never" while a capturing subscriber is alive, and each event
    /// asks the hitting thread's own subscriber. Called before any scrape in the module.
    fn keep_per_thread_subscribers_reliable() {
        static KEEP: std::sync::OnceLock<[tracing::Dispatch; 2]> = std::sync::OnceLock::new();
        KEEP.get_or_init(|| {
            [
                tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()),
                tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()),
            ]
        });
    }

    /// Installs a challenge config for the current thread and removes it again.
    /// `#[tokio::test]` runs on the current thread, so the scrape sees it.
    struct Installed;
    impl Installed {
        fn new(config: ChallengeConfig) -> Self {
            keep_per_thread_subscribers_reliable();
            TEST_CHALLENGE_CONFIG.with(|slot| slot.set(Some(config)));
            Installed
        }
    }
    impl Drop for Installed {
        fn drop(&mut self) {
            TEST_CHALLENGE_CONFIG.with(|slot| slot.set(None));
        }
    }

    fn runner_for(
        zdr: ZdrMode,
        status: u16,
        content_type: Option<&'static str>,
        body: &str,
    ) -> (PageRunner, mpsc::Receiver<quarry_core::event::Event>, Arc<InMemoryStore>) {
        let (tx, rx) = mpsc::channel(64);
        let artifacts = Arc::new(InMemoryStore::new());
        let runner = PageRunner {
            driver: Arc::new(BodyDriver { status, content_type, body: body.as_bytes().to_vec() }),
            security: Arc::new(
                quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true),
            ),
            artifacts: artifacts.clone(),
            event_sink: EventSink::new(tx),
            zdr,
            ingest: None,
            org_id: Some("org_test".into()),
            user_id: None,
            privacy: PrivacyPolicy::default(),
            cancel_token: None,
            local_index: None,
            policy: RunPolicy::default(),
            scheduler: None,
            autoscale: None,
            render: RenderHints::default(),
            page_renderer: None,
            source_registrar: None,
            title_enricher: None,
            doc_routing: DocRouting::Off,
            robots: None,
        };
        (runner, rx, artifacts)
    }

    fn target() -> Url {
        "https://walled.example.test/page".parse().unwrap()
    }

    fn drain(rx: &mut mpsc::Receiver<quarry_core::event::Event>) -> Vec<quarry_core::event::Event> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    async fn scrape(
        zdr: ZdrMode,
        status: u16,
        content_type: Option<&'static str>,
        body: &str,
    ) -> (QuarryResult<NormalizedOutput>, Vec<quarry_core::event::Event>) {
        let (runner, mut rx, _) = runner_for(zdr, status, content_type, body);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let result = runner.run(&run_id, &target(), None).await;
        (result, drain(&mut rx))
    }

    #[tokio::test]
    async fn a_strong_block_page_fails_the_scrape_with_its_block_reason() {
        let _config = Installed::new(ChallengeConfig::enforcing());
        // (what the driver served, status, expected block reason)
        let cases = [
            ("cloudflare interstitial", 403, CF_INTERSTITIAL, "waf_interstitial"),
            ("cloudflare interstitial served as 200", 200, CF_INTERSTITIAL, "waf_interstitial"),
            ("a turnstile page", 200, TURNSTILE_PAGE, "captcha_widget"),
        ];
        for (name, status, body, reason) in cases {
            let (result, events) = scrape(ZdrMode::Off, status, Some("text/html"), body).await;
            let error = result.expect_err(name);
            assert_eq!(error.code, ErrorCode::ChallengeDetected, "{name}");
            assert_eq!(error.code.http_status(), 403, "{name}");
            let details = error.details.as_ref().expect("details");
            assert_eq!(details["block_reason"], reason, "{name}");
            assert_eq!(details["recovery"], "not_applicable", "{name}");
            assert_eq!(details["next"], "other_source", "{name}");
            // Nothing of the page, the host or the URL in the error.
            let wire = serde_json::to_string(&error).unwrap();
            for secret in ["walled.example", "cloudflare", "turnstile", "Just a moment"] {
                assert!(!wire.contains(secret), "{name}: {secret} in {wire}");
            }
            let blocked = events
                .iter()
                .find(|e| e.event_type == EventType::PageBlocked)
                .unwrap_or_else(|| panic!("{name}: page_blocked emitted"));
            assert_eq!(blocked.payload["reason"], "challenge_detected", "{name}");
            assert_eq!(blocked.payload["block_reason"], reason, "{name}");
            // The wall is not content: nothing past the fetch event was emitted.
            for event in &events {
                assert!(
                    !matches!(
                        event.event_type,
                        EventType::ChangeDetected | EventType::BrandingExtracted | EventType::ArtifactWritten
                    ),
                    "{name}: {:?}",
                    event.event_type
                );
            }
        }
    }

    #[tokio::test]
    async fn a_refused_wall_stores_no_artifact() {
        let _config = Installed::new(ChallengeConfig::enforcing());
        let (runner, _rx, artifacts) = runner_for(ZdrMode::Off, 403, Some("text/html"), CF_INTERSTITIAL);
        let run_id: RunKind = quarry_core::ids::Id::new();
        assert!(runner.run(&run_id, &target(), None).await.is_err());
        assert_eq!(
            artifacts.count("org_test").await.unwrap(),
            Some(0),
            "a refused wall must not be stored"
        );
    }

    #[tokio::test]
    async fn pages_that_are_not_walls_still_scrape() {
        let _config = Installed::new(ChallengeConfig::enforcing());
        let cases = [
            ("an ordinary article about captchas", 200, ARTICLE),
            ("a login page", 200, LOGIN_PAGE),
            ("a bare nginx 403 (a status alone is no verdict)", 403, NGINX_403),
            ("a 429 page", 429, "<html><head><title>Too many requests</title></head><body>Slow down.</body></html>"),
            ("a contact form with a recaptcha", 200, CONTACT_WITH_WIDGET),
        ];
        for (name, status, body) in cases {
            let (result, _events) = scrape(ZdrMode::Off, status, Some("text/html"), body).await;
            let out = result.unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(out.status, status, "{name}");
        }
    }

    #[tokio::test]
    async fn only_markup_is_classified() {
        let _config = Installed::new(ChallengeConfig::enforcing());
        // The same bytes under a binary or JSON type are not a page to classify.
        for content_type in ["application/json", "application/octet-stream", "image/png"] {
            let (result, _) = scrape(ZdrMode::Off, 200, Some(content_type), CF_INTERSTITIAL).await;
            assert!(result.is_ok(), "{content_type}: {:?}", result.err());
        }
        // No content type reads as HTML.
        let (result, _) = scrape(ZdrMode::Off, 200, None, CF_INTERSTITIAL).await;
        assert_eq!(result.expect_err("html").code, ErrorCode::ChallengeDetected);
    }

    /// INNH-7: in `log` mode (the shipped default) a bot wall and a soft-404 return an
    /// Ok output, so the inline parts carry the verdict the URL result cache needs.
    #[tokio::test]
    async fn log_mode_still_flags_a_bot_wall_and_a_soft_404_on_the_inline_parts() {
        const SOFT_404: &str = "<html><head><title>Oops</title></head><body><h1>Page not found</h1><p>Sorry.</p></body></html>";
        let _config = Installed::new(ChallengeConfig::from_values(None, Some("log")));
        let inline_of = |body: &'static str| async move {
            let (runner, _rx, _) = runner_for(ZdrMode::Off, 200, Some("text/html"), body);
            let run_id: RunKind = quarry_core::ids::Id::new();
            runner
                .run_with_inline(&run_id, &target(), None)
                .await
                .expect("log mode returns an output")
                .1
        };
        let wall = inline_of(TURNSTILE_PAGE).await;
        assert!(wall.bot_wall_suspected && !wall.soft_404, "{wall:?}");
        let wall = inline_of(CF_INTERSTITIAL).await;
        assert!(wall.bot_wall_suspected, "{wall:?}");
        let soft = inline_of(SOFT_404).await;
        assert!(soft.soft_404 && !soft.bot_wall_suspected, "{soft:?}");
        // Ordinary pages carry neither flag.
        for body in [ARTICLE, LOGIN_PAGE, CONTACT_WITH_WIDGET] {
            let page = inline_of(body).await;
            assert!(!page.bot_wall_suspected && !page.soft_404, "{page:?}");
        }
    }

    #[tokio::test]
    async fn the_legacy_classifier_leaves_the_scrape_path_as_it_was() {
        let _config = Installed::new(ChallengeConfig::from_values(Some("legacy"), None));
        let (result, _) = scrape(ZdrMode::Off, 403, Some("text/html"), CF_INTERSTITIAL).await;
        assert_eq!(result.expect("legacy scrapes the wall as before").status, 403);
    }

    /// Collects tracing events as text for the thread that installs it.
    #[derive(Clone, Default)]
    struct EventLog(Arc<std::sync::Mutex<Vec<String>>>);

    impl EventLog {
        fn lines(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
        fn matching(&self, message: &str) -> Vec<String> {
            self.lines().into_iter().filter(|line| line.contains(message)).collect()
        }
    }

    impl tracing::Subscriber for EventLog {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            struct Fields(String);
            impl tracing::field::Visit for Fields {
                fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                    if !self.0.is_empty() {
                        self.0.push(' ');
                    }
                    self.0.push_str(&format!("{}={:?}", field.name(), value));
                }
            }
            let mut fields = Fields(String::new());
            event.record(&mut fields);
            self.0.lock().unwrap().push(format!("{} {}", event.metadata().level(), fields.0));
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    // The EventLog is installed with `set_default`, which is per THREAD, so this test
    // needs the scrape to run on the test's own thread: a current-thread runtime
    // (spelled out here, not left to the default) guarantees that. If the scrape work
    // were ever spawned elsewhere the `len() == 1` assertions below would fail loudly
    // (zero lines), never pass vacuously.
    #[tokio::test(flavor = "current_thread")]
    async fn the_scrape_path_logs_the_reason_and_the_status_and_never_the_host() {
        // Before the first `set_default` below, so no registration races it.
        keep_per_thread_subscribers_reliable();
        for zdr in [ZdrMode::Off, ZdrMode::On] {
            // Enforce: one refusal line.
            let events = EventLog::default();
            {
                let _log = tracing::subscriber::set_default(events.clone());
                let _config = Installed::new(ChallengeConfig::enforcing());
                let (result, _) = scrape(zdr, 403, Some("text/html"), CF_INTERSTITIAL).await;
                assert!(result.is_err(), "{zdr:?}");
            }
            let refused = events.matching("quarry.challenge.scrape_refused");
            assert_eq!(refused.len(), 1, "{:?}", events.lines());
            assert!(refused[0].contains("reason=\"waf_interstitial\""), "{refused:?}");
            assert!(refused[0].contains("status=403"), "{refused:?}");
            assert!(events.matching("would_refuse_scrape").is_empty(), "{:?}", events.lines());

            // Log: one would-refuse line, the page goes through.
            let events = EventLog::default();
            {
                let _log = tracing::subscriber::set_default(events.clone());
                let _config = Installed::new(ChallengeConfig::from_values(None, Some("log")));
                let (result, _) = scrape(zdr, 403, Some("text/html"), CF_INTERSTITIAL).await;
                assert!(result.is_ok(), "{zdr:?}");
            }
            let would = events.matching("quarry.challenge.would_refuse_scrape");
            assert_eq!(would.len(), 1, "{:?}", events.lines());
            assert!(would[0].contains("reason=\"waf_interstitial\""), "{would:?}");
            assert!(would[0].contains("status=403"), "{would:?}");
            assert!(events.matching("scrape_refused").is_empty(), "{:?}", events.lines());

            // Neither the host nor anything of the page is in a challenge line. (The
            // generic "page extracted" line of a page that goes through logs its URL
            // on a non-ZDR run like every other page does; that is the existing
            // behaviour of the scrape path and not part of this contract.)
            for line in events.matching("quarry.challenge") {
                assert!(!line.contains("walled.example"), "{line}");
                assert!(!line.contains("Just a moment"), "{line}");
            }
            // Under ZDR the walled page that goes through in log mode leaves no
            // host in ANY line, the generic "page extracted" line included.
            if zdr == ZdrMode::On {
                for line in events.lines() {
                    assert!(!line.contains("walled.example"), "{line}");
                }
            }
        }
        // A page that is not a wall logs nothing about challenges.
        let events = EventLog::default();
        {
            let _log = tracing::subscriber::set_default(events.clone());
            let _config = Installed::new(ChallengeConfig::enforcing());
            let (result, _) = scrape(ZdrMode::Off, 200, Some("text/html"), ARTICLE).await;
            assert!(result.is_ok());
        }
        assert!(events.matching("quarry.challenge").is_empty(), "{:?}", events.lines());
    }

    #[tokio::test]
    async fn the_log_phase_lets_the_wall_through_and_emits_no_page_blocked() {
        let config = ChallengeConfig::from_values(None, Some("log"));
        assert_eq!(config.enforce, EnforceMode::Log);
        let _config = Installed::new(config);
        let (result, events) = scrape(ZdrMode::Off, 403, Some("text/html"), CF_INTERSTITIAL).await;
        assert_eq!(result.expect("log mode refuses nothing").status, 403);
        assert!(
            events.iter().all(|e| e.event_type != EventType::PageBlocked),
            "no page_blocked in log mode"
        );
    }

    #[tokio::test]
    async fn a_zdr_scrape_is_refused_the_same_and_the_error_has_no_host() {
        let _config = Installed::new(ChallengeConfig::enforcing());
        let (result, _events) = scrape(ZdrMode::On, 403, Some("text/html"), CF_INTERSTITIAL).await;
        let error = result.expect_err("refused under ZDR as well");
        assert_eq!(error.code, ErrorCode::ChallengeDetected);
        let wire = serde_json::to_string(&error).unwrap();
        assert!(!wire.contains("walled.example"), "{wire}");
        assert_eq!(error.details.unwrap()["block_reason"], "waf_interstitial");
    }

    #[test]
    fn classify_fetched_reads_at_most_the_scan_window_and_never_returns_a_hint() {
        let config = ChallengeConfig::enforcing();
        let signal = crate::challenge::classify_fetched(
            config,
            "https://walled.example.test/page",
            403,
            Some("Just a moment..."),
            CF_INTERSTITIAL.as_bytes(),
        )
        .expect("strong");
        assert!(signal.requires_escalation);
        // A wall past the scan window is not seen.
        let mut padded = vec![b' '; crate::challenge::MAX_SCANNED_BYTES + 10];
        padded.extend_from_slice(CF_INTERSTITIAL.as_bytes());
        assert!(crate::challenge::classify_fetched(config, "https://walled.example.test/page", 200, None, &padded).is_none());
        // A hint is not a verdict.
        assert!(crate::challenge::classify_fetched(
            config,
            "https://x.example/contact",
            200,
            Some("Contact us"),
            CONTACT_WITH_WIDGET.as_bytes()
        )
        .is_none());
        // Bytes that are not UTF-8 do not panic.
        assert!(crate::challenge::classify_fetched(config, "https://x.example/", 200, None, &[0xff, 0xfe, 0x00, 0xc3]).is_none());
    }
}

#[cfg(test)]
mod capture_pipeline_tests {
    use super::*;
    use crate::artifact_store::InMemoryStore;
    use crate::driver::Driver;
    use crate::fetch::FetchResponse;
    use async_trait::async_trait;
    use quarry_core::capture::{CapturedImage, CapturedPdf, ImageFormat};
    use quarry_core::output::DriverKind;
    use tokio::sync::mpsc;

    const PAGE: &str = "<html><head><title>Capture page</title></head><body><main><h1>Capture</h1><p>Some body text for the capture test page, long enough to be extracted as the main content of the page.</p></main></body></html>";

    /// A driver whose fetch carries the captures it is given.
    struct CaptureDriver {
        captures: crate::capture::PageCaptures,
    }

    #[async_trait]
    impl Driver for CaptureDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Browser
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
                body: PAGE.as_bytes().to_vec(),
                duration_ms: 3,
                served_by: DriverKind::Browser,
                captures: Some(Box::new(self.captures.clone())),
            })
        }
    }

    fn png(bytes: usize) -> CapturedImage {
        CapturedImage {
            bytes: vec![0x89; bytes],
            format: ImageFormat::Png,
            width_px: 1280,
            height_px: 720,
            tiles: 1,
            stitched: false,
            truncated: false,
        }
    }

    fn captures(screenshot: Option<CapturedImage>, pdf: Option<CapturedPdf>) -> crate::capture::PageCaptures {
        crate::capture::PageCaptures {
            screenshot,
            pdf,
            ..Default::default()
        }
    }

    fn runner(
        tx: mpsc::Sender<quarry_core::event::Event>,
        zdr: ZdrMode,
        driver: CaptureDriver,
        render: RenderHints,
    ) -> PageRunner {
        PageRunner {
            driver: Arc::new(driver),
            security: Arc::new(
                quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true),
            ),
            artifacts: Arc::new(InMemoryStore::new()),
            event_sink: EventSink::new(tx),
            zdr,
            ingest: None,
            org_id: Some("org_capture".into()),
            user_id: None,
            privacy: PrivacyPolicy::default(),
            cancel_token: None,
            local_index: None,
            policy: RunPolicy::default(),
            scheduler: None,
            autoscale: None,
            render,
            page_renderer: None,
            source_registrar: None,
            title_enricher: None,
            doc_routing: DocRouting::Off,
            robots: None,
        }
    }

    fn both_render() -> RenderHints {
        RenderHints {
            capture: Some(quarry_core::capture::CapturePlan {
                screenshot: Some(Default::default()),
                pdf: Some(Default::default()),
            }),
            ..RenderHints::default()
        }
    }

    #[tokio::test]
    async fn capture_refs_fill_output_formats() {
        let (tx, _rx) = mpsc::channel(64);
        let driver = CaptureDriver {
            captures: captures(
                Some(png(64)),
                Some(CapturedPdf {
                    bytes: b"%PDF-1.7 test".to_vec(),
                }),
            ),
        };
        let runner = runner(tx, ZdrMode::Off, driver, both_render());
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://www.example.com/".parse().unwrap();
        let out = runner.run(&run_id, &url, None).await.expect("run ok");

        let shot = out.formats.screenshot.expect("screenshot ref");
        assert_eq!(shot.bytes, 64);
        let pdf = out.formats.pdf.expect("pdf ref");
        assert_eq!(pdf.bytes, 13);
    }

    #[tokio::test]
    async fn zdr_capture_is_inline_and_writes_no_artifact_event_or_index() {
        let (tx, mut rx) = mpsc::channel(64);
        let driver = CaptureDriver {
            captures: captures(Some(png(64)), None),
        };
        let runner = runner(tx, ZdrMode::On, driver, both_render());
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://www.example.com/".parse().unwrap();
        let (out, inline) = runner
            .run_with_inline_request(&run_id, &url, None, &InlineRequest::default_for(ZdrMode::On))
            .await
            .expect("run ok");

        assert!(out.formats.screenshot.is_none(), "nothing is stored under ZDR");
        let shot = inline.screenshot.expect("inline screenshot");
        assert_eq!(shot.bytes, 64);
        assert_eq!(shot.content_type, "image/png");
        assert!(!shot.data_base64.is_empty());
        while let Ok(event) = rx.try_recv() {
            assert!(
                !matches!(event.event_type, EventType::ArtifactWritten),
                "no artifact event for a capture under ZDR"
            );
        }
    }

    #[tokio::test]
    async fn over_cap_zdr_capture_is_omitted_with_a_warning() {
        let (tx, _rx) = mpsc::channel(64);
        let driver = CaptureDriver {
            captures: captures(Some(png(64)), None),
        };
        let mut render = both_render();
        render.capture_inline_max_bytes = Some(16);
        let runner = runner(tx, ZdrMode::On, driver, render);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://www.example.com/".parse().unwrap();
        let (_out, inline) = runner
            .run_with_inline_request(&run_id, &url, None, &InlineRequest::default_for(ZdrMode::On))
            .await
            .expect("run ok");

        assert!(inline.screenshot.is_none());
        assert!(inline.omitted.iter().any(|field| field == "screenshot"));
        assert!(inline
            .warnings
            .iter()
            .any(|w| w.code == "capture_exceeds_inline_cap"));
    }

    // ---- WS13.5: what the scrape-time actions took ----

    use crate::artifact_store::ArtifactStore;
    use crate::capture::ActionOutputBytes;
    use quarry_core::scrape_actions::{ActionOutcome, ActionOutput, ActionStatus};

    fn plain_outcome(index: usize, kind: &str) -> ActionOutcome {
        ActionOutcome {
            index,
            kind: kind.to_owned(),
            status: ActionStatus::Ok,
            duration_ms: 4,
            error: None,
            output: None,
        }
    }

    fn capture_outcome(index: usize, kind: &str, content_type: &str, bytes: usize) -> ActionOutcome {
        ActionOutcome {
            output: Some(ActionOutput {
                kind: kind.to_owned(),
                content_type: content_type.to_owned(),
                bytes: bytes as u64,
                width: (kind == "screenshot").then_some(800),
                height: (kind == "screenshot").then_some(600),
            }),
            ..plain_outcome(index, kind)
        }
    }

    /// Captures as a browser fetch hands them over after `click, screenshot, wait, pdf`.
    fn action_captures(
        screenshot_bytes: usize,
        pdf_bytes: usize,
        final_screenshot: Option<CapturedImage>,
    ) -> crate::capture::PageCaptures {
        crate::capture::PageCaptures {
            screenshot: final_screenshot,
            actions: vec![
                plain_outcome(0, "click"),
                capture_outcome(1, "screenshot", "image/png", screenshot_bytes),
                plain_outcome(2, "wait"),
                capture_outcome(3, "pdf", "application/pdf", pdf_bytes),
            ],
            action_outputs: vec![
                None,
                Some(ActionOutputBytes {
                    content_type: "image/png".into(),
                    bytes: vec![0x89; screenshot_bytes],
                }),
                None,
                Some(ActionOutputBytes {
                    content_type: "application/pdf".into(),
                    bytes: vec![0x25; pdf_bytes],
                }),
            ],
            ..Default::default()
        }
    }

    fn actions_render() -> RenderHints {
        RenderHints {
            actions: Some(Arc::new(
                quarry_core::scrape_actions::validate_actions(
                    vec![
                        serde_json::json!({ "type": "click", "selector": "#go" }),
                        serde_json::json!({ "type": "screenshot" }),
                        serde_json::json!({ "type": "wait", "milliseconds": 10 }),
                        serde_json::json!({ "type": "pdf" }),
                    ],
                    quarry_core::scrape_actions::ActionOnError::Abort,
                    &quarry_core::scrape_actions::ActionLimits::default(),
                    &quarry_core::capture::CaptureLimits::default(),
                )
                .expect("the actions validate"),
            )),
            ..RenderHints::default()
        }
    }

    #[tokio::test]
    async fn action_outputs_are_stored_with_distinct_keys_and_org() {
        let (tx, mut rx) = mpsc::channel(64);
        let driver = CaptureDriver {
            captures: action_captures(48, 32, Some(png(64))),
        };
        let mut render = actions_render();
        render.capture = both_render().capture;
        let store = Arc::new(InMemoryStore::new());
        let mut runner = runner(tx, ZdrMode::Off, driver, render);
        runner.artifacts = store.clone();
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://www.example.com/".parse().unwrap();
        let (out, inline) = runner
            .run_with_inline_request(&run_id, &url, None, &InlineRequest::default_for(ZdrMode::Off))
            .await
            .expect("run ok");

        // Four entries, in request order, with the capture entries pointing at artifacts.
        let kinds: Vec<_> = inline.actions.iter().map(|a| (a.index, a.kind.as_str())).collect();
        assert_eq!(kinds, vec![(0, "click"), (1, "screenshot"), (2, "wait"), (3, "pdf")]);
        assert!(inline.actions[0].output.is_none() && inline.actions[2].output.is_none());
        let shot = inline.actions[1].output.as_ref().expect("screenshot output");
        let pdf = inline.actions[3].output.as_ref().expect("pdf output");
        let shot_id = shot.artifact_id.clone().expect("a stored screenshot has an id");
        let pdf_id = pdf.artifact_id.clone().expect("a stored pdf has an id");
        assert!(shot.data_base64.is_none() && pdf.data_base64.is_none(), "nothing inline when stored");
        assert_eq!((shot.bytes, pdf.bytes), (48, 32));
        assert_eq!((shot.width, shot.height), (Some(800), Some(600)));

        // Distinct keys: the two action outputs and the final screenshot are three artifacts.
        let final_shot = out.formats.screenshot.expect("the final screenshot is stored too");
        let ids = [shot_id.clone(), pdf_id.clone(), final_shot.artifact_id.to_string()];
        assert_eq!(
            ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
            3,
            "an action output never overwrites a final capture: {ids:?}"
        );

        // Org stamp: the request's org reads each back; another org reads nothing.
        for (id, expected) in [(&shot_id, 48usize), (&pdf_id, 32usize)] {
            let artifact: quarry_core::ids::kinds::ArtifactKind = id.parse().expect("an artifact id");
            let bytes = store.get("org_capture", &artifact).await.expect("same org reads it");
            assert_eq!(bytes.len(), expected);
            assert!(
                store.get("org_other", &artifact).await.is_err(),
                "another org must not read an action output"
            );
        }

        // One content-free ArtifactWritten per stored action output (and no bytes in it).
        let mut action_events = 0;
        while let Ok(event) = rx.try_recv() {
            if matches!(event.event_type, EventType::ArtifactWritten)
                && event.payload.get("action_index").is_some()
            {
                action_events += 1;
                assert!(event.payload.get("data_base64").is_none());
            }
        }
        assert_eq!(action_events, 2);
    }

    #[tokio::test]
    async fn action_outputs_inline_under_zdr_without_artifact_events() {
        let (tx, mut rx) = mpsc::channel(64);
        let driver = CaptureDriver {
            captures: action_captures(48, 32, None),
        };
        let store = Arc::new(InMemoryStore::new());
        let mut runner = runner(tx, ZdrMode::On, driver, actions_render());
        runner.artifacts = store.clone();
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://www.example.com/".parse().unwrap();
        let (_out, inline) = runner
            .run_with_inline_request(&run_id, &url, None, &InlineRequest::default_for(ZdrMode::On))
            .await
            .expect("run ok");

        let shot = inline.actions[1].output.as_ref().expect("screenshot output");
        let pdf = inline.actions[3].output.as_ref().expect("pdf output");
        assert!(shot.artifact_id.is_none() && pdf.artifact_id.is_none(), "no id under ZDR");
        assert!(shot.data_base64.as_deref().is_some_and(|data| !data.is_empty()));
        assert!(pdf.data_base64.as_deref().is_some_and(|data| !data.is_empty()));
        assert_eq!(shot.bytes, 48);
        // ZDR floor: nothing reached the store and no artifact event was emitted.
        let listed = store
            .list("org_capture", &quarry_core::pagination::ListFilter::default())
            .await
            .expect("list");
        assert!(listed.items.is_empty(), "ZDR writes no artifact for an action output");
        while let Ok(event) = rx.try_recv() {
            assert!(
                !matches!(event.event_type, EventType::ArtifactWritten),
                "no artifact event for an action output under ZDR"
            );
        }
        // No action-only captures block when nothing was captured at the end.
        assert!(inline.captures.is_none());
    }

    #[tokio::test]
    async fn zdr_action_output_over_the_shared_inline_cap_is_left_out_and_named() {
        let (tx, _rx) = mpsc::channel(64);
        // 48 + 32 = 80 bytes of action output against a 60-byte cap: the screenshot fits,
        // the pdf does not (the cap is one budget across the run, final captures included).
        let driver = CaptureDriver {
            captures: action_captures(48, 32, None),
        };
        let mut render = actions_render();
        render.capture_inline_max_bytes = Some(60);
        let runner = runner(tx, ZdrMode::On, driver, render);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://www.example.com/".parse().unwrap();
        let (_out, inline) = runner
            .run_with_inline_request(&run_id, &url, None, &InlineRequest::default_for(ZdrMode::On))
            .await
            .expect("run ok");

        assert!(inline.actions[1].output.as_ref().unwrap().data_base64.is_some());
        let pdf = inline.actions[3].output.as_ref().unwrap();
        assert!(pdf.data_base64.is_none() && pdf.artifact_id.is_none());
        assert_eq!(pdf.bytes, 32, "the size is still reported");
        assert!(inline.omitted.iter().any(|field| field == "actions[3].output"));
        assert!(inline.warnings.iter().any(|w| w.code == "capture_exceeds_inline_cap"));
    }

    #[tokio::test]
    async fn a_run_without_actions_reports_no_actions() {
        let (tx, _rx) = mpsc::channel(64);
        let driver = CaptureDriver {
            captures: captures(Some(png(64)), None),
        };
        let runner = runner(tx, ZdrMode::Off, driver, both_render());
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://www.example.com/".parse().unwrap();
        let (_out, inline) = runner
            .run_with_inline_request(&run_id, &url, None, &InlineRequest::default_for(ZdrMode::Off))
            .await
            .expect("run ok");
        assert!(inline.actions.is_empty());
        assert!(inline.captures.is_some(), "a final capture still reports its measurements");
    }
}

#[cfg(test)]
mod final_url_recheck_tests {
    //! WS7.G: a redirect to a host the policy denies ends the run before anything is
    //! stored, and a final URL equal to the requested one is asked about once.
    use super::*;
    use crate::artifact_store::{ArtifactStore, InMemoryStore};
    use crate::driver::Driver;
    use crate::fetch::FetchResponse;
    use crate::policy_gate::{PolicySecurityEngine, BLOCK_PREFIX};
    use async_trait::async_trait;
    use quarry_core::output::DriverKind;
    use quarry_core::web_policy::{resolve, OrgInput, PolicyInputs, PolicyLayer, Principal};
    use quarry_security::{SecurityEngine, Verdict};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::mpsc;

    const PAGE: &str = "<html><head><title>Landing</title></head><body><main><h1>Landing page</h1><p>Some body text that is long enough to be a page worth keeping in a store.</p></main></body></html>";

    /// Answers every fetch with `PAGE` served from `final_host` (a redirect when it
    /// differs from the requested host).
    struct RedirectDriver {
        final_host: &'static str,
    }

    #[async_trait]
    impl Driver for RedirectDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            let mut final_url = url.clone();
            final_url.set_host(Some(self.final_host)).unwrap();
            Ok(FetchResponse {
                status: 200,
                final_url,
                headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
                body: PAGE.as_bytes().to_vec(),
                duration_ms: 1,
                served_by: DriverKind::Static,
                captures: None,
            })
        }
    }

    /// Counts the preflights it is asked and allows them all.
    #[derive(Default)]
    struct CountingEngine {
        preflights: AtomicUsize,
    }

    #[async_trait]
    impl SecurityEngine for CountingEngine {
        async fn preflight(&self, _url: &Url) -> Verdict {
            self.preflights.fetch_add(1, Ordering::SeqCst);
            Verdict::allow()
        }
        async fn check_discovered(&self, _parent: &Url, _child: &Url) -> Verdict {
            Verdict::allow()
        }
        fn allow_private_hosts(&self) -> bool {
            true
        }
    }

    fn deny_policy(host: &str) -> quarry_core::web_policy::ResolvedPolicy {
        let layer = PolicyLayer::new(BTreeMap::new(), None, None, None, vec![host.to_string()])
            .expect("a valid layer");
        resolve(&PolicyInputs {
            principal: Principal::Human,
            zdr: false,
            baseline: PolicyLayer::default(),
            org: OrgInput::Absent,
            grant: None,
            request: Some(layer),
        })
        .expect("the policy resolves")
    }

    fn runner(
        tx: mpsc::Sender<quarry_core::event::Event>,
        driver: RedirectDriver,
        security: Arc<dyn SecurityEngine>,
        artifacts: Arc<InMemoryStore>,
    ) -> PageRunner {
        PageRunner {
            driver: Arc::new(driver),
            security,
            artifacts,
            event_sink: EventSink::new(tx),
            zdr: ZdrMode::Off,
            ingest: None,
            org_id: Some("org_test".into()),
            user_id: None,
            privacy: PrivacyPolicy::default(),
            cancel_token: None,
            local_index: None,
            policy: RunPolicy::default(),
            scheduler: None,
            autoscale: None,
            render: RenderHints::default(),
            page_renderer: None,
            source_registrar: None,
            title_enricher: None,
            doc_routing: DocRouting::Off,
            robots: None,
        }
    }

    #[tokio::test]
    async fn redirect_to_a_denied_host_returns_security_blocked_and_persists_nothing() {
        let (tx, mut rx) = mpsc::channel(64);
        let artifacts = Arc::new(InMemoryStore::new());
        let engine = PolicySecurityEngine::new(
            Arc::new(CountingEngine::default()),
            deny_policy("denied.example"),
        )
        .with_final_url_check(true);
        let runner = runner(
            tx,
            RedirectDriver {
                final_host: "denied.example",
            },
            Arc::new(engine),
            artifacts.clone(),
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://start.example/page".parse().unwrap();

        let error = runner.run(&run_id, &url, None).await.expect_err("the redirect is refused");
        assert_eq!(error.code, quarry_core::error::ErrorCode::SecurityBlocked);
        assert!(error.message.starts_with(BLOCK_PREFIX), "{}", error.message);

        assert_eq!(
            artifacts.count("org_test").await.unwrap(),
            Some(0),
            "nothing is written to the artifact store"
        );
        let mut types = Vec::new();
        while let Ok(event) = rx.try_recv() {
            types.push(event.event_type);
        }
        assert!(types.contains(&EventType::PageBlocked), "{types:?}");
        assert!(
            !types.contains(&EventType::PageFetched) && !types.contains(&EventType::ArtifactWritten),
            "no fetched page and no artifact event: {types:?}"
        );
    }

    #[tokio::test]
    async fn final_url_equal_to_requested_is_not_preflighted_twice() {
        let (tx, _rx) = mpsc::channel(64);
        let inner = Arc::new(CountingEngine::default());
        let engine = PolicySecurityEngine::new(inner.clone(), deny_policy("denied.example"))
            .with_final_url_check(true);
        let runner = runner(
            tx,
            RedirectDriver {
                final_host: "start.example",
            },
            Arc::new(engine),
            Arc::new(InMemoryStore::new()),
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://start.example/page".parse().unwrap();
        runner.run(&run_id, &url, None).await.expect("an allowed page runs");
        assert_eq!(inner.preflights.load(Ordering::SeqCst), 1, "one preflight, for the requested URL");
    }

    #[tokio::test]
    async fn a_redirect_to_an_allowed_host_is_preflighted_once_more() {
        let (tx, _rx) = mpsc::channel(64);
        let inner = Arc::new(CountingEngine::default());
        let engine = PolicySecurityEngine::new(inner.clone(), deny_policy("denied.example"))
            .with_final_url_check(true);
        let runner = runner(
            tx,
            RedirectDriver {
                final_host: "www.start.example",
            },
            Arc::new(engine),
            Arc::new(InMemoryStore::new()),
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://start.example/page".parse().unwrap();
        runner.run(&run_id, &url, None).await.expect("a redirect within the allowed hosts runs");
        assert_eq!(inner.preflights.load(Ordering::SeqCst), 2, "requested URL, then the final URL");
    }

    #[tokio::test]
    async fn with_the_flag_off_the_final_url_is_not_rechecked() {
        let (tx, _rx) = mpsc::channel(64);
        let inner = Arc::new(CountingEngine::default());
        let engine = PolicySecurityEngine::new(inner.clone(), deny_policy("denied.example"));
        let runner = runner(
            tx,
            RedirectDriver {
                final_host: "www.start.example",
            },
            Arc::new(engine),
            Arc::new(InMemoryStore::new()),
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://start.example/page".parse().unwrap();
        runner.run(&run_id, &url, None).await.expect("runs as before");
        assert_eq!(inner.preflights.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod robots_gate_tests {
    //! WS14.S4: the robots.txt gate inside `PageRunner::run_inner`.
    use super::*;
    use crate::artifact_store::InMemoryStore;
    use crate::fetch::FetchResponse;
    use crate::robots_cache::{RobotsFetch, RobotsFetcher};
    use crate::robots_gate::{RobotsGate, RobotsGateConfig};
    use async_trait::async_trait;
    use quarry_core::output::DriverKind;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::mpsc;

    const PAGE: &str = "<html><head><title>Docs</title></head><body><main><h1>Docs</h1><p>Some readable body text for the page, long enough to extract.</p></main></body></html>";

    /// Serves one canned page and counts how often it was asked. `redirect_to`
    /// makes the answer come from another URL, as after a redirect.
    struct PageDriver {
        calls: Arc<AtomicUsize>,
        redirect_to: Option<Url>,
    }

    #[async_trait]
    impl Driver for PageDriver {
        fn kind(&self) -> quarry_core::output::DriverKind {
            DriverKind::Static
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<crate::fetch::FetchResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(FetchResponse::new(
                200,
                self.redirect_to.clone().unwrap_or_else(|| url.clone()),
                vec![("content-type".into(), "text/html; charset=utf-8".into())],
                PAGE.as_bytes().to_vec(),
                2,
                DriverKind::Static,
            ))
        }
    }

    /// Answers robots.txt from a map keyed by origin key; an origin not in the
    /// map has no robots.txt.
    struct MapFetcher(HashMap<String, RobotsFetch>);

    #[async_trait]
    impl RobotsFetcher for MapFetcher {
        async fn fetch(&self, origin: &str, _org_id: &str) -> RobotsFetch {
            self.0.get(origin).cloned().unwrap_or(RobotsFetch::Absent)
        }
    }

    fn gate(rules: &[(&str, &str)]) -> Arc<RobotsGate> {
        let map = rules
            .iter()
            .map(|(origin, body)| ((*origin).to_string(), RobotsFetch::Found((*body).to_string())))
            .collect();
        Arc::new(RobotsGate::with_fetcher(
            Arc::new(MapFetcher(map)),
            RobotsGateConfig::default(),
        ))
    }

    struct Harness {
        runner: PageRunner,
        rx: mpsc::Receiver<quarry_core::event::Event>,
        calls: Arc<AtomicUsize>,
        gate: Arc<RobotsGate>,
    }

    fn harness(
        gate: Arc<RobotsGate>,
        mode: Option<RobotsEnforceMode>,
        scheduler: Option<Arc<HostScheduler>>,
        redirect_to: Option<&str>,
    ) -> Harness {
        let (tx, rx) = mpsc::channel(64);
        let calls = Arc::new(AtomicUsize::new(0));
        let runner = PageRunner {
            driver: Arc::new(PageDriver {
                calls: calls.clone(),
                redirect_to: redirect_to.map(|u| u.parse().unwrap()),
            }),
            security: Arc::new(
                quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true),
            ),
            artifacts: Arc::new(InMemoryStore::new()),
            event_sink: EventSink::new(tx),
            zdr: ZdrMode::Off,
            ingest: None,
            org_id: Some("org_test".into()),
            user_id: None,
            privacy: PrivacyPolicy::default(),
            cancel_token: None,
            local_index: None,
            policy: RunPolicy::default(),
            scheduler,
            autoscale: None,
            render: RenderHints::default(),
            page_renderer: None,
            source_registrar: None,
            title_enricher: None,
            doc_routing: DocRouting::Off,
            robots: mode.map(|mode| RobotsEnforcement::new(gate.clone(), mode)),
        };
        Harness {
            runner,
            rx,
            calls,
            gate,
        }
    }

    fn drain(rx: &mut mpsc::Receiver<quarry_core::event::Event>) -> Vec<quarry_core::event::Event> {
        let mut out = Vec::new();
        while let Ok(event) = rx.try_recv() {
            out.push(event);
        }
        out
    }

    fn count(events: &[quarry_core::event::Event], kind: EventType) -> usize {
        events.iter().filter(|e| e.event_type == kind).count()
    }

    const PRIVATE: &str = "User-agent: *\nDisallow: /private/\nCrawl-delay: 3\n";
    const ORIGIN: &str = "https://example.test:443";

    #[tokio::test]
    async fn a_disallowed_url_never_reaches_the_driver_and_emits_one_page_blocked() {
        let mut h = harness(gate(&[(ORIGIN, PRIVATE)]), Some(RobotsEnforceMode::Enforce), None, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/private/secret".parse().unwrap();
        let err = h.runner.run(&run_id, &url, None).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
        let denial = &err.details.as_ref().unwrap()["denial"];
        assert_eq!(denial["code"], "robots_disallowed");
        assert_eq!(denial["path"], "/private/secret");
        assert!(denial["user_agent"].is_string());
        assert_eq!(h.calls.load(Ordering::SeqCst), 0, "the page driver was never called");
        let events = drain(&mut h.rx);
        assert_eq!(count(&events, EventType::PageBlocked), 1, "exactly one PageBlocked");
        assert_eq!(count(&events, EventType::PageFailed), 0, "no PageFailed on top of it");
        let blocked = events.iter().find(|e| e.event_type == EventType::PageBlocked).unwrap();
        assert_eq!(blocked.payload["code"], "robots_disallowed");
        assert_eq!(blocked.payload["path"], "/private/secret");
        assert_eq!(blocked.payload["source"], "fetch");
    }

    #[tokio::test]
    async fn a_robots_denial_does_not_shrink_the_hosts_concurrency() {
        let scheduler = Arc::new(HostScheduler::with_defaults());
        drop(scheduler.acquire("example.test").await);
        let before = scheduler.stats("example.test").await.unwrap().target_concurrency;
        let h = harness(
            gate(&[(ORIGIN, PRIVATE)]),
            Some(RobotsEnforceMode::Enforce),
            Some(scheduler.clone()),
            None,
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/private/secret".parse().unwrap();
        h.runner.run(&run_id, &url, None).await.unwrap_err();
        let stats = scheduler.stats("example.test").await.unwrap();
        assert_eq!(stats.target_concurrency, before);
        assert!(stats.last_failure_at.is_none(), "record_bad was not called");
    }

    #[tokio::test]
    async fn observe_fetches_and_counts_would_deny() {
        let mut h = harness(gate(&[(ORIGIN, PRIVATE)]), Some(RobotsEnforceMode::Observe), None, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/private/secret".parse().unwrap();
        h.runner.run(&run_id, &url, None).await.expect("observe never blocks");
        assert_eq!(h.calls.load(Ordering::SeqCst), 1);
        assert_eq!(h.gate.stats().would_deny, 1);
        let events = drain(&mut h.rx);
        assert_eq!(count(&events, EventType::PageBlocked), 0);
    }

    #[tokio::test]
    async fn a_run_without_a_gate_fetches() {
        let h = harness(gate(&[(ORIGIN, PRIVATE)]), None, None, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/private/secret".parse().unwrap();
        h.runner.run(&run_id, &url, None).await.expect("no gate, no check");
        assert_eq!(h.calls.load(Ordering::SeqCst), 1);
        assert!(h.gate.is_empty(), "robots.txt was not even fetched");
    }

    #[tokio::test]
    async fn off_does_not_evaluate() {
        let h = harness(gate(&[(ORIGIN, PRIVATE)]), Some(RobotsEnforceMode::Off), None, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/private/secret".parse().unwrap();
        h.runner.run(&run_id, &url, None).await.expect("off fetches");
        assert!(h.gate.is_empty());
    }

    #[tokio::test]
    async fn enforce_feeds_crawl_delay_to_the_scheduler_and_observe_does_not() {
        let url: Url = "https://example.test/docs/a".parse().unwrap();
        let run_id: RunKind = quarry_core::ids::Id::new();

        let scheduler = Arc::new(HostScheduler::with_defaults());
        let h = harness(
            gate(&[(ORIGIN, PRIVATE)]),
            Some(RobotsEnforceMode::Enforce),
            Some(scheduler.clone()),
            None,
        );
        h.runner.run(&run_id, &url, None).await.expect("allowed path fetches");
        assert_eq!(
            scheduler.min_interval("example.test").await,
            std::time::Duration::from_secs(3)
        );

        let scheduler = Arc::new(HostScheduler::with_defaults());
        let h = harness(
            gate(&[(ORIGIN, PRIVATE)]),
            Some(RobotsEnforceMode::Observe),
            Some(scheduler.clone()),
            None,
        );
        h.runner.run(&run_id, &url, None).await.expect("observe fetches");
        assert_eq!(
            scheduler.min_interval("example.test").await,
            std::time::Duration::ZERO,
            "observe changes no timing"
        );
    }

    #[tokio::test]
    async fn a_redirect_to_a_disallowed_origin_persists_nothing() {
        let rules = [("https://other.test:443", "User-agent: *\nDisallow: /\n")];
        let mut h = harness(
            gate(&rules),
            Some(RobotsEnforceMode::Enforce),
            None,
            Some("https://other.test/landing"),
        );
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/start".parse().unwrap();
        let err = h.runner.run(&run_id, &url, None).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
        assert_eq!(h.calls.load(Ordering::SeqCst), 1, "the page was fetched once");
        let events = drain(&mut h.rx);
        assert_eq!(count(&events, EventType::PageBlocked), 1);
        assert_eq!(count(&events, EventType::PageFetched), 0);
        assert_eq!(count(&events, EventType::ArtifactWritten), 0);
        assert_eq!(count(&events, EventType::PageFailed), 0);
        let blocked = events.iter().find(|e| e.event_type == EventType::PageBlocked).unwrap();
        assert_eq!(blocked.payload["stage"], "final_url");
    }

    #[tokio::test]
    async fn an_unreachable_robots_txt_blocks_in_enforce_and_names_the_host() {
        let unreachable: HashMap<String, RobotsFetch> =
            [(ORIGIN.to_string(), RobotsFetch::Unreachable("status_5xx".into()))].into();
        let gate = Arc::new(RobotsGate::with_fetcher(
            Arc::new(MapFetcher(unreachable)),
            RobotsGateConfig::default(),
        ));
        let h = harness(gate, Some(RobotsEnforceMode::Enforce), None, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/docs/a".parse().unwrap();
        let err = h.runner.run(&run_id, &url, None).await.unwrap_err();
        let denial = &err.details.as_ref().unwrap()["denial"];
        assert_eq!(denial["code"], "robots_unreachable");
        assert_eq!(denial["host"], "example.test");
        assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn an_enforcing_run_under_zdr_leaves_no_cache_entry() {
        let (tx, _rx) = mpsc::channel(64);
        let gate = gate(&[(ORIGIN, PRIVATE)]);
        let mut h = harness(gate.clone(), Some(RobotsEnforceMode::Enforce), None, None);
        h.runner.zdr = ZdrMode::On;
        h.runner.event_sink = EventSink::new(tx);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/private/secret".parse().unwrap();
        h.runner.run(&run_id, &url, None).await.unwrap_err();
        assert!(gate.is_empty(), "ZDR reads and fills no robots cache");
    }

    #[tokio::test]
    async fn a_cached_page_is_refused_before_it_is_served_under_enforce() {
        let mut h = harness(gate(&[(ORIGIN, PRIVATE)]), Some(RobotsEnforceMode::Enforce), None, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/private/secret".parse().unwrap();
        let err = h.runner.precheck_robots(&run_id, &url).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
        let denial = &err.details.as_ref().unwrap()["denial"];
        assert_eq!(denial["code"], "robots_disallowed");
        assert_eq!(denial["path"], "/private/secret");
        assert_eq!(h.calls.load(Ordering::SeqCst), 0, "the page driver is not asked");
        assert_eq!(h.gate.stats().denied, 1);
        let events = drain(&mut h.rx);
        assert_eq!(count(&events, EventType::PageBlocked), 1, "exactly one PageBlocked");
        let blocked = events.iter().find(|e| e.event_type == EventType::PageBlocked).unwrap();
        assert_eq!(blocked.payload["stage"], "request");
    }

    #[tokio::test]
    async fn a_cached_page_under_observe_is_counted_once_and_not_refused() {
        let mut h = harness(gate(&[(ORIGIN, PRIVATE)]), Some(RobotsEnforceMode::Observe), None, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/private/secret".parse().unwrap();
        h.runner.precheck_robots(&run_id, &url).await.expect("observe never refuses");
        let stats = h.gate.stats();
        assert_eq!(stats.would_deny, 1);
        assert_eq!(stats.total(), 1, "one decision for the cached page");
        assert_eq!(h.calls.load(Ordering::SeqCst), 0, "the precheck fetches nothing");
        assert!(drain(&mut h.rx).is_empty(), "observe emits no PageBlocked");
    }

    #[tokio::test]
    async fn a_fetched_page_gets_one_decision_not_two() {
        let h = harness(gate(&[(ORIGIN, PRIVATE)]), Some(RobotsEnforceMode::Observe), None, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/private/secret".parse().unwrap();
        h.runner.run(&run_id, &url, None).await.expect("observe fetches");
        assert_eq!(h.gate.stats().total(), 1);
        assert_eq!(h.gate.stats().would_deny, 1);
    }

    #[tokio::test]
    async fn an_unreachable_robots_txt_under_observe_is_counted_as_would_unreachable() {
        let unreachable: HashMap<String, RobotsFetch> =
            [(ORIGIN.to_string(), RobotsFetch::Unreachable("status_5xx".into()))].into();
        let gate = Arc::new(RobotsGate::with_fetcher(
            Arc::new(MapFetcher(unreachable)),
            RobotsGateConfig::default(),
        ));
        let h = harness(gate, Some(RobotsEnforceMode::Observe), None, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/docs/a".parse().unwrap();
        h.runner.run(&run_id, &url, None).await.expect("observe fetches");
        assert_eq!(h.calls.load(Ordering::SeqCst), 1);
        let stats = h.gate.stats();
        assert_eq!(stats.would_deny_unreachable, 1);
        assert_eq!(stats.would_deny, 0);
        assert_eq!(stats.denied_unreachable, 0);
        assert_eq!(stats.total(), 1);
    }

    #[tokio::test]
    async fn an_unreachable_cached_page_is_refused_under_enforce_before_it_is_served() {
        let unreachable: HashMap<String, RobotsFetch> =
            [(ORIGIN.to_string(), RobotsFetch::Unreachable("status_5xx".into()))].into();
        let gate = Arc::new(RobotsGate::with_fetcher(
            Arc::new(MapFetcher(unreachable)),
            RobotsGateConfig::default(),
        ));
        let h = harness(gate, Some(RobotsEnforceMode::Enforce), None, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let url: Url = "https://example.test/docs/a".parse().unwrap();
        let err = h.runner.precheck_robots(&run_id, &url).await.unwrap_err();
        let denial = &err.details.as_ref().unwrap()["denial"];
        assert_eq!(denial["code"], "robots_unreachable");
        assert_eq!(denial["host"], "example.test");
        assert_eq!(h.gate.stats().denied_unreachable, 1);
        assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    }

    /// Records each fetch: the host, the time since the test began and the global permits still
    /// free. A request holds its global permit from its fetch onwards.
    struct ProbeDriver {
        pool: Arc<AutoscaledPool>,
        started: tokio::time::Instant,
        fetches: Arc<std::sync::Mutex<Vec<(String, std::time::Duration, usize)>>>,
    }

    #[async_trait]
    impl Driver for ProbeDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }

        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            self.fetches.lock().unwrap().push((
                url.host_str().unwrap_or_default().to_owned(),
                self.started.elapsed(),
                self.pool.available(),
            ));
            Ok(FetchResponse::new(
                200,
                url.clone(),
                vec![("content-type".into(), "text/html; charset=utf-8".into())],
                PAGE.as_bytes().to_vec(),
                2,
                DriverKind::Static,
            ))
        }
    }

    /// D2-161: a request waiting out its host's Crawl-delay holds no global autoscale permit, so
    /// another host is fetched meanwhile, and the delayed request is fetched at its start time.
    #[tokio::test(start_paused = true)]
    async fn a_crawl_delay_wait_holds_no_global_permit() {
        let pool = Arc::new(AutoscaledPool::new(1, 1, 1));
        let scheduler = Arc::new(HostScheduler::with_defaults());
        scheduler
            .set_min_interval("example.test", std::time::Duration::from_secs(30))
            .await;
        // The host's first start is reserved now, so its next request may start 30 s from now.
        let _first_start = scheduler.reserve_start("example.test").await;
        let fetches = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut h = harness(
            gate(&[(ORIGIN, PRIVATE)]),
            None,
            Some(scheduler.clone()),
            None,
        );
        let probe: Arc<dyn Driver> = Arc::new(ProbeDriver {
            pool: pool.clone(),
            started: tokio::time::Instant::now(),
            fetches: fetches.clone(),
        });
        h.runner.driver = probe;
        h.runner.autoscale = Some(pool.clone());
        let runner = h.runner;
        let run_id: RunKind = quarry_core::ids::Id::new();
        let delayed_url: Url = "https://example.test/docs/a".parse().unwrap();
        let other_url: Url = "https://other.test/docs/a".parse().unwrap();
        let delayed = runner.run(&run_id, &delayed_url, None);
        let other = runner.run(&run_id, &other_url, None);
        let check = async {
            // Ten seconds in, the delayed request is still waiting and holds no global permit.
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            assert_eq!(
                pool.available(),
                1,
                "the waiting request holds no global permit"
            );
            assert!(
                fetches
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|(host, _, _)| host != "example.test"),
                "the delayed request is not fetched before its start time"
            );
        };
        let (delayed, other, ()) = tokio::join!(delayed, other, check);
        other.expect("the other host is fetched while the delayed request waits");
        delayed.expect("the delayed host is fetched at its start time");

        let fetches = fetches.lock().unwrap().clone();
        let fetch_of = |host: &str| {
            fetches
                .iter()
                .find(|(seen, _, _)| seen == host)
                .cloned()
                .unwrap_or_else(|| panic!("no fetch of {host}"))
        };
        let (_, other_at, other_free) = fetch_of("other.test");
        assert!(
            other_at < std::time::Duration::from_secs(30),
            "the other host is not held up"
        );
        assert_eq!(other_free, 0, "the fetch holds the one global permit");
        let (_, delayed_at, delayed_free) = fetch_of("example.test");
        assert!(
            delayed_at >= std::time::Duration::from_secs(30),
            "the delay is honoured"
        );
        assert_eq!(delayed_free, 0, "the fetch holds the one global permit");
        assert_eq!(pool.available(), 1, "every global permit is back");
    }
}

#[cfg(test)]
mod pii_redaction_tests {
    //! E3.6-4a: the fail-closed PII redaction stage.
    use super::*;
    use crate::artifact_store::InMemoryStore;
    use crate::driver::Driver;
    use crate::fetch::FetchResponse;
    use async_trait::async_trait;
    use quarry_core::capture::{CapturedImage, ImageFormat};
    use quarry_core::output::DriverKind;
    use tokio::sync::mpsc;

    const EMAIL: &str = "ola.nordmann@example.no";
    const MARKER: &str = "[redacted:pii.email]";
    const ORG: &str = "org_test";

    const PAGE: &str = "<html lang=\"no\"><head><title>Kontakt ola.nordmann@example.no</title><meta name=\"description\" content=\"Skriv til ola.nordmann@example.no\"></head><body><main><h1>Kontakt oss</h1><p>Vi leverer rengjøringsløsninger, kjemikalier og kompetanse til næringsmiddelindustrien i hele Norden. Skriv til ola.nordmann@example.no for et tilbud, så svarer vi innen en arbeidsdag.</p><p><a href=\"mailto:ola.nordmann@example.no\">ola.nordmann@example.no</a></p></main></body></html>";

    /// Serves [`PAGE`], with the captures it is given.
    struct PageDriver {
        captures: Option<crate::capture::PageCaptures>,
    }

    #[async_trait]
    impl Driver for PageDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Browser
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
                body: PAGE.as_bytes().to_vec(),
                duration_ms: 3,
                served_by: DriverKind::Browser,
                captures: self.captures.clone().map(Box::new),
            })
        }
    }

    /// Installs settings for this thread and removes them when dropped.
    struct SettingsGuard;

    impl Drop for SettingsGuard {
        fn drop(&mut self) {
            TEST_PII_REDACTION.with(|slot| *slot.borrow_mut() = None);
        }
    }

    fn install(settings: PiiRedactionSettings) -> SettingsGuard {
        TEST_PII_REDACTION.with(|slot| *slot.borrow_mut() = Some(settings));
        SettingsGuard
    }

    fn on(org_default: PiiOrgDefault) -> PiiRedactionSettings {
        PiiRedactionSettings {
            mode: PiiRedactionMode::On,
            org_default,
        }
    }

    fn runner(
        tx: mpsc::Sender<quarry_core::event::Event>,
        zdr: ZdrMode,
        captures: Option<crate::capture::PageCaptures>,
    ) -> PageRunner {
        PageRunner {
            driver: Arc::new(PageDriver { captures }),
            security: Arc::new(
                quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true),
            ),
            artifacts: Arc::new(InMemoryStore::new()),
            event_sink: EventSink::new(tx),
            zdr,
            ingest: None,
            org_id: Some(ORG.into()),
            user_id: None,
            privacy: PrivacyPolicy::default(),
            cancel_token: None,
            local_index: None,
            policy: RunPolicy::default(),
            scheduler: None,
            autoscale: None,
            render: RenderHints::default(),
            page_renderer: None,
            source_registrar: None,
            title_enricher: None,
            doc_routing: DocRouting::Off,
            robots: None,
        }
    }

    fn url() -> Url {
        "https://www.example.com/kontakt".parse().unwrap()
    }

    fn drain(rx: &mut mpsc::Receiver<quarry_core::event::Event>) -> Vec<quarry_core::event::Event> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    fn all_text_inline() -> InlineRequest {
        InlineRequest {
            markdown: true,
            html: true,
            raw_html: true,
            ..InlineRequest::default_for(ZdrMode::Off)
        }
    }

    async fn stored(runner: &PageRunner, format: Option<&FormatRef>) -> String {
        let id = format.expect("an artifact").artifact_id.clone();
        String::from_utf8(runner.artifacts.get(ORG, &id).await.unwrap()).unwrap()
    }

    fn png() -> CapturedImage {
        CapturedImage {
            bytes: vec![0x89; 64],
            format: ImageFormat::Png,
            width_px: 1280,
            height_px: 720,
            tiles: 1,
            stitched: false,
            truncated: false,
        }
    }

    // ---- settings

    #[test]
    fn the_stage_is_off_unless_the_flag_says_on() {
        assert_eq!(PiiRedactionSettings::parse(None, None), PiiRedactionSettings::default());
        assert_eq!(PiiRedactionSettings::parse(None, None).mode, PiiRedactionMode::Off);
        assert_eq!(PiiRedactionSettings::parse(Some("off"), None).mode, PiiRedactionMode::Off);
        assert_eq!(PiiRedactionSettings::parse(Some(""), None).mode, PiiRedactionMode::Off);
        assert_eq!(PiiRedactionSettings::parse(Some("on"), None).mode, PiiRedactionMode::On);
        assert_eq!(PiiRedactionSettings::parse(Some(" TRUE "), None).mode, PiiRedactionMode::On);
        assert!(PiiRedactionMode::parse("maybe").unwrap_err().contains(PII_REDACTION_ENV));
        assert_eq!(
            PiiRedactionSettings::parse(Some("maybe"), None).mode,
            PiiRedactionMode::On,
            "an unreadable mode fails closed"
        );
    }

    #[test]
    fn the_org_default_is_a_comma_list_or_every_org() {
        assert_eq!(PiiOrgDefault::parse(" , "), PiiOrgDefault::None);
        assert_eq!(
            PiiOrgDefault::parse("org_a, org_b"),
            PiiOrgDefault::Orgs(vec!["org_a".into(), "org_b".into()])
        );
        assert_eq!(PiiOrgDefault::parse("org_a,*"), PiiOrgDefault::All);
        assert_eq!(
            PiiRedactionSettings::parse(Some("on"), Some("org_a")).org_default,
            PiiOrgDefault::Orgs(vec!["org_a".into()])
        );
    }

    #[test]
    fn a_run_redacts_when_the_stage_is_on_and_it_asks_or_its_org_default_is_on() {
        let off = PiiRedactionSettings {
            mode: PiiRedactionMode::Off,
            org_default: PiiOrgDefault::All,
        };
        assert!(!off.applies(true, Some("org_a")), "off never redacts");

        let asked_only = on(PiiOrgDefault::None);
        assert!(asked_only.applies(true, Some("org_a")));
        assert!(!asked_only.applies(false, Some("org_a")));

        let listed = on(PiiOrgDefault::Orgs(vec!["org_a".into()]));
        assert!(listed.applies(false, Some("org_a")));
        assert!(!listed.applies(false, Some("org_b")));
        assert!(!listed.applies(false, None), "no org is not a listed org");
        assert!(!listed.applies(false, Some("")));

        assert!(on(PiiOrgDefault::All).applies(false, None));
    }

    // ---- the redactor

    #[test]
    fn text_is_redacted_whole_or_refused() {
        assert_eq!(
            redact_text_fail_closed(&format!("Skriv til {EMAIL} i dag")).unwrap(),
            format!("Skriv til {MARKER} i dag")
        );
        assert_eq!(redact_text_fail_closed("ingenting her").unwrap(), "ingenting her");
        let over = "a".repeat(PII_REDACT_MAX_BYTES + 1);
        assert_eq!(redact_text_fail_closed(&over), Err(PiiRedactionFailure::TooLarge));
    }

    #[test]
    fn every_string_of_a_structured_field_is_redacted() {
        let meta = quarry_core::output::PageMetadata {
            title: Some(format!("Kontakt {EMAIL}")),
            description: Some(format!("Skriv til {EMAIL}")),
            ..Default::default()
        };
        let redacted = redact_serde(&meta).unwrap();
        assert_eq!(redacted.title.as_deref(), Some("Kontakt [redacted:pii.email]"));
        assert_eq!(redacted.description.as_deref(), Some("Skriv til [redacted:pii.email]"));
    }

    #[test]
    fn the_failure_is_typed_final_and_carries_nothing_of_the_page() {
        let error = pii_redaction_error(PiiRedactionFailure::ResidualPii, "markdown");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert!(!error.retryable);
        assert!(is_pii_redaction_failure(&error));
        let details = error.details.clone().unwrap();
        assert_eq!(details["reason"], PII_REDACTION_FAILED);
        assert_eq!(details["cause"], "residual_pii");
        assert_eq!(details["field"], "markdown");
        assert!(!is_pii_redaction_failure(&QuarryError::new(ErrorCode::Forbidden, "x")));
    }

    #[test]
    fn a_capture_is_not_redactable() {
        assert_eq!(unredactable_capture(None), None);
        let empty = crate::capture::PageCaptures::default();
        assert_eq!(unredactable_capture(Some(&empty)), None);
        let shot = crate::capture::PageCaptures {
            screenshot: Some(png()),
            ..Default::default()
        };
        assert_eq!(unredactable_capture(Some(&shot)), Some("screenshot"));
    }

    // ---- the pipeline

    #[tokio::test]
    async fn with_the_flag_off_a_run_that_asks_is_unchanged() {
        let _settings = install(PiiRedactionSettings::default());
        let (tx, _rx) = mpsc::channel(64);
        let runner = runner(tx, ZdrMode::Off, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (out, inline) = runner
            .run_with_inline_request_redacting(&run_id, &url(), None, &all_text_inline(), true)
            .await
            .expect("run ok");
        assert!(stored(&runner, out.formats.markdown.as_ref()).await.contains(EMAIL));
        assert!(inline.markdown.unwrap().contains(EMAIL));
    }

    #[tokio::test]
    async fn a_redacting_run_keeps_emits_and_returns_no_email() {
        let _settings = install(on(PiiOrgDefault::None));
        let (tx, mut rx) = mpsc::channel(64);
        let runner = runner(tx, ZdrMode::Off, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (out, inline) = runner
            .run_with_inline_request_redacting(&run_id, &url(), None, &all_text_inline(), true)
            .await
            .expect("run ok");

        let markdown = stored(&runner, out.formats.markdown.as_ref()).await;
        assert!(!markdown.contains(EMAIL), "markdown artifact: {markdown}");
        assert!(markdown.contains(MARKER));
        let html = stored(&runner, out.formats.html.as_ref()).await;
        assert!(!html.contains(EMAIL), "the source artifact is the redacted page");

        let output_json = serde_json::to_string(&out).unwrap();
        assert!(!output_json.contains(EMAIL), "output: {output_json}");
        for (field, text) in [
            ("markdown", inline.markdown.as_deref()),
            ("html", inline.html.as_deref()),
            ("raw_html", inline.raw_html.as_deref()),
        ] {
            let text = text.unwrap_or_else(|| panic!("{field} inline"));
            assert!(!text.contains(EMAIL), "{field}: {text}");
        }
        for event in drain(&mut rx) {
            let event_json = serde_json::to_string(&event).unwrap();
            assert!(!event_json.contains(EMAIL), "event: {event_json}");
        }
    }

    #[tokio::test]
    async fn the_org_default_redacts_a_run_that_did_not_ask() {
        let _settings = install(on(PiiOrgDefault::Orgs(vec![ORG.into()])));
        let (tx, _rx) = mpsc::channel(64);
        let runner = runner(tx, ZdrMode::Off, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner.run(&run_id, &url(), None).await.expect("run ok");
        let markdown = stored(&runner, out.formats.markdown.as_ref()).await;
        assert!(!markdown.contains(EMAIL));
        assert!(markdown.contains(MARKER));
    }

    #[tokio::test]
    async fn an_org_outside_the_default_is_not_redacted_unless_it_asks() {
        let _settings = install(on(PiiOrgDefault::Orgs(vec!["org_other".into()])));
        let (tx, _rx) = mpsc::channel(64);
        let runner = runner(tx, ZdrMode::Off, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let out = runner.run(&run_id, &url(), None).await.expect("run ok");
        assert!(stored(&runner, out.formats.markdown.as_ref()).await.contains(EMAIL));
    }

    #[tokio::test]
    async fn under_zdr_the_stage_runs_and_nothing_is_written() {
        let _settings = install(on(PiiOrgDefault::All));
        let (tx, mut rx) = mpsc::channel(64);
        let runner = runner(tx, ZdrMode::On, None);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let (out, inline) = runner
            .run_with_inline_request(&run_id, &url(), None, &InlineRequest::default_for(ZdrMode::On))
            .await
            .expect("run ok");
        assert!(out.formats.markdown.is_none() && out.formats.html.is_none());
        let markdown = inline.markdown.expect("inline markdown under ZDR");
        assert!(!markdown.contains(EMAIL));
        assert!(markdown.contains(MARKER));
        for event in drain(&mut rx) {
            assert!(!matches!(event.event_type, EventType::ArtifactWritten));
            assert!(!serde_json::to_string(&event).unwrap().contains(EMAIL));
        }
    }

    #[tokio::test]
    async fn a_page_that_cannot_be_redacted_fails_typed_and_leaves_nothing() {
        for zdr in [ZdrMode::Off, ZdrMode::On] {
            let _settings = install(on(PiiOrgDefault::All));
            let (tx, mut rx) = mpsc::channel(64);
            let captures = crate::capture::PageCaptures {
                screenshot: Some(png()),
                ..Default::default()
            };
            let runner = runner(tx, zdr, Some(captures));
            let run_id: RunKind = quarry_core::ids::Id::new();
            let error = runner
                .run_with_inline_request(&run_id, &url(), None, &InlineRequest::default_for(zdr))
                .await
                .expect_err("a capture cannot be redacted");
            assert!(is_pii_redaction_failure(&error), "{zdr:?}: {error}");
            let details = error.details.clone().unwrap();
            assert_eq!(details["cause"], "capture_not_redactable");
            assert_eq!(details["field"], "screenshot");
            assert!(!error.to_string().contains(EMAIL));

            let events = drain(&mut rx);
            assert!(
                events.iter().all(|e| !matches!(e.event_type, EventType::ArtifactWritten)),
                "{zdr:?}: nothing is stored for a failed page"
            );
            assert!(events.iter().all(|e| !matches!(
                e.event_type,
                EventType::PageExtracted | EventType::BrandingExtracted
            )));
            for event in &events {
                assert!(!serde_json::to_string(event).unwrap().contains(EMAIL));
            }
            if zdr == ZdrMode::Off {
                let failed = events
                    .iter()
                    .find(|e| matches!(e.event_type, EventType::PageFailed))
                    .expect("a page_failed event");
                assert_eq!(failed.payload["reason"], PII_REDACTION_FAILED);
                assert_eq!(failed.payload["cause"], "capture_not_redactable");
            }
        }
    }
}
