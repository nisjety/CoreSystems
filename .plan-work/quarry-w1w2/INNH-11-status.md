# INNH-11: Public route that parses a PDF/DOCX to Markdown without storing

Status: DONE IN TESTS, DARK. Built and committed locally on branch innh/lane-c (2026-10-31). Default off (`QUARRY_EDGE__PARSE_ROUTE`). On no stack; nothing deployed, no live flag changed.
Lane: C (crawl/change/parse). Plan home: E3.6 (plan item F-t2-037). Paths under `apps/Ingestion Plane/Quarry-v2`.

## What existed (before this item)
The parsing capability existed only inside a URL scrape: `PageRunner` routes PDF/DOCX to `content_route::parse_document` when `QUARRY_EDGE__DOC_ROUTING=on`. `content_route.rs` already had the 15 s deadline, the 2 process-wide parse permits, the 8 MiB text cap, the 415 `pdf_parse_failed|docx_parse_failed|too_large` mapping and the rlimited child-process backend (`QUARRY_EDGE__DOC_PARSE_ISOLATION`). Extractors in `quarry-transform` (`pdf.rs`, `docx.rs`, `classify_pages`). There was NO `/v1/parse` route, no raw-upload route, and the usage metrics list had no parse metric.

## What was built
- `crates/quarry-edge/src/parse_routes.rs` (new): the handler. It takes no `AppState`, so it has no store, cache, index, sink or meter to write to, and logs nothing about a request. Order of checks: flag (501), isolation (501 unless the parse is in the child), scope, declared Content-Type and Content-Length (before any body byte), per-org and process slot (429 before the body is read), body read up to the limit (413), kind from the bytes (415), parse in the child (415/429/504). 200 carries `Cache-Control: no-store`.
- `routes.rs`: its own router with the surface's auth chain (`require_auth`, `mark_floor`, `require_service_route_scope`), because the body is up to 25 MiB (the surface caps at 1 MB). `auth.rs`: `/v1/parse` needs `parse:write` for a service principal. `lib.rs`, `main.rs`: module and a boot line `document parse route is on` (isolation, serving).
- `crates/quarry-runtime/src/content_route.rs`: `parse_document_info` / `parse_document_info_with` return text plus PDF page facts (`PdfPageSummary`, child kind `pdf-pages`), and a queue timeout so a busy parser is a 429 `parser_busy` with `retry_after_seconds`, not a hang. The existing `parse_document` path is unchanged in behaviour.
- `crates/quarry-core/src/zdr_matrix.rs`: row `parse` (Inert, Enforced). Goldens regenerated with `QUARRY_UPDATE_GOLDEN=1`: `crates/quarry-runtime/contracts/zdr-feature-matrix.json`, `.sha256`, `docs/ZDR_MATRIX.md`.
- Tests: `tests/web_policy_routes.rs` manifest (`/v1/parse` NoUrl), `tests/support/zdr_harness.rs` (`Harness::url`), `tests/parse_route.rs` (17), `tests/parse_route_off.rs` (2), `tests/parse_route_process.rs` (1, starts the real edge binary and sends a decompression bomb), `quarry-runtime/tests/doc_parse_sandbox.rs` (page-facts legs), unit tests in `parse_routes.rs` and `content_route.rs`. Dev-dependencies `flate2` and `zip` in `quarry-edge/Cargo.toml`.
- Docs: `docs/CONTRACTS.md` (section), `docs/ERROR_REASONS.md`, `docs/RUNBOOKS.md` (enable/disable/read/evidence owed), `docs/GAP.md` line 85, `docs/openapi.yaml` (`/v1/parse`, schemas `ParseData`, `ParseWarning`, `ParseResponse`, `ParseError`, tag `parse`), `scripts/openapi-route-allowlist.json` (`route_registrations` 110).
- This session's fix: the review's broken test. In `tests/parse_route.rs`, `declare_only` built its raw request with bare newlines (a multi-line string literal; Rust turns a source CRLF in a literal into LF, so the server saw a request line followed by an empty line and no headers). It now uses `\r\n` escapes in a single-line literal, and the response body split uses `\r\n\r\n`. No other code change was needed: the builder's code compiled and passed on the first run.

## Commits (branch innh/lane-c, local only)
- `03e6608f6` feat(quarry-edge): POST /v1/parse ... (code, tests, ZDR matrix row, regenerated goldens and ZDR_MATRIX.md)
- `819f83ae6` docs(quarry): POST /v1/parse contract, errors, runbook and OpenAPI
- (this status file and the depth append: the commit after these)

## Tests (Linux container via qtest-lane.sh, lane c, base 4130f3382 plus this change)
- `cargo test -p quarry-edge --lib`: 703 passed, 0 failed.
- `cargo test -p quarry-edge --test parse_route`: 17 passed (the first run had 16 passed and 1 failed: `a_body_over_the_limit_is_a_413_...`, the `declare_only` defect above; green after the fix). `--test parse_route_off`: 2 passed. `--test parse_route_process`: 1 passed.
- `cargo test -p quarry-runtime --test doc_parse_sandbox`: 12 passed.
- `cargo test -p quarry-core zdr_matrix`: 6 passed after `QUARRY_UPDATE_GOLDEN=1`, and 6 passed again without the variable. No schema golden changed (`UPDATE_SCHEMAS` was not needed).
- `cargo test -p quarry-edge --tests --no-fail-fast`: 1984 passed, 0 failed across 76 test result lines (lib, bin and all integration binaries, including `zdr_feature_matrix`, `web_policy_routes`, `log_field_discipline`).
- `cargo test -p quarry-runtime --lib`: 1062 passed, 0 failed.
- `cargo test -p quarry-core` (all targets): 364 passed, 0 failed across 7 result lines.
- `node --test scripts/tests/*.test.mjs` on the host: 60 tests, 48 pass, 0 fail, 12 todo (the todo ones are marked known gaps in the guards, for example the `/v1/search` 400 documentation guard that points at another branch's openapi work; none concerns `/v1/parse`).
- Pre-existing failures: none seen. Nothing failed, so there was nothing to compare against the base.

## Not verified
- Not run on any stack: no side-by-side stack, no real gateway, no real token from the Control Plane. The `curl --data-binary` evidence and the "GET /v1/artifacts shows no new row" check are owed (RUNBOOKS has the recipe).
- The memory-bomb leg through HTTP is covered only by the flate bomb that the inflate budget refuses (`parse_route_process.rs`); the RLIMIT_AS kill is covered at the extractor level in `doc_parse_sandbox.rs`, not through the route.
- No log-capture test for the route in `zdr_log_capture` (the source-grep test forbids `tracing::` in the module only).
- No ZIP-bomb DOCX through the route, no end-to-end 504 deadline through HTTP, field names of `ParseData` not pinned in `quarry-core/tests/contracts.rs`.
- Depth findings from the review (sniff in the edge process, child inherits the environment, three-pass PDF parse against the 15 s deadline, shared parse permits with scrapes, slow-upload slot holding, Vec growth, warning accuracy for blank pages, child stdout cap) are NOT fixed: see `INNH-depth-lane-c.md`. Fix the sniff, the environment and the shared permits before enabling on a shared edge.
- Windows: the route needs the Unix child; on Windows it answers 501 `isolation_required` unless `QUARRY_EDGE__PARSE_ALLOW_INPROCESS=1`. Tests ran in Linux.

## Owner decisions needed
1. Metering and billing: the route is not metered (no usage record, no Lago metric). Options: reuse `quarry.scrape.page` with `attrs.op='parse'`, or a new metric (needs a Control Plane billing code). A metering call needs only the org id and byte count, so it can be added without keeping content; the source-grep test that forbids the word `usage` would need a narrower rule.
2. The `parse:write` scope: it is not in the Control Plane registry (`plane-service-principals.json`, the auth-core golden claims fixture), so no service principal (the Innhenting gateway included, if it calls with a service token) can call the route until the owner adds it. A user token needs no scope. The route fails closed.
3. Enabling: `QUARRY_EDGE__PARSE_ROUTE` on a shared edge, after the depth findings above are handled.
4. Multipart upload is out of v1 (raw bytes only); the gateway forwards raw bytes. Say so if the SPA wants multipart.
5. Per-org limits (1 parse, 4 uploads per process) are per edge process, not per cluster.

## Contract for the Innhenting gateway and UI
Verbatim from `docs/CONTRACTS.md`, section "`POST /v1/parse`":

**Request.** The body is the document's raw bytes. Not a URL (a URL is `POST /v1/scrape` with `QUARRY_EDGE__DOC_ROUTING=on`), and not multipart: multipart is out of v1 (it needs a multipart parser in the edge, and the gateway can forward raw bytes). `Content-Type` is absent, `application/octet-stream`, `application/pdf` or `application/vnd.openxmlformats-officedocument.wordprocessingml.document`; any other type is a 415 before the body is read. The bytes decide the kind (`quarry_transform::sniff`, magic numbers only): a PDF marker in the first kilobyte, or a ZIP whose `[Content_Types].xml` names WordprocessingML. A declared PDF or DOCX type that the bytes contradict is a 415 `content_type_mismatch`; bytes that are neither are a 415 `unsupported_media` with `details.detected`. No header is read for a filename and none is echoed, stored or logged. No JSON body.

**Response 200**, in the typed envelope, with `Cache-Control: no-store`:
```
{ "data": { "markdown": "...", "kind": "pdf" | "docx", "bytes": 1599, "structure": "plain",
            "truncated": false, "pages": 1, "warnings": [ { "code": "...", "message": "..." } ] },
  "meta": { "request_id": "..." } }
```
`pages` is for a PDF only (absent for a DOCX, never null). `structure` is always `plain`: the text is paragraphs, with no headings, lists or tables marked up, and a PDF whose text sits in `Tj` and `TJ` runs loses its layout. `warnings` is always present, usually empty: `document_has_no_text_layer` (no page of the PDF has a text layer, a scan, so the text is empty), `some_pages_have_no_text_layer` (a mixed PDF; the message says how many pages), `document_has_no_text` (a DOCX with no text), `truncated_at_8mib` (the text was cut at 8 MiB, `truncated` is true). There is no OCR. **No redaction is applied** (`pii.rs` is not wired): personal data in a document comes back as it is, to the authenticated caller. No artifact id, no scrape id: nothing was kept to point at.

**Errors**, in the standard envelope (`{ "error": { "code", "message", "details" }, "meta": { "request_id" } }`): 400 `empty_body` (or `body_unreadable`); 413 `too_large` with `max_bytes`, by the declared `Content-Length` before the body is read, or by the bytes when none was declared (the limit is 25 MiB, the parsers' own; `QUARRY_EDGE__PARSE_MAX_BYTES` lowers it, never raises it); 415 as above, and `pdf_parse_failed`, `docx_parse_failed` or `too_large` from the parser (a file it cannot read, or that its limits stopped), each with `details.kind` and `details.supported: ["pdf","docx"]`; 429 `RATE_LIMITED` with `Retry-After`; 501 as above, or `isolation_required`; 504 `TIMEOUT` (the 15 second deadline). 401 and 403 come from the auth chain.

**Limits.** One organisation may have `QUARRY_EDGE__PARSE_CONCURRENCY_PER_ORG` parses in flight (default 1, at most 8) and the process `QUARRY_EDGE__PARSE_MAX_INFLIGHT` uploads (default 4, at most 32); past either the answer is a 429 (`org_parse_busy`, `parse_capacity`) with `retry_after_seconds` 2 and `Retry-After: 2`, before the body is read. A request that has a slot then waits at most 3 seconds for one of the two process-wide parse slots (shared with the scrape pipeline): past that, 429 `parser_busy` with `Retry-After: 5`. The parse itself: a child process under 768 MiB of address space, 20 s of CPU and 16 file descriptors, the 15 second deadline, the 8 MiB text cap, 25 MiB of input.

**Isolation.** The route refuses to serve (501 `isolation_required`) while documents would be parsed inside the edge process (`QUARRY_EDGE__DOC_PARSE_ISOLATION=inprocess`, or an edge that is not a Unix `quarry-edge-rs`): a hostile file could take the edge down. `QUARRY_EDGE__PARSE_ALLOW_INPROCESS=1` lifts that for a single-tenant development box.

**Auth.** A user token needs no scope. A service principal needs `parse:write` (not in the Control Plane scope registry yet, see the owner decisions). No other scope stands in for it.

**ZDR and storage.** The handler takes no `AppState`: nothing is written (no artifact, cache, index, event, usage record, ingest). A ZDR caller is served the same way. **Billing:** not metered in this version.

Gateway notes (from this status, for the Innhenting session): forward the request body as raw bytes with the caller's `Content-Type` (or drop the header); a `Content-Type` other than the three above is a 415 even when the bytes are a valid PDF, so a client that sends `application/x-www-form-urlencoded` (curl `--data-binary` without `-H`) must be corrected at the gateway. The envelope on 429 has `details.retry_after_seconds` and the HTTP `Retry-After` header. A bare 408 with an empty body can come from the router's 60 s path deadline (a stalled upload) and is not in the envelope. Error codes to show: `BAD_REQUEST` (400/413), `UNSUPPORTED_MEDIA` (415), `RATE_LIMITED` (429), `UNSUPPORTED` (501), `TIMEOUT` (504); the UI should key on `error.details.reason`.
