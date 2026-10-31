# D2-A-openapi-internal: findings not fixed here (2026-10-28)

Unit: decision 3.13 OPENAPI-INTERNAL, first half (DECISIONS-ROUND-2026-10-27.md section 3.13). Every item below was
seen while doing the move and is left open on purpose. Each says what was checked, why it stays open, and who decides.

## 1. The eight Go control internal routes are not moved (D2-050)

- Routes (services/quarry-control, registered in internal/resources/resources.go and schedules.go, zdr_tombstone.go):
  POST /v1/internal/jobs/claim, GET /v1/internal/jobs/pending-control, POST /v1/internal/jobs/{id}/control-ack,
  GET /v1/internal/jobs/running, POST /v1/internal/jobs/{id}/finalize, GET /v1/internal/schedules/desired,
  POST /v1/internal/schedules/{id}/runs, POST /v1/internal/zdr-tombstones.
- Why open: the decision says they follow the D2-050 internal secret split (finalize can end any org's job; the group
  shares an HMAC secret with the edge run routes). Documenting them now would describe an auth model that is about to
  change. The internal spec says so in its info description.
- Owner: D2-050 owner. Next step: add them to docs/openapi-internal.yaml in the same commit as the secret split, and
  extend the guard (it checks only the edge's registered routes today).

## 2. The public spec's envelope schemas do not match the wire (not a move issue)

- docs/openapi.yaml: Envelope_Error, Envelope_HandoffAck, Envelope_NormalizedOutput describe top-level `request_id` and
  `success` with `data` / `error`.
- The edge serialises quarry-core `Envelope<T>` as `{ data, meta: { request_id }, error }` (crates/quarry-core/src/envelope.rs).
  Evidence: tests/request_id.rs:73 asserts `body["meta"]["request_id"]`; `grep -rn '"success"' crates/quarry-edge/src
  crates/quarry-core/src` finds nothing. The internal spec documents the wire shape (InternalMeta, InternalErrorEnvelope).
- Not checked: whether the verevonv3 gateway normalises the envelope before the SPA sees it. If it does, the public
  spec describes the gateway's output, not the edge's; either way the SDK typed envelopes (EnvelopeError,
  EnvelopeHandoffAck, ...) follow the spec.
- Owner: the API owner decides which side is the contract. Next step: a wire test per envelope family, not a spec edit.

## 3. Dangling security references in the public spec (pre-existing)

- docs/openapi.yaml uses `security: - bearerAuth: []` 30 times and defines no `securitySchemes` (grep count 0).
  The generated SDKs therefore carry auth references to an undefined scheme. Not changed here (public spec, outside
  the move). Owner: whoever owns the public SDK auth story.

## 4. Eight orphan component schemas in the public spec (pre-existing)

- Reachability (scratchpad/D2-A-openapi-internal/refs.mjs, run after the move): Envelope_NormalizedOutput,
  EvidenceDelta, FrameSnapshotRefAction, LegacyAgentAction, RobotsFetchDenial, SemanticTargetAction,
  SnapshotRefAction, TabOpener are defined and referenced by no path. The generator still emits models for them.
  RobotsFetchDenial is named only in text (the run_page 403 description, now in the internal spec).
- Owner: public SDK owner. Next step: delete or reference each one.

## 5. page-images is unauthenticated by design; exposure depends on routing (D2-312, not proven live)

- GET /v1/internal/page-images/{org}/{doc}/{page}/{hash} has no token: the content hash is the capability on the trusted
  bus (routes.rs:111, resource_routes.rs:198). Anyone who can reach the edge's port can fetch an image whose hash they know.
- Checked: the gateway's calls built on state.quarry_edge_url (grep) are /v1/crawl and /v1/scrape only
  (domains/actions/dispatchers.rs:229, 284, 327, 413). No /v1/internal/* call or generic forwarder was found in
  apps/Frontend Plane/verevonv3/apps/gateway/src. Other clients of the edge were not searched for.
- Not checked: the live ingress (compose, reverse proxy, the Control Plane's routes). D2-312 stays open until a live
  request to /v1/internal/* through the public host is refused.
- Owner: Ingestion Plane and Frontend Plane gateway owners. Next step: a scripted negative test on the stack (list B).

## 6. A missing page image answers 500 INTERNAL, not 404

- get_page_image returns the store's error, and the CAS get_object maps every failure to ErrorCode::Internal
  (crates/quarry-runtime/src/cas_store.rs:91-103). The embedding consumer may treat a 500 as retryable.
- The internal spec describes the current behaviour (the 500 entry says so). Fix belongs to quarry-runtime's CAS error
  mapping (NotFound for a missing key). Not changed: outside this unit's files.

## 7. Stale -Aquatiq-Dev copies in sdks/typescript/docs (pre-existing)

- Commit 7c75b3c32 ("chore: snapshot local workspace changes") left 63 tracked `*-Aquatiq-Dev.md` files. The current
  generator does not write them. This unit deleted the three internal ones (InternalApi-, InternalRunPage-,
  InternalRunPageResult-Aquatiq-Dev.md) with the internal docs. The other 60 remain.
- Owner: SDK owner decides whether to delete them. Not internal, so not touched here.

## 8. The internal spec is written from the handlers, not from a live edge

- The paths, auth schemes, status codes and bodies of the eight edge internal routes were read from the Rust handlers
  (change_routes.rs, resource_routes.rs, schedule_preflight.rs, internal_routes.rs, queue_routes.rs, routes.rs).
  No stack was run; no test compares these shapes with a live response.
- The guard checks that the documented paths equal the registered ones, that the file parses, and that every $ref
  resolves. It does not check the body shapes. Next step: a Rust integration test per internal route that asserts the
  top-level keys (list B, stack session or a quarry-edge test).

## 9. Refreshed and copied schemas: what was verified

- InternalRunPage and InternalRunPageResult were rewritten from the edge's structs (routes.rs:1338-1400). D2-331 (the
  stale InternalRunPage) is closed by this move: user_id, privacy, render, prefer_http3 and kind are now described.
- The crawl family (CrawlContext, CrawlScope, CrawlReport, CrawlDenials, CrawlDenialSample, CrawlRobotsDenial) was
  moved verbatim. Checked against quarry-core/src/crawl_scope.rs: CrawlContext and CrawlReport top-level keys. Not
  re-checked: the nested denial and scope fields.
- CachePolicy and DriverSignals stay public (used by ScrapeRequest) and are copied verbatim into the internal file.

## 10. Names kept in repo docs only, on purpose

- The internal spec names the env vars (QUARRY_EDGE__RUNTIME_AUTH_TOKEN, QUARRY_EDGE_INTERNAL_TOKEN,
  QUARRY_EDGE__INTERNAL_SECRET, QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES, QUARRY_EDGE_AUTH_DEV_BYPASS) and the signature
  headers (X-Quarry-Run-Sig, X-Quarry-Queue-Sig). Names only, no values. The decision's open risk asks that the file stay
  in repo docs, and the guard checks that no code or generator reads it.

## 11. Test environment notes

- Node suite (Quarry scripts/tests/*.test.mjs): 53 tests, 41 pass, 0 fail, 12 todo. The todo set is the docs-contract
  option-B pending list (unchanged, D2-027 / QB-T17); their failure text prints as warnings, not failures.
- The docs-contract test file had another unit's uncommitted hunk when this unit started; it is not in this unit's commits.
- Python SDK: pytest 9.2.0, pydantic 2.14.1, urllib3 2.8.0 (installed to a scratch target with pip --target; the
  decision's note said pytest 8). tests/ (tracked smoke): 25 passed (was 28; the three InternalRunPage cases are gone).
  Full collection from sdks/python without --cov: 338 passed, 2 warnings (PytestCollectionWarning for the generated
  model TestIdLocator, from a public schema of that name; pre-existing).
- TypeScript 5.9.3 (npx): `tsc -p sdks/typescript --noEmit`, exit 0, no output.
- No Rust, Go or gateway code changed, so no cargo run was needed for this unit.
