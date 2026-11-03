# INNH-2 progress

Run of 2026-10-31 (after INNH-1 U1 to U3). Continue from the tree: every step below leaves a checkable change.

Working note: `extract_routes.rs`, `ai_formats.rs`, `RUNBOOKS.md` and `zdr_claim_floor.rs` are CRLF in the worktree (index is LF, `core.autocrlf=true`). They were converted to LF to edit and must be converted back to CRLF before the commit (`python scratchpad/eol.py crlf <files>`); `git diff` is the same either way.

- [x] Read INNH-2-status.md, brief, extract_routes.rs, ai_formats.rs, mp_client invoke path, zdr_matrix `extract` row, zdr_claim_floor harness.
- [x] Engine (`quarry-runtime/src/ai_formats.rs`): `JsonRequest { schema, prompt }`, `AiFormatRunner::extract_for_org` (refuses ZDR before any request), `json`/`json_for_org` kept as thin wrappers, prompt template with the caller's text outside a random-id page fence (reuses `ai_scrape::fresh_nonce` and `frame`, made `pub(crate)`), prompt-only asks for one JSON object and rejects a non-object reply. `lib.rs` re-exports `JsonRequest`.
- [x] Route (`quarry-edge/src/extract_routes.rs`): `#[allow(dead_code)]` gone; `clean_prompt` (<= 10000 chars, control chars dropped, trimmed, empty is 400, no echo); model arm for schema or prompt; 501 only when not ZDR; ZDR per-source `ZDR_UNAVAILABLE` (page still read under ZDR, harvest kept); `ExtractItem` gains `schema_valid`, `schema_issues`, `truncated_input`; 50 s request budget (`run_extract` takes it as a parameter for tests).
- [x] Docs: openapi.yaml (ExtractRequest.prompt maxLength + description, ExtractItem fields, 400/501 text, operation description), CONTRACTS.md new section "`/v1/extract`: the `prompt` takes effect", RUNBOOKS.md new section.
- [x] ZDR matrix `extract` row: basis + 3 cited tests; golden JSON, `.sha256`, `ZDR_MATRIX.md` regenerated.
- [x] Tests written: 8 engine tests, 11 route tests, 1 integration test (`zdr_claim_floor`).
- [x] Rust (qtest, quarry-target-integ) and node tests green: counts in INNH-2-status.md. clippy and rustfmt clean on the new code.
- [x] Status file, INNH-STATUS row 2, INNH-depth.md (7 lines) written; CRLF restored on the four CRLF files; committed locally.
