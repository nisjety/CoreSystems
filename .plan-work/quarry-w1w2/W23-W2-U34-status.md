# W2-U34 status: WS7.G 6b durable-job policy snapshot end to end

Lane a, branch `w23/lane-a` (fast-forwarded to `w23/integ` first). Plane: Ingestion (edge and orchestrator).

## Built

6a (the backstops) was already on the branch (W2-U34-progress.md, tests written in 2026-10-24 and `web_policy_backstop.rs` already existed). Built the gap: 6b, and ran the 6a tests for the first time.

- quarry-core `web_policy/mod.rs`: `ResolvedPolicy::snapshot_json()` (v1 document: non-neutral modes, robots/proxy ceilings, allow lists, deny hosts, informational org_rev; host names and closed labels only, never a URL), `PolicyLayer::from_snapshot_json()` (strict: unknown field, other version, unknown capability, forced ability, bad host, more than 16 allow lists are refused as `SnapshotError`), 5 unit tests.
- Edge handoff (`routes.rs`): `stamp_policy_snapshot` puts `params.policy_snapshot` on the crawl and the batch job when the flag is observe or enforce and the resolved policy differs from the catalogue default (a default policy stamps nothing, so jobs without a tightening are byte-identical). It stamps the policy `enforce` would apply (`shadow` under baseline observe).
- Edge page (`routes.rs`): `InternalRunPage.policy_snapshot`; `apply_policy_snapshot` rebuilds the layer and re-decides with it as the request layer (tighten-only meet). enforce: the page runs under the met policy (and, being non-default, neither reads nor writes the page cache). observe: page unchanged, counters `web_policy_snapshot_tighter_total` and `web_policy_snapshot_would_deny_total` (no labels). off: ignored. An unreadable snapshot is a 400 `invalid_policy_snapshot` in observe and enforce.
- Run binding (`internal_auth.rs`): `RunBindingExtras`, `verify_run_binding_with`, `run_binding_sig`, `canonical_json`, `snapshot_sha256_hex`. The canonical string is `org\nurl` plus `\npolicy=<sha256 hex of canonical snapshot>` only when a snapshot is present (no snapshot: byte-identical to before). Verified whatever the flag says, so the binding holds before anything uses the snapshot; a snapshot cannot be added, swapped or stripped without the secret. WS14.S7a extends `RunBindingExtras` / `runBindingSigExtras` with a `robots` line after `policy`.
- Flag (`policy_baseline.rs`): `QUARRY_EDGE__WEB_POLICY_SNAPSHOT` = off | observe | enforce, default off, strict parse (bad value fails boot), `PolicyBaseline::snapshot_mode()`.
- Orchestrator: `RunPageInput.PolicySnapshot` (omitempty), `runBindingSigExtras`, `canonicalPolicySnapshot`, `policySnapshotSHA256` (`activities.go`); `CrawlJobInput` and `BatchJobInput` carry `PolicySnapshot`, read from `params.policy_snapshot` in `CrawlInputFromParams` / `BatchInputFromParams`, sent with every page through the new `runPageSnapshot` (`workflows.go`, `inputs.go`).

## Tests (all run, all pass)

- `cargo test -p quarry-core --lib web_policy` (qtest lane a): 7 passed (5 new).
- `cargo test -p quarry-edge --lib -- internal_auth policy_baseline`: 20 passed (5 new: canonical json, Rust half of the golden, no-extras equals the two-field string, add/swap/strip, flag parse).
- `cargo test -p quarry-edge --test web_policy_backstop`: 18 passed (5 existing 6a tests, run for the first time, plus 13 new 6b tests: enforce deny and allow, cannot loosen a baseline, signature covers add/swap/strip, binding checked with the flag off, off ignores, observe never refuses, unreadable snapshot is 400 (off ignores), null snapshot, crawl and batch stamp, off stamps nothing, default stamps nothing, stamped snapshot binds an edge with no baseline).
- Regression, same lane: web_policy_routes 11, crawl_scope_robots 16, batch_handoff_check 3, crawl_options_off 3, crawl_options_on 7, zdr_posture_stamp 13, web_policy_claim 12, policy_baseline 6: all passed. The edge bin target compiles (the old `scrape_actions` E0433 is gone).
- Go (host, GOWORK=off): `go test ./...` in quarry-orchestrator all ok; `go vet` clean. `go test ./internal/activities`: new `TestPolicySnapshotGolden` (canonical JSON, sha256, HMAC golden), `TestRunBindingSigExtrasWithoutASnapshotIsTheTwoFieldString`, `TestRunPageCarriesThePolicySnapshotAndSignsItsHash`, `TestRunPageRefusesASnapshotThatIsNotJSONWithoutCallingTheEdge`. `go test ./internal/workflows`: new `policy_snapshot_test.go` (params to inputs, absent/null/empty/wrong, pre-snapshot input decodes, every crawl page on both frontiers and every batch page carries it).
- Two-sided golden: the same snapshot document, canonical string, sha256 `eac47139...782c` and HMAC `e85e8d3b...888a` (secret `0123456789abcdef0123456789abcdef`, org_a, `https://example.com/pricing`) are pinned in `internal_auth.rs` and `activities_test.go`; the values were first computed with an independent Python implementation.

## Flag and default

`QUARRY_EDGE__WEB_POLICY_SNAPSHOT`, default off. The web-policy `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` mode still decides whether baseline layers bind at all.

## Assumed decisions

- Tri-state flag (off, observe, enforce) because the queue says "default off (observe)".
- The snapshot is a PolicyLayer-shaped document (not the full ResolvedPolicy and not a digest), because the page-level policy must meet it; a default policy is not stamped.
- No GetVersion marker: the snapshot rides only in the page activity's input and adds no workflow command, so old histories replay unchanged (same reasoning as INNH-6b `scrape_options`). Recorded as D2-U34-7.
- Signature extension is a `\npolicy=<hex>` line appended only when present, and it is verified regardless of the flag.
- The snapshot is stamped from the handoff principal's decision (the caller's, with the org claim), and the page re-resolves as Autonomous with no org layer plus the snapshot as the request layer.
- Egress receipts: decision is that under ZDR a receipt stores the host only, not scheme/host/path. NOT built (see Not verified / D2-U34-5).
- Files outside the listed scope were touched, because the scope cannot be built in the listed files alone: `quarry-core/src/web_policy/mod.rs` (the only place that can read `allow_terms`), `quarry-edge/src/policy_baseline.rs` (flag; per-edge state keeps tests independent of process env), `orchestrator workflows/workflows.go` and `inputs.go` (params to input to page), new `workflows/policy_snapshot_test.go`. `crawl_request.rs` was not touched.

## Model Plane asks

None.

## Not verified

- Egress-receipt host-only redaction is a decision only (chromiumoxide.rs belongs to W2-U52/U07 this batch; `BrowserEgressReceipt.url` is a contract field).
- No live stack: not run against a real Control (job params storage of `policy_snapshot` is assumed free-form JSON, as with `scrape_options`) or a real Temporal.
- Full quarry-edge and quarry-core suites were not run, only the files named above.
- The internal OpenAPI, RUNBOOKS and ERROR_REASONS were not updated (D2-U34-4).
- Scheduled crawls get no snapshot (D2-U34-2).

## Depth rows

D2-U34-1 to D2-U34-8 in W23-depth-lane-a.md.
