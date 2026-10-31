# W2-U33 progress (WS7.E, remaining routes: search, search/similar, extract, map, crawl, batch, profile create)

Status 2026-10-23: BUILT to the queue row's scope and committed. Tests written and compiled (cargo check through qtest); NOT run (build-first rule). Local commits only, nothing pushed. The optional ZDR consolidation PR is not built.

## Committed (git log is authoritative)
- `77ad12125` feat(edge): helpers in web_policy_edge.rs (paid_provider, record_downgrade, gate_urls). Its file copy was taken from a HEAD that predated the WS7.J commit c5cb15995, so it took back that commit's decide(&Claims) change in this file. Transient bad state on HEAD.
- `0f3dde7f4` fix(edge): restores web_policy_edge.rs to the WS7.J blob (53357b53d) plus the WS7.E helpers only. Net against 53357b53d: 87 added lines, 0 removed. Read this pair together with 77ad12125.
- `ebed40304` feat(edge): the route call sites (search, search/similar, extract, map, crawl, batch, profile create), openapi `SearchResponse.policy_downgrades`, RUNBOOKS WS7.E section. Its openapi and RUNBOOKS copies were taken from a HEAD that predated the WS15.L3 commit 568379c31, so it removed those lines.
- `629c1dd90` fix(docs): restores the WS15.L3 openapi and RUNBOOKS hunks from 568379c31 and adds only the WS7.E text. Net against 568379c31: openapi +8, RUNBOOKS +24, no removed lines. Read this pair together with ebed40304.
- `dd6d29f52` test(edge): `tests/web_policy_routes.rs` (new), seven decision-level tests.
- This progress file, `depth/W2-U33.md` and `wf-W2-U33.md` (docs(plan-work) commit that follows).

Code check against 568379c31: the route commit added only its own lines to extract_routes.rs (+26), map_routes.rs (+17), profile_routes.rs (+17), routes.rs (+23) and search_routes.rs (+73 / -1, the `let opts` line replaced by the decision block). No other unit's lines were removed from these files.

## Built, in the committed code
- web_policy_edge.rs: `PaidProvider` / `paid_provider(decision, requested, zdr)` (a downgrade, never a 403; ZDR is not a downgrade), `record_downgrade` (`web_policy_downgraded_total{capability,decision_source}`, log `policy.downgraded`), `gate_urls(decision, urls)` (first denial decides; details carry capability, decision source and `denied_count`; the host is removed).
- search_routes.rs: `SearchResponse.policy_downgrades` (skipped when empty); `search_run` decides and sets `opts.allow_paid_providers` from `paid_provider`; `similar` gates its seed URL before the fetch (it fetched an unchecked URL).
- routes.rs: `crawl_handoff` gates the seed after the durable-ZDR refusal and the drain check; `batch_handoff` gates every URL before the handoff.
- map_routes.rs: seed gated after the SSRF heuristic, before any fetch (403 FORBIDDEN, message only).
- extract_routes.rs: every target gated before the first fetch (403 FORBIDDEN as ApiError, message only).
- profile_routes.rs: `create_profile` (persistent scope) requires `browser.profile.persistent` from the policy; the ZDR refusal of creation stays with profile_guard.
- openapi: `SearchResponse.policy_downgrades` (additive). RUNBOOKS: section "Web policy on the remaining routes ... (WS7.E ...)".

## Not built, with reason
- ZDR consolidation PR (replace `claims.effective_zdr*` with `resolved.zdr()`): the plan marks it optional and last.
- Route-level tests (orchestrator double for crawl and batch, router-level search downgrade with provider telemetry, extract and profile 403 bodies through the router, the counter through a recorder): need the real-token harness of zdr_claim_floor.rs; test phase.
- Answer candidate filter: WS7.G.
- Save and update profile gates: not in the plan's named slice; depth.
- Deployment evidence (plan WS7.E section): not run.

## Verified (only what ran)
- `cargo check -p quarry-edge --lib --test web_policy_routes` through qtest on the shared working tree that carries this content: rc=0 (CHK_RC=0). Only the two pre-existing warnings (fleet_routes.rs:26 unused import; run_live.rs:406 deprecated fetch_update).
- `cargo check -p quarry-edge --lib --tests` (earlier attempt, same content): the lib compiled; the only errors were in `tests/policy_baseline.rs` (WS7.C test, 7 call sites `decide(&baseline, Principal::Human, ...)`), caused by the WS7.J signature change in c5cb15995, not by this unit. NOT fixed here (not our file); the WS7.C/WS7.J owner must change those calls to pass `&claims`.
- An earlier attempt failed in quarry-runtime (`crate::profile_blob_store` unresolved in cached_profile_store.rs, another unit mid-edit); it passed on the next attempt.
- NOT run: any test.

## Git hygiene (for the arbiter)
- Attribution: the commit messages end with `Co-Authored-By: Claude Haiku 5.5` (the system attribution reminder). The unit text asked for a Sonnet line; same resolution as the earlier W2 units.
- The HEAD-copy method failed twice under concurrent commits: a copy taken from HEAD at time T is stale if another unit commits before ours. Two fix-up commits (0f3dde7f, 629c1dd90) put back other units' lines. Next units: take the HEAD copy and commit in one command, then check `git diff --stat <base> HEAD` for deletions.

## For the next batch
- Gate helpers: `web_policy_edge::decide(baseline, &Claims, zdr, ask)`, `gate_url`, `gate_urls`, `paid_provider`, `record_downgrade`, `envelope_error`, `quarry_error`.
- Gated routes: search, search/similar, extract, map, crawl, batch, profile create. The sweep for any other URL-taking route (grep `Url::parse`, `fetch_markdown`, `driver.fetch`) was not repeated after `similar`; do it in the test phase.
