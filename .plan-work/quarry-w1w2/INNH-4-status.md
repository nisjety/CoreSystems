# INNH-4: Change monitoring text diff with stored artifacts

Status: done in tests, dark (default-off flag `QUARRY_EDGE__CHANGE_TEXT_DIFF`). Branch `innh/lane-b`, built 2026-10-31. Nothing was run against a real Postgres, Temporal or a deployed stack; not enabled anywhere.
Lane: C (crawl/change/parse), built in lane-b. Plan home: E3.5. All paths under `apps/Ingestion Plane/Quarry-v2`.

## What existed (verified by reading, still true before this change)
- `record_check` saved baselines with `artifact_id: None` and, on a change, a one-line fingerprint note (kind `raw`, `DiffRecord.format = text`). Only the orchestrator's `CheckChange` reaches it (`/v1/internal/change/record`); `POST /v1/change/check` takes a caller fingerprint, fetches nothing and stores nothing.
- The page markdown artifact was already written on every non-ZDR run (`pipeline.rs`) and carried in `NormalizedOutput.formats.markdown`, but `InternalRunPageResult` and the Go `RunPageResult` did not return its id.
- `quarry_transform::diff::diff_paragraphs` had no non-test caller. The store had no `load_baseline` or `load_diff`.
- The webhook and event payload had `new_baseline` and `diff_id` empty (`compare_snapshot` leaves them for the caller), so the `change_detected` idempotency key ended in an empty baseline id.

## What I built
1. `markdown_artifact_id` on run_page (general, item 12 reuses it): `InternalRunPageResult` (skipped when None; the cached branch uses the same `from_output`), Go `RunPageResult.MarkdownArtifactID`, forwarded by `CheckChange` on the record body (omitted when empty, so an old runtime or a ZDR run gives a byte-identical body).
2. Record path (`change_routes.rs`, `record_core`): with the flag on and the body not `zdr: true`, a new or changed check verifies the id is a markdown artifact of the body's org (`verify_markdown_artifact`, 400 and nothing saved otherwise), saves the baseline with `artifact_id`, and on a change stores a JSON paragraph diff (artifact kind `change_diff`) with `DiffRecord.format = json` and a summary of at most 1 KB. `ChangeRecord.new_baseline` and `diff_id` are filled before the event and webhook (idempotency key now ends in the baseline id). Flag off: the old code path, line for line, tested.
3. Store: `load_baseline`, `load_diff`, `find_diff` (org filter in SQL).
4. `GET /v1/change/diff?url=[&from=&to=][&diff_id=][&include=markdown]` (`change_diff.rs` + `change_routes::diff`): latest two by default, two named baselines, or a stored diff by id; 404 for unknown and other-org ids (one message, no oracle); 400 for bad combinations; 501 flag off; 403 ZDR reader. Uses the stored diff when it reads back, else diffs the stored pages now; unavailable answers are 200 with a closed `reason`.
5. Fixes found on the way, in `quarry-transform`: `paragraph_chunks` panicked on a multi-byte character at a cut (the pipeline calls it on every page's markdown with 1000); `diff_paragraphs` is linear and `moved` no longer fires for every paragraph after an inserted one.
6. Docs: openapi (`/v1/change/diff`, 6 schemas, BaselineSnapshot and ChangeRecord notes), openapi-internal (both new fields), CONTRACTS (new section), CHANGE_TRACKING, RUNBOOKS (enable, disable, retention, ZDR), ZDR matrix row `change_diff_read` (golden JSON, sha256 and ZDR_MATRIX.md regenerated, then re-run without the update), route-count pin 108 to 109, docs-contract test for the switch.

## Contract the Innhenting UI needs
Public, bearer, org from the token, envelope `{ data, meta }`. Off by default (501 until the edge sets the flag).

`GET /v1/change/diff` query: `url` (required unless `diff_id`), `from` and `to` (baseline ids, together), `diff_id`, `include=markdown`.

```json
{ "data": {
  "source_url": "https://example.com/pricing",
  "from": { "baseline_id": "bln_...", "fingerprint": "blake3:...", "captured_at": "...", "artifact_id": "art_..." },
  "to":   { "baseline_id": "bln_...", "fingerprint": "blake3:...", "captured_at": "...", "artifact_id": "art_..." },
  "diff_id": "diff_..." ,
  "available": true,
  "reason": "no_previous_version | no_previous_text | no_text | artifact_gone",
  "diff": { "format": "paragraph", "real_delta": true, "added": 1, "removed": 1, "moved": 0, "unchanged": 14,
            "ops": [ { "op": "added", "index": 6, "text": "..." }, { "op": "removed", "index": 6, "text": "..." }, { "op": "moved", "from": 2, "to": 5 } ],
            "truncated": false },
  "markdown": { "from": "...", "to": "...", "truncated": false }
} }
```
`from` is null with one baseline; `diff_id` is null when the pair has no stored diff (computed now); `artifact_id` is null for a baseline recorded without text; `reason` and `diff` are mutually exclusive; `markdown` only with `include=markdown`. `real_delta: false` means the page bytes changed and its text did not (say "no text change" in the UI). The webhook and `change_detected` event payload (`ChangeRecord`) now carries `diff_id` and `new_baseline` (with `artifact_id`), so a notification can link to `?diff_id=`. Caps: 500 ops, 2 KB per op text, 1 MiB per markdown side. The baseline text is also readable through `GET /v1/artifacts/{id}`. Gateway route and SPA are the Innhenting session's.

## Commits
- `c9cca3667` feat(quarry-core): change text diff contract, store loaders, safe paragraph chunking (INNH-4)
- `9455099e3` feat(quarry-orchestrator): CheckChange forwards run_page's markdown artifact id (INNH-4)
- `96c5dc9c3` feat(quarry-edge): change text diff, stored markdown on baselines and GET /v1/change/diff behind QUARRY_EDGE__CHANGE_TEXT_DIFF (INNH-4)
- docs commit: openapi, openapi-internal, CONTRACTS, CHANGE_TRACKING, RUNBOOKS, route-count pin, docs-contract test, this status, progress and depth-lane-b files (`docs(quarry): ... (INNH-4)`, the commit after `96c5dc9c3` on `innh/lane-b`)

## Tests (counts)
Run in the lane container (`qtest-lane.sh b`) and on the host, 2026-10-31:
- quarry-core: 280 lib tests pass (new: 4 in `change_history` for the wire shapes and `cut_utf8`, 1 in `artifact` for the new kind); the ZDR matrix golden test passes after regeneration. quarry-transform: 11 `diff` tests (5 new) and 3 new `chunk` tests pass.
- quarry-edge `--features postgres-queue --lib`: 699 pass. New: 22 in `change_diff` (query rules, gate order, ops cap with moves not hiding a removal, character-boundary cut, latest-two, named pair, stored diff preferred, legacy record fallback, other-org ids read as unknown, different urls, one baseline, each unavailable reason, foreign artifact reads as gone, markdown cap, markdown id checks, stored diff artifact), 8 in `change_routes::record_tests` (new baseline keeps the id, unchanged saves nothing, change stores the JSON diff and names it in the record and webhook payload, pre-feature baseline gives an unavailable diff, cross-org / non-markdown / garbage id refused with nothing saved, flag off identical to before, ZDR-declared body keeps no text with the flag on, same text different bytes gives `real_delta` false), 2 in `change_webhook` (idempotency key, payload), 2 in `routes` (`markdown_artifact_id` present and omitted).
- quarry-edge integration, same feature: `change_diff_route` (new, real RS256 router: 501 off with the variable named, 403 ZDR, 400 bad queries, 501 store text), `zdr_feature_matrix`, `web_policy_routes`, `zdr_gap_rows`, `zdr_posture_stamp`, `zdr_refusal_modes`, `change_webhook_proptest` all pass.
- quarry-runtime `--features postgres-queue --lib`: 1100 pass, 1 ignored. The two new store tests (`load_baseline_is_org_scoped...`, `load_diff_and_find_diff_are_org_scoped`) return early without `DATABASE_URL`: they did NOT exercise the SQL.
- `cargo check -p quarry-edge --tests` with default features and with `postgres-queue` (bins included) compile. `rustfmt --check` was clean on the new files after formatting.
- Go: `GOWORK=off go test -count=1 ./...` in quarry-orchestrator all pass (3 new in `internal/activities/markdown_artifact_test.go`: forwarded, omitted when none, RunPage decodes it).
- Node: `node --test scripts/tests/*.test.mjs` 61 tests, 49 pass, 0 fail, 12 todo (pre-existing); includes the openapi route coverage (count re-pinned), no-bare-null and internal guards, and the new docs-contract test.

## Deviations from the earlier plan in this file
- The store is behind a trait (`ChangeStore`) in the edge so the record and read logic are tested without Postgres.
- Reason set gained `no_previous_version` (one baseline), and `from` is nullable.
- The read route refuses ZDR callers entirely (the plan allowed metadata-only). Owner decision below.
- `diff_paragraphs` was changed (linear lookup, `moved` by longest increasing run) and `paragraph_chunks` fixed; both are in quarry-transform, outside the item's named files, because the feature cannot be correct without them.
- No compose line for the flag: unset is off, and a docs-contract test fails if a compose turns it on.

## Not verified
- The SQL of `load_baseline`, `load_diff`, `find_diff` against a real Postgres (tests skip without `DATABASE_URL`). The `ChangeStore` impl for `PostgresBaselineStore` only delegates.
- The end-to-end flow (schedule a change_monitor on a fixture page, edit a paragraph, see `change_detected` with `diff_id`, read `GET /v1/change/diff`) on a side-by-side stack. Not done: no Docker stack, Temporal or Postgres was started.
- The SDKs (`sdks/`) were not regenerated from the new openapi (needs the generator in Docker); `spa-verify` or the SDK job will show the diff.
- Behaviour with a retried CheckChange activity (duplicate baselines, see depth file) and with the janitor armed.
- Clippy was not run.

## Owner decisions needed
1. Read route under ZDR: refuse entirely (built) or allow metadata only (fingerprints and ids, no text).
2. Enabling the flag on any deployed edge (live flags are not touched here). Also: deploy edge before orchestrator.
3. Arming the artifact janitor on a stack that uses text diffs (it deletes baseline text by age) and org erasure for baselines: both are open, see RUNBOOKS.
4. The route-count pin in `scripts/openapi-route-allowlist.json` (108 to 109) will conflict with the other lanes' route additions at integration; re-pin with `ROUTE_BASELINE_UPDATE=1`.
