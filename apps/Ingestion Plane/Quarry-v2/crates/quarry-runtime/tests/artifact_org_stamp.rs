//! W2-U45 (D2-134, claim-org half): the final screenshot and PDF that a scrape
//! captures are stamped with the org its run was started for.
//!
//! The edge takes the tenant from the verified claim and ignores the request
//! body's `org_id` (quarry-edge `scrape_with`, "Tenant isolation"), then gives
//! that org to the runner as `PageRunner::org_id`. This file pins the runner side
//! on both artifact stores: the in-memory store (dev) and the filesystem store
//! (prod). On each, the claim org reads both captures back under a key it owns,
//! and the other tenant reads and lists none of them.
//!
//! The scrape-time action outputs are covered in the crate by
//! `pipeline.rs` `action_outputs_are_stored_with_distinct_keys_and_org`.

use std::sync::Arc;

use async_trait::async_trait;
use quarry_core::capture::{CapturePlan, CapturedImage, CapturedPdf, ImageFormat};
use quarry_core::error::ErrorCode;
use quarry_core::event::Event;
use quarry_core::ids::kinds::{ArtifactKind, RunKind};
use quarry_core::ids::Id;
use quarry_core::output::{DriverKind, FormatRef};
use quarry_core::pagination::ListFilter;
use quarry_core::privacy::PrivacyPolicy;
use quarry_core::zdr::ZdrMode;
use quarry_core::QuarryResult;
use quarry_runtime::artifact_store::{org_from_key, ArtifactStore, FilesystemStore, InMemoryStore};
use quarry_runtime::capture::PageCaptures;
use quarry_runtime::driver::{Driver, RenderHints};
use quarry_runtime::events::EventSink;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::pipeline::{DocRouting, PageRunner};
use quarry_runtime::policy::RunPolicy;
use quarry_security::preflight::DefaultEngine;
use tokio::sync::mpsc;
use url::Url;

/// The tenant the verified claim names. The edge gives the runner this org.
const CLAIM_ORG: &str = "org_claim";
/// The tenant a request body may name. The edge never passes it on, so no
/// capture of a claimed run is stored under it or readable through it.
const BODY_ORG: &str = "org_named_in_body";

const PAGE: &str = "<html><head><title>Capture page</title></head><body><main><h1>Capture</h1><p>Some body text for the capture test page, long enough to be extracted as the main content of the page.</p></main></body></html>";
const SCREENSHOT_BYTE: u8 = 0x89;
const SCREENSHOT_LEN: usize = 64;
const PDF: &[u8] = b"%PDF-1.7 claim org";

/// A driver whose fetch carries the captures it is given, as a browser fetch does.
struct CaptureDriver {
    captures: PageCaptures,
}

#[async_trait]
impl Driver for CaptureDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Browser
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        let mut response = FetchResponse::new(
            200,
            url.clone(),
            vec![("content-type".into(), "text/html; charset=utf-8".into())],
            PAGE.as_bytes().to_vec(),
            3,
            DriverKind::Browser,
        );
        response.captures = Some(Box::new(self.captures.clone()));
        Ok(response)
    }
}

/// A screenshot and a PDF, as a browser capture hands them over.
fn captures() -> PageCaptures {
    PageCaptures {
        screenshot: Some(CapturedImage {
            bytes: vec![SCREENSHOT_BYTE; SCREENSHOT_LEN],
            format: ImageFormat::Png,
            width_px: 1280,
            height_px: 720,
            tiles: 1,
            stitched: false,
            truncated: false,
        }),
        pdf: Some(CapturedPdf {
            bytes: PDF.to_vec(),
        }),
        ..PageCaptures::default()
    }
}

/// A runner for one capturing run. `org` is what the edge hands over: the verified
/// claim's org. The other fields are the edge's defaults for a scrape that asks for
/// both captures under ZDR off.
fn runner_for(
    org: &str,
    artifacts: Arc<dyn ArtifactStore>,
) -> (PageRunner, mpsc::Receiver<Event>) {
    let (tx, rx) = mpsc::channel(64);
    let render = RenderHints {
        capture: Some(CapturePlan {
            screenshot: Some(Default::default()),
            pdf: Some(Default::default()),
        }),
        ..RenderHints::default()
    };
    let runner = PageRunner {
        driver: Arc::new(CaptureDriver {
            captures: captures(),
        }),
        security: Arc::new(DefaultEngine::new().with_allow_private_hosts(true)),
        artifacts,
        event_sink: EventSink::new(tx),
        zdr: ZdrMode::Off,
        ingest: None,
        org_id: Some(org.to_owned()),
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
    };
    (runner, rx)
}

/// Runs one capturing scrape for `org` on `artifacts` and returns the stored refs
/// of its screenshot and its PDF.
async fn capture_for(org: &str, artifacts: Arc<dyn ArtifactStore>) -> (FormatRef, FormatRef) {
    let (runner, _rx) = runner_for(org, artifacts);
    let run_id: RunKind = Id::new();
    let url: Url = "https://www.example.com/".parse().expect("a valid url");
    let output = runner
        .run(&run_id, &url, None)
        .await
        .expect("the capturing run succeeds");
    (
        output
            .formats
            .screenshot
            .expect("the screenshot is stored, not inline"),
        output.formats.pdf.expect("the pdf is stored, not inline"),
    )
}

/// Runs a capturing scrape for the claim org and one for the other tenant on the
/// same store, then checks the claim org's captures from both sides: what the
/// claim org reads and lists, and what the other tenant reads and lists.
async fn assert_captures_stamped_with_the_claim_org(artifacts: Arc<dyn ArtifactStore>) {
    let (shot, pdf) = capture_for(CLAIM_ORG, artifacts.clone()).await;
    let (other_shot, _other_pdf) = capture_for(BODY_ORG, artifacts.clone()).await;

    let stored: [(&ArtifactKind, Vec<u8>); 2] = [
        (&shot.artifact_id, vec![SCREENSHOT_BYTE; SCREENSHOT_LEN]),
        (&pdf.artifact_id, PDF.to_vec()),
    ];
    for (id, body) in &stored {
        let (bytes, key) = artifacts
            .get_with_key(CLAIM_ORG, id)
            .await
            .expect("the claim org reads its own capture");
        assert_eq!(&bytes, body, "the capture reads back as it was stored");
        assert_eq!(
            org_from_key(&key),
            Some(CLAIM_ORG),
            "the stored key carries the claim org: {key}"
        );
        let refused = artifacts
            .get(BODY_ORG, id)
            .await
            .expect_err("the other tenant must not read a capture of the claimed run");
        assert_eq!(refused.code, ErrorCode::NotFound);
    }

    let claim_listing = artifacts
        .list(CLAIM_ORG, &ListFilter::default())
        .await
        .expect("the claim org lists its artifacts");
    let claim_ids: Vec<ArtifactKind> = claim_listing
        .items
        .iter()
        .map(|item| item.artifact_id.clone())
        .collect();
    assert!(
        claim_ids.contains(&shot.artifact_id) && claim_ids.contains(&pdf.artifact_id),
        "the claim org lists both captures"
    );
    assert!(
        claim_listing
            .items
            .iter()
            .all(|item| item.org_id == CLAIM_ORG),
        "every artifact the claim org lists is stamped with the claim org"
    );
    assert!(
        !claim_ids.contains(&other_shot.artifact_id),
        "the claim org does not list the other tenant's capture"
    );

    let other_listing = artifacts
        .list(BODY_ORG, &ListFilter::default())
        .await
        .expect("the other tenant lists its own artifacts");
    let other_ids: Vec<ArtifactKind> = other_listing
        .items
        .iter()
        .map(|item| item.artifact_id.clone())
        .collect();
    assert!(
        !other_ids.contains(&shot.artifact_id) && !other_ids.contains(&pdf.artifact_id),
        "the other tenant lists none of the claimed run's captures"
    );

    // The reverse direction on the same store: the claim org cannot read the other
    // tenant's own capture by its id.
    let refused = artifacts
        .get(CLAIM_ORG, &other_shot.artifact_id)
        .await
        .expect_err("the claim org must not read another tenant's capture");
    assert_eq!(refused.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn in_memory_store_stamps_the_final_capture_with_the_claim_org() {
    assert_captures_stamped_with_the_claim_org(Arc::new(InMemoryStore::new())).await;
}

#[tokio::test]
async fn filesystem_store_stamps_the_final_capture_with_the_claim_org() {
    let root = tempfile::tempdir().expect("a temp dir for the artifact root");
    let store = FilesystemStore::new(root.path().to_path_buf()).expect("the artifact root opens");
    assert_captures_stamped_with_the_claim_org(Arc::new(store)).await;
}
