# Fixer F6 log: seg6.md (plan lines 2925-3233, section 3.3 to 3.5, new 3.6)

Segment line n = plan line n + 2924 (before edits). Status: applied | applied-modified | rejected | out-of-range-skipped.

| id | status | note |
|---|---|---|
| K1-13 | applied | E3.8: the Quarry `PageCache` org key is now WS4.K (0.5 d, test `page_cache_is_not_readable_across_orgs`, merged with PR 1). Dependencies, scope ("slice 0" is a dependency on WS4.K), first slice, wave 3 sequencing "start now" list, E4.4 cache sentence and the 3.5 GAP.md line 61 row updated. F-t2-048 keeps the URL-keyed cache. |
| K1-01 to K1-12, K1-14 to K1-23 (except K1-13) | out-of-range-skipped | All locations are outside 2925-3233. K1-09 (PRE-2 becomes PR 3): E5.2 text (plan 3186 area) still says "a 1-day W1 fix PR is recommended", which stays true; not changed. K1-19 (migration gaps tolerated): no [UNVERIFIED] gap note exists in this range. K1-20 (`src/tests` paths): none occur in this segment. |
| K2-01 to K2-17 | out-of-range-skipped | Locations are 417-1571. Checked this segment for repeats: no `buf generate`, no `AppState` gateway literals, no "nine fixtures", no "11 of 17" claim. |
| K3-01, K3-02 | out-of-range-skipped | Locations 1625-2816 and 2242; no `src/tests` or `src/src` paths in this segment. |
| K3-03 | applied-modified | Listed locations (2466-2913) are outside the range; the same wrong file appears at plan 2984 (path-check row 3), fixed there to `quarry-control/internal/dispatcher/dispatcher.go:41` (not the `cmd/control/main.go` K3-03 proposes, because that file has no `http.Client`); second client `catalogwatch.go:64` added. |
| K3-04, K3-05 | out-of-range-skipped | Epic text in this range does not repeat either (E3.x to E5.3 have no `getByRunId` or `ingestions.run_schedule_action` text). Nothing to change; the other fixers' edits stand. |
| K3-06 | applied-modified | E3.5 and E3.7 test lines now say the Go tests run only in the report-only `go-coverage` job until WS14.S0b adds the hard `go-test` job. Depends on the segment-5 fixer adding WS14.S0b. |
| K3-07 | applied | E3.2 notification-core migration is now `016_register_approval_requested`; 3.3 header lists notification-core 015 (WS15.S4) and 016 (E3.2), citing R-32 (added by the segment owning R-1 to R-5). |
| K3-08 | applied-modified | WS14.S4 and WS16.S6 rows are out of range; the only `AppState` field addition in this range is E4.5 part 2 (quarry-edge backend registry), where the four full literals (`main.rs:1426`, `test_support.rs:96`, `tests/support/mod.rs:41`, `routes.rs:1301`) are now named. |
| K3-09 to K3-13, K3-15, K3-17 to K3-20, K3-22 | out-of-range-skipped | No matching text in this range (K3-11 `max_pages`, K3-13 `AI_FORMATS_ZDR` not mentioned in E3.x to E5.3). |
| K3-10 | applied | E3.3 dependencies and the wave 3 hotspot table now cite the single `chromiumoxide.rs` queue of R-31 verbatim: WS3.C, WS3.D, WS3.F, WS3.K, WS3.G, WS8.E, WS12.B, WS13.C, WS12.F, WS17.E (C1), WS17.G (C2). |
| K3-14 | applied | E3.3 scope: WS11.S0 owns the `TrustClass::classify` change (provenance.rs and the hand-mirrored `moderation.rs` 228-240 copy, test `trust_class_browser_mcp_tools_are_browser_scraped`); WS9.E and the epic consume it. Both files provenance C. |
| K3-16 | applied | Header of 3.3 and each epic: control `021_webhook_org_scope` (E3.5), `022_jobs_idempotency_org` (E3.7); capability-core `0017_procedures` (E4.2; 0016 stays WS7 outcome B; E3.7 R2 takes 0018+); session-core `0048` E4.6, `0049` only if E4.2 picks session-core; E3.4 quarry-runtime `0018`; all marked provisional and allocated at merge by the migration arbiter. Decision (b) in 3.4 changed from "session-core migration" to "capability-core migration". R-2/R-4 edits belong to the segment that owns them. |
| K3-21 | applied | E3.3 dependencies: WS9 slice 5 (WS9.I) for 038, slice 6 only if built (WS9.J), 038 does not wait for slice 6; wave 3 sequencing item (5) matches. |
| K3 gaps vs the mission | applied | New `### 3.6 Phases with no owner yet`: F-t2-046 phase B, F-t2-029 phases 2 and 3 (WS6 L1 to L3), F-t2-025 beyond WS6.F to H (L6, L7; one [UNVERIFIED] note), F-t2-012 slice 6b (WS9.K), F-t2-013 phase B (WS8.K), erasure of other quarry_edge tables; each with owner plane and first slice. 3.5 coverage cross-check now points to 3.6. |
| paths-C6 #1 | applied | `wait_out_pause_or_cancel` near 972 (verified `browser_agent.rs:972`). |
| paths-C6 #2 | applied | `action.started`: `event.rs:44` [C], `observation.rs:386-392` [C], `agent_routes.rs:969` [C, per reviewer, not reopened], rect type beside `InteractiveElement` `contracts.rs:462`. Provenance C for event.rs and observation.rs. |
| paths-C6 #3 | applied | `internal/dispatcher/dispatcher.go:41` [C] verified. |
| paths-C6 #4 | applied | Definition `store/pg/resources.go:80` [C], memory variant `store/store.go:576` [C] (verified line 576 is `memJobs.FindByIdempotencyKey`), call sites `resources/resources.go:425, 448` [C] verified. |
| paths-C6 #5 | applied | `model-gateway/src/quarry.rs` tag [C] to [M] (provenance M). |
| paths-C6 #6 | applied | `src/features/browser/*` tagged [NEW] at both occurrences in the range (E3.3 risks, E5.1 dependencies); provenance MISSING. |
| paths-C6 #7 | applied | audit-core claim reworded: GET pull plus POST ingest routes `api.go:111-115`, NATS subscribers `control-event-bridge/main.go:86`, `extra_nats.go:198-199` (provenance C; api.go lines verified at 111-115 in the file). |
| paths-C6 #8 | applied | Kernel and Browserless: compiled in quarry-browser (Kernel behind feature `kernel`, `lib.rs` 155-159 verified), unit-test only, not registered. Same fact corrected in the 3.4 disposition table row F-t2-059. |
| paths-C6 #9 | applied | 3.5 DRIVER_MATRIX row split per the matrix rows 64-66 (verified: Browserless has no live view, no recording, shared pool). |
| paths-C6 #10 | applied | E3.9 CI claim now includes the quarry-browser chromiumoxide check and test (verified `quarry-v2-ci.yml` 207-211). |
| paths-C6 #11 | applied | `egress_broker.rs` near 115 (verified line 115). |
| paths-C6 #12 | applied | `integration_tools.rs:54` constant, `truncate()` 875-880 (verified). |
| paths-C6 #13 | applied | lopdf wording fixed in E3.6 and in the 3.5 GAP.md line 85 row (`pdf.rs:31-32` verified). |
| paths-C6 #14 | applied | The note now names `docs/plans/quarry-browser-agent-loop-plan.md` [C] (provenance C; the unprefixed path is MISSING). Fixing `.plan-work/00-context.md` line 59 is outside my write scope. |
| paths-C1 to C5, C7 | out-of-range-skipped | No row falls inside 2925-3233. |

## New inconsistencies noticed, not fixed

- `.plan-work/00-context.md` line 59 still cites the missing path `docs/quarry-browser-agent-loop-plan.md` (outside my scope).
- 3.5 intro sentence "Every path and line below was opened in the live tree ()." has an empty parenthesis; left as is.
- E3.5 and E3.7 now cite WS14.S0b and R-32, which other segments must add (K3-06, K3-07); if they do not, those two references dangle.
- E3.2 scope (B) still says `approval_requested` rides migration 016 while R-1 to R-5 text in segment 1 lists no notification-core row until R-32 lands.
- `agent_routes.rs:969` (event wire name) was taken from paths-C6 and not reopened by me.

<!-- COMPLETE -->
