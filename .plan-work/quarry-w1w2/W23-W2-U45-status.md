# W2-U45 status (claim-org half): the final screenshot and PDF are stamped with the claim org

Branch w23/lane-b. Date 2026-11-03. Code only: not compiled, no tests run (owner order for W2/W3 unit work).

## Built
- The gap only. New integration test `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/artifact_org_stamp.rs` (241 lines; CRLF in the working tree, LF in the blob). Two tests:
  - `in_memory_store_stamps_the_final_capture_with_the_claim_org`
  - `filesystem_store_stamps_the_final_capture_with_the_claim_org`
- Each test runs a capturing scrape (a final screenshot and a final PDF, from a driver that carries captures the way the browser driver does) with `PageRunner::org_id` set to the claim org. It runs a second capturing scrape for another tenant on the same store, then asserts:
  - the claim org reads both captures back byte for byte, under a key whose `org=` segment is the claim org (`org_from_key`);
  - the other tenant gets NotFound for both;
  - the claim org lists both by artifact id, and the other tenant lists neither;
  - the claim org gets NotFound for the other tenant's screenshot.
- Already existed, not rebuilt: the action-output org stamp (pipeline.rs `action_outputs_are_stored_with_distinct_keys_and_org`), and the edge's body-org override (routes.rs `scrape_with` ignores `req.org_id` and uses the verified claim).
- Pipeline: no fix. `persist_captures` puts the final screenshot and PDF under `self.org_id`, which the edge sets from the claim (`org_id: Some(org_id.clone())`, with `org_id = claims.org_id.clone()`). The test should pass unchanged, so `pipeline.rs` is untouched.

## Commits (local on w23/lane-b, not pushed)
- 48750c26a test(quarry-runtime): stamp the final screenshot and pdf with the claim org (W2-U45). The test file. Gitleaks hook passed.
- Second commit: this status file and the lane-b depth rows (W23-b-U45-1 to -4).

## Files
- apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/artifact_org_stamp.rs (new)
- .plan-work/quarry-w1w2/W23-W2-U45-status.md (new, this file)
- .plan-work/quarry-w1w2/W23-depth-lane-b.md (rows appended under "W2-U45 (lane b)")

## Flag and default
No flag: test-only unit. The behaviour under test ships with the capture feature, which is dark behind `QUARRY_EDGE__SCRAPE_CAPTURE=off` (the committed default).

## Assumed decisions
- Location: the queue row's file (quarry-runtime `tests/artifact_org_stamp.rs`) governs. The D2-134 row and the W2-U45 depth row say "quarry-edge tests". The edge half is left open as depth row W23-b-U45-1, not written into another unit's file.
- The runtime has no request body, so "the body org" is modelled as a second tenant (`BODY_ORG`) on the same store. That is the tenant the edge would have refused.
- Both artifact stores are covered: the in-memory store (dev) and the filesystem store (prod, per the artifact_store.rs header).
- Only the final screenshot and PDF are asserted. Action outputs are not duplicated (see Built).
- The test uses `list()` and `get_with_key()`, not `count()`, because `FilesystemStore` does not override `count()` (depth row W23-b-U45-4).
- The URL `https://www.example.com/` and the `PAGE` fixture match the existing in-module capture tests. The preflight runs with `allow_private_hosts(true)` and does no DNS lookup, so no network is needed.

## Model Plane asks
None. No file under apps/Model Plane was touched, and MP-ASKS.md was not appended.

## Not verified
- Not compiled and not run (code only). The integration step must run `qtest -p quarry-runtime --test artifact_org_stamp`.
- Checked by reading, not by compiling: each type, field and signature the test uses was matched against its definition (the 20 `PageRunner` fields, `FetchResponse::new`, the `CapturedImage` and `CapturedPdf` fields, the `ArtifactStore` signatures, `ListFilter::default`, `Page::items`, `QuarryError::code`).
- Not verified: that the `PAGE` run returns `formats.screenshot` and `formats.pdf` through this runner. It takes the same path as pipeline.rs `capture_refs_fill_output_formats`, which is also compiled and not run.
- The edge-level claim-versus-body case (W23-b-U45-1) is not covered by this unit.
