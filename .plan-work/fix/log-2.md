# Fixer F2 log: segment 2 (plan lines 417-946, `fix/seg2.md`)

Method: edits applied by a scripted exact-substring replace (every old string matched exactly once, original kept in the scratchpad). Provenance re-run for every new or corrected path (all as tagged in the segment): `agent_routes.rs` [C], `chat/shared.rs` [C], `zdr.rs` [C], `chromiumoxide.rs` [C], `test_support.rs` [M], `cache.rs` [C], `cache_policy.rs` [C], `routes.rs` [M], `main.rs` [M], `config.rs` [M], `browser_run.rs` [M], `model-gateway/src/quarry.rs` [M], `quarry_agent.rs` [C], `browser_agent.rs` [C], `buf.gen.go-only.yaml` [C], `mp-contracts/build.rs` [C], `browser_pb2.py` [C], `Frontend Plane/verevon/src/lib/model-plane/gen` [C], `server_test.go` [C], `latency_test.go` [C], `pg.go` [C]; `ERROR_REASONS.md` MISSING so [NEW]; `tools/sync-protos.sh` MISSING (the plan now says it does not exist). No proposed correction was left unresolved, so no [UNVERIFIED] was added for a path.

## Skeptic K1

| id | status | note |
|---|---|---|
| K1-01 | applied | WS2.D config: one pair `MAX_CONCURRENT_RUNS` 32, `QUARRY_BROWSER_MAX_SESSIONS` 40, heavy lane 1, "Defaults conflict" sentence deleted, boot check WARN-only until RSS recorded then fails; WS2.D evidence (6) sets `MAX_SESSIONS=48` in the test env; WS3.F env default 12 to 40, R-26 sentence, DoD now states the 13th acquire with `MAX_SESSIONS=12` set in the test env; WS2.D TTL-ordering boot check also WARN-only. SC3, 4.5, flag table (3427/3432/3471) and SC3 evidence are out of range. |
| K1-02 | applied | WS1.A files (contract overlay may only set env and volumes, loaded after both compose files), WS1.A2 DoD and evidence (production overlay plus contract overlay, WS3.A proof recorded on the same stack), WS1.B evidence, WS1.A Depends line (WS3.A dependency of the SC1 L2 run), WS3.A state of code and DoD. SC1 text, W1 gate (3361) and 6.4 (3624) out of range; the W1 gate in 3.1 intro (line 425) now says production overlay plus contract overlay. |
| K1-03 | out-of-range-skipped | SC7 (83), WS6.A (993-1002), 3688, X-10 all outside 417-946. |
| K1-04 | out-of-range-skipped | SC10, X-19, S2 outside. |
| K1-05 | applied | WS2.C flag paragraph and evidence: SC2a with `TAKEOVER_ENFORCEMENT=1` named on the evidence stack, SC2b is WS11.D (wave 2). SC2 text, 3478-3479, 3424 out of range. |
| K1-06 | applied-modified | The reviewer offered two alternatives; I took the first (WS5.C moves from optional to gated and into the W1 exit gate; gated 16, optional 6). Edits: WS5.C slice row, WS5 header effort, WS5.A flag text, WS5.B contracts, 3.1 intro totals and exit gate. SC6, 3361 and exit evidence 5 (3482) are outside: those fixers must add WS5.C to the gate or the two segments disagree. |
| K1-07 | out-of-range-skipped | X-3 (212-216), 4.3 rows, 42 outside; the segment contains no "disjoint" claim. |
| K1-08 | out-of-range-skipped | 7.2 / PR 2 and PR 3 outside. |
| K1-09 | applied (range part) | W1 exit gate in the 3.1 intro now lists "PRE-1 and PRE-2 committed". 7.2 rule, PR 3 and next candidates (3671, 3701) outside. |
| K1-10 | out-of-range-skipped | 3694, 3363 outside. |
| K1-11 | applied | WS1.A split into WS1.A1 (broker half, no Control Plane edit, no X-1) and WS1.A2 (registry row, manifest row, Quarry service-token path, `grant_validator.rs`, G6, boot gates, agent-route callers, overlays, L2 legs A to C; only A2 waits for X-1). Slice table (three rows with A0), header, Depends and Gated by, Files paragraph (marker "WS1.A2 starts here" plus [A1]/[A2] labels), Contracts, Flag, Tests ownership, DoD, Evidence, Risks; WS1.B and WS1.C table rows depend on WS1.A1; WS2.B freeze text names WS1.A2; rollback text names the WS1.A1 image. 2.1, 2.4 B, 4.3 position 1 are out of range. |
| K1-12 | applied | WS4.H files: refuse only `POST/PUT/PATCH /v1/profiles*`, list and delete stay allowed; test `zdr_org_can_list_and_delete_profiles` added to the WS4.H tests. |
| K1-13 | applied | New slice WS4.K (0.5 d): table row, header effort 27.25 d, Resolves, Free to start, state-of-code bullet rewritten, full detail section (files `cache.rs`, `routes.rs` callers, `tests/cache_policy.rs`, tests `page_cache_is_not_readable_across_orgs` and `page_cache_key_includes_the_org`, DoD, evidence, risks); in the W1 exit gate. SC5 addition is out of range. |
| K1-14 | out-of-range-skipped | R-11 (347), B8, ledger row outside; no event-type numbers 160-162 occur in the segment. |
| K1-15 | out-of-range-skipped | WS13.F, X-20 outside. |
| K1-16 | applied-modified | WS1.A 16 d (A1 10 + A2 6, plus A0 1.5 d as a separate slice), WS2.C 10 d, WS3.F 10 d; headers, slice rows and totals updated (WS1 33.5, WS2 22 / 21, WS3 48 / 43, Wave 1 A to J 40 d). W1 core now 132.25 d: 113.75 + 1.5 + 6 + 4.5 + 4 + 0.5 + 2 (WS6.A 1.5 to 3.5, counted per K2-12 though its slice sits in another segment). The reviewer's "about 140 d" is a loose figure; 4.4 and the weeks estimate are out of range. WS6.A 3.5 d must be applied by the segment 3 fixer or the total is off by 2 d. |
| K1-17 | applied (WS4.H) | WS4.H rollout now says Quarry reads posture only through the narrow `org:retention:read:any` GET at fire time (X-9). WS6.D DoD and R-13 are outside. |
| K1-18 | out-of-range-skipped | ADR-1 (283) outside. |
| K1-19 | applied | The [UNVERIFIED] gap notes in WS1.C risks and WS4.J replaced by the verified statements. Item (d) of 7.1 is outside. |
| K1-20 | applied (no occurrences) | Searched the segment for `src/tests`, `src/src`, `src/../tests`: none in 417-946 (the affected lines are in 134-135, 1014, 1066, 3282+, 3542+). |
| K1-21 | applied | WS4.G: matrix generated from the Rust `ROWS` table, completeness enforced by `every_registered_route_has_a_matrix_row`, new `log: Canary{test} \| LogUnverified` field and test `every_row_has_a_log_canary_or_is_marked_log_unverified`, DoD wording. SC5 is outside. |
| K1-22 | out-of-range-skipped | 4.1 table outside. |
| K1-23 | out-of-range-skipped | Flag table (3451) outside. |

## Skeptic K2 (range 417-946)

| id | status | note |
|---|---|---|
| K2-01 | applied-modified | New slice WS1.A0 (S, 1.5 d, table row plus full detail section: production overlay gets `QUARRY_EDGE__ARTIFACT_BACKEND` and `QUARRY_EDGE__DURABLE_EVENT_HISTORY: "1"`, DoD `up quarry-edge` reaches `/ready` 200 with `ENVIRONMENT=production`); owning slice named (WS1.A0 owns the overlay boot keys, WS3.A the writable Chromium root); state-of-code bullet with the code evidence; contract overlay env list as in the reviewer text, plus the sentence that after WS1.A0 it shrinks to the validator URL, the dev-bypass flags and the volume (to agree with K1-02 and with the reviewer's own DoD); "boot spike recorded" replaced by "production overlay boots (WS1.A0)". Modified: A0 is priced as a separate 1.5 d slice on top of the 16 d (K2-17 had it inside 15 d). Also flagged [UNVERIFIED] whether a placeholder validator URL boots. |
| K2-02 | applied | WS1.A2 files: `EXPECTED_REQUESTS` to seven rows, the `"browser-broker"` arm at `service_tokens.rs:805`, `token_request_contract.rs` `expected_path` arm, `endpoint`; test name `the_shipped_manifest_is_accepted_unchanged` added to the Quarry tests. |
| K2-03 | applied | WS1.B files (five test literals plus preferred `AppState::test_default()`), WS4.A contracts (six literals with corrected lines). WS6.F, WS8.D, WS8.I outside. |
| K2-04 | applied | WS1.A1 files: Go-only `buf generate --template buf.gen.go-only.yaml` in the container, stale Python and TS trees registered, Rust by `build.rs`, dead `tools/sync-protos.sh` reference deleted, L1 vendored-subset assertion added; WS1.B points to it. WS6.E, WS7.K, WS8.C outside. |
| K2-05 | applied | WS4.G and WS5.H route scans: named files only, `#[cfg(test)]` modules cut, `agent_routes.rs` or `agent_routes/router.rs`, `ROUTE_BASELINE_UPDATE=1`, tests `route_scan_ignores_cfg_test_modules` and `route_scan_follows_the_agent_routes_split`; WS5.H "[UNVERIFIED count]" removed (85 verified). |
| K2-06 | applied | WS2.A (test uses `crate::test_support::insert_live_run`, `test_support.rs` listed), WS2.C and WS2.D file lists (`insert_live_run`, no `tests/*.rs` may construct `RunEntry` or `RunHandle`). |
| K2-07 | out-of-range-skipped | WS6.D, WS6.H outside. |
| K2-08 | applied-modified | WS2.C Go mirror: five missing codes with their statuses (409, 403, 503, 409, 409 read from `error.rs`) plus `USER_TOOK_OVER` 409, each with a `case`; state-of-code says 12 of 17 and the 500 default arm; `grpc.rs`/`tools.rs` clauses rewritten in WS2.C, the ErrorCode mirror list and the WS2 cross-session B6 line; `tools.rs` default arm 1184-1189. Modified: `UNSUPPORTED_MEDIA` 415 and `BUDGET_EXHAUSTED` 402 are left to WS5.A and WS6.B under R-10(a) (WS5.A already lists the Go case); adding them in WS2.C would put Go codes ahead of the Rust enum. The reviewer's six-number status list covered eight codes; I wrote the full explicit list. WS6.B (1014) outside. |
| K2-09 | applied (range part) | `event_type_token` 394 to 364 in WS2.D. The wrong paths and the other line lists are at 1000-1156 (outside). |
| K2-10 | applied (range part) | WS1.C and WS4.J notes replaced; 1088 and 1381 outside. |
| K2-11 | applied | nine to ten in WS5.A tests and WS5.E. |
| K2-12 | applied-modified | `launch_count()` accessor added to WS3.C files; WS3 header (Wave 1 A to J 40 d, 35 without G, WS3.K 8 d Wave 2; the reviewer's 36/31 become 40/35 because WS3.F is now 10 d); WS6.A counted in the W1 total and gate (now 3.5 d, see K1-16). |
| K2-13 | applied (range part) | WS2.A evidence: mint recipe `curl ... /api/quarry/token` and the service-principal route. WS7.C and WS7.D outside. |
| K2-14 | out-of-range-skipped | WS7.C outside. |
| K2-15 | applied | WS1.A1 and WS1.B files: `TestBrowserBroker` implements new RPCs as `Unimplemented`; `server_test.go` and `latency_test.go` use `grant.NewMemoryRepository()`. |
| K2-16 | out-of-range-skipped | WS6.E, WS7.K outside. |
| K2-17 | applied-modified | WS1.A effort is 16 d (K1-16) split A1 10 + A2 6, A0 1.5 d on top; WS1 total 33.5 d, not the reviewer's 31 d. |

## Skeptic K3 (range 417-946)

| id | status | note |
|---|---|---|
| K3-01, K3-02, K3-03, K3-04, K3-05, K3-06, K3-07, K3-09, K3-11 to K3-22 | out-of-range-skipped | All locations are 1572 or later. |
| K3-08 | applied-modified | Not the WS14/WS16 rows (outside). In range, WS5.A's "avoids a 17th `AppState` literal" contradicted the verified count; it now names the four full literals (`main.rs:1426`, `test_support.rs:96`, `tests/support/mod.rs:41`, `routes.rs:1301`). |
| K3-10 | applied (in-range occurrence) | R-31 (367) and the WS12, WS13, WS17 lines are outside; the WS3 merge-order paragraph is a fifth statement of the same queue and now reads the one line "WS3.C, WS3.D, WS3.F, WS3.K, WS3.G, WS8.E, WS12.B, WS13.C, WS12.F, WS17.E (C1), WS17.G (C2)", preceded by WS3.A's two-line guard. |

## Path checkers

| row | status | note |
|---|---|---|
| C2-A (576 `live.rs`) | applied | Full path `agent_routes.rs` [C] 949 and 965, moves to `agent_routes/live.rs` [NEW] under WS2.B. |
| C2-B (808 `chat/shared.rs:111`) | applied | Call site of `org_zdr_enabled`; `request_zdr` defined in `zdr.rs` [C]. |
| C2-C (881) and C2-D (921) | applied | nine to ten. |
| C2-E (614) | applied | Attributed to `chromiumoxide.rs` 121-122. |
| C2-F (586, 606, 691 `ERROR_REASONS.md`) | applied | Full path with [NEW] at each mention (WS2.D DoD, WS2 cross-session, WS3.F contracts); the first mention in WS2.D already had it. |
| C2-G (436 `SELF_HOST.md:60,130`) | applied | `:60` only, line 130 noted as already correct; docs-corrected list also `:60`. |
| C2-H (618 `slots.rs`) | applied | Knobs are read today in `chromiumoxide.rs`; `slots.rs` [NEW] will read them. |
| C2-I (515 `run_limits.rs`) | applied | [NEW], "will be read", created by WS2.C. |
| C2-J (661 `lease.rs`) | applied | Current text quoted, the quote labelled as the proposed replacement. |
| C2-K (434 `browser_broker.rs`) | applied | Mapping at 241-249, `_` arm at 248. |
| C2-L (514 `tools.rs`) | applied | 1184-1189, default arm 1189. |
| C2-M (834 `MountTeam`) | applied | 191. |
| C2-N (618, 685 `lib.rs` hunks) | applied | 42 and 160-163 (both mentions). |
| C2-O (855 `openapi.yaml`) | applied | 238-250, 4012-4143 (WS5 state of code and WS5.H). |
| C2-P (748 `fetch.rs`) | applied | "host-bearing lines". |
| C2-Q (455 `browser_grants.rs`), C2-R (546 `agent_routes/mod.rs`), C2-S (808 `cache_admission.rs`), C2-T (814 `log_field_discipline.rs`), C2-U (943 schema json) | applied | Tags [NEW] added; C2-U also "will be". |
| C1 (R-22 `browser_agent.rs` 742-745) | applied (in-range occurrence) | 749-752 and 768 in WS2 state of code. |
| C1 (R-23 `ReceiptBuilder` 453-556) | applied (in-range occurrence) | 455-557, struct at 457 in WS2.C. |
| C1 (R-10 "six missing codes", envelope.go range) | applied (in-range occurrence) | five missing, consts 31-47, `HTTPStatus` 50-73 (WS2 state of code, WS2.C). |
| C1 (zdr-contract-sync "+95-line diff") | applied (in-range occurrences) | "+93/-2, 95 lines touched" in WS1 state of code, WS4 state of code, WS4.G. |
| C1 (111/201 `stealth.rs` `lib.rs:164`) | no change needed | WS3.E already removes `pub mod stealth;` at `lib.rs` 164. |
| C3, C4, C5, C6, C7 | out-of-range-skipped | Scopes are 947 and later. |

## Extra fix, not from a finding
- WS5.D said "the seven existing tests 78-284" of `tests/cache_policy.rs`; the file has four `#[tokio::test]` functions (lines 78, 139, 200, 265). Changed to "the four existing tests (lines 78-284)".

## Cross-segment items other fixers must match
- WS5.C is now gated and in the W1 exit gate (K1-06 first alternative); SC6, exit evidence 5, 4.4 totals must follow (113.75 core is now 132.25 here; gated 16, optional 6).
- WS6.A 3.5 d (K1-16) is counted in the 132.25 figure.
- WS1.A is now A0, A1, A2 everywhere; 2.1, 2.4 B, 4.3 position 1, the slice table and any "WS1.A" reference elsewhere must use the new ids (the 50-day critical path should name WS1.A1; A2 is off it because WS1.B depends on A1 only).
- Session and run pair 40 / 32 / heavy lane 1, boot check WARN-only until RSS recorded (K1-01): SC3, 4.5, the flag table and WS8 references must agree.
- SC2 split (SC2a with the flag set to `1` on the evidence stack, SC2b = WS11.D).

## New inconsistencies noticed, not fixed
- WS3.F "Definition of done" and evidence still describe the 12-session test setting; they are consistent only because the test env file sets 12 (stated now), but WS3.F's `docs/ERROR_REASONS.md` sentence says "sibling (b)'s and WS2.D's gateway mapping must list both" while K2-07 wants the gateway mapping for crawl codes in WS6 (outside).
- WS2.C text names `fallback_driver.rs` 587 as the mirror arm; C3 shows `error_code_short` is declared at 575 and 587 is an arm (minor, left).
- WS5.D says "WS13.F raises the layer to 115 s only when actions are enabled"; K1-15 (WS13.F) may change that clause.
- Seg 3 intro line "Wave 1 workstreams are WS1 to WS5" now also depends on WS6.A being in the W1 gate; the section 3.2 introduction (outside) should say so.

<!-- COMPLETE -->
