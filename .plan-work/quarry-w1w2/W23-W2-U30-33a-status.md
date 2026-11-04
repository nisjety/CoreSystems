# W2-U30-33a status: dev-stub import scope and gate_url on answer fetches (D2-055, U33 part)

Plane: Ingestion (quarry-edge, quarry-runtime). Lane c, branch w23/lane-c (fast-forwarded to w23/integ first).

## Built
Nothing existed beforehand (the stub carried the scope; the answer routes never called gate_url). Both halves built.

1. Dev-bypass stub: `dev_bypass_stub_claims()` in `auth.rs` now carries `scopes: Vec::new()`. It no longer holds `quarry:profiles:import`, so a bearer that fails JWKS verification under the bypass cannot import a profile.
   - Unit test `the_dev_bypass_stub_holds_no_scope_and_cannot_import_profiles` (auth.rs).
   - Edge test `the_dev_bypass_stub_cannot_import_a_profile` (tests/profile_roundtrip.rs): POST /v1/profiles is 403 naming the scope, nothing stored.
   - Fallout fixed: `tests/profile_roundtrip.rs` used the stub to save profiles over HTTP; those tests now put the snapshot straight into the store (`save_snapshot`). `tests/profile_guard.rs::import_over_cap_is_413` expected 200 for a 1.5 MB body; it now expects 403 (passed the cap layer, refused for the scope). RUNBOOKS.md line about the stub updated.
2. gate_url on answer fetches:
   - runtime `answer.rs`: `UrlGate` (= `Arc<dyn Fn(&str)->bool>`), `FetchFailure::PolicyDenied` (wire code `policy_denied`), `AnswerPipeline::prepare_gated` / `answer_gated` (`prepare` / `answer` call them with `None`, so every other caller is unchanged). A denied URL is never fetched, never written to the corpus, and the search snippet does NOT stand in for it: the citation is `outcome: {kind: failed, reason: {code: policy_denied}}`, counted in `sources_skipped`.
   - edge `answer_routes.rs`: `url_gate()` takes one `web_policy_edge::decide` per request (with the effective ZDR posture) and wraps `gate_url`; both `/v1/answer` and `/v1/answer/stream` use it. An undecidable policy is a 403 before any work (ZDR is still refused first, ahead of the gate). Denials are counted by the existing `record_denial` counters.
   - `policy_fp` unchanged: the gate only reads the decision; no layer, no RunPolicy, no fingerprint input is added (the answer routes never compute a policy_fp).

## Commits
ea662b480 (stub scope + gate_url on /v1/answer and /stream), 15d19421f (status, depth rows, runbook note), 7ae4efbd1 (security review: gate include_answer, keep restricted-policy runs off the answer caches). Re-run on the integ-merged lane c tree 2026-11-02: answer_gate 11 passed, quarry-edge --lib auth:: 44 passed, profile_roundtrip 12 passed.

## Tests and counts
Run through the lane c wrapper (Redis started inside the container, so the cache test ran, not skipped):
- `cargo test -p quarry-edge --test answer_gate`: 7 passed, 0 failed (denied host never fetched on /v1/answer and /stream, default policy fetches both, all-denied makes no model call, ZDR refused before search/fetch/model on both routes, ZDR stream leaves no answer-cache entry and the control run does).
- `cargo test -p quarry-edge --lib auth::`: 44 passed (includes the new stub unit test); `the_dev_bypass_stub_holds_no_scope_and_cannot_import_profiles`: 1 passed.
- Regression: `--test profile_roundtrip` 12 passed (incl. the new 403 test); `--test profile_guard` 8 passed; `--test answer_provider_status` 14 passed; `--test web_policy_routes` 11 passed; `cargo test -p quarry-runtime --lib answer::` 31 passed.

## Flag and default
No flag (leak fix, per scope). The default policy admits every URL, so with no operator baseline the answer routes behave as before (test `answer_under_the_default_policy_fetches_every_result`).

## Assumed decisions
- A policy-denied result is skipped outright (no snippet fallback): the snippet is the provider's copy of the refused page.
- The reason is a new `FetchFailure::PolicyDenied` variant (additive on the wire: `policy_denied`), not a new SourceOutcome kind, so existing `failed` consumers keep working.
- The dev stub carries NO scopes at all (not only without the import scope); nothing relied on any other.
- ZDR on the answer routes stays a refusal before the gate (ADR-4); the gate is built with the effective posture anyway so it holds if ZDR synthesis is ever allowed.

## Model Plane asks
None.

## Not verified
- `include_answer` on `POST /v1/search` runs the same pipeline (`search_routes.rs` ~894) with `pipeline.answer`, which is the ungated path. Not in this unit's file scope; recorded as a depth row. The same `url_gate` can be passed there.
- OpenAPI/SDK enumerate `FetchFailure` codes nowhere (no schema lists them); not regenerated.
- No live check against the deployed edge.
