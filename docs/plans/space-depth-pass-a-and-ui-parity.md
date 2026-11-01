# Space depth pass A: fix order, and the UI parity track (2026-10-28)

Updated 2026-10-28 (items 2, 3 and 4 fixed, U2 done) and 2026-10-31 (items 5 and 6, F2, F4, the archive leg, part of cluster 9, U3, U4 and two visual rounds; section 4). Inputs: `space-w1-report.md` (clusters), `space-depth-register-w1.md`, `space-w1-live-proof.md`, and a code check of each item below. Nothing here starts Wave 2. Branch `space`, nothing pushed.

## 1. What is already fixed (commits c453d629c, 739b510ed)

| Item | Result |
|---|---|
| D1-651 post sender read the journal right after writing it (Solid 2 flush) | Fixed: a human post now reaches the API. 3 skipped tests run and pass. |
| D1-652, D1-662 connectivity banner wrote a signal in its body and halted reactivity | Fixed: watcher writes only on change and starts in `onSettled`. 8 skipped tests run and pass. |
| D1-674 `RegionBoundary` passed the error accessor to the card unread | Fixed: stale deploy, expired session and unreachable gateway now classify. 2 skipped tests run. |
| Avatar read "S(" for the viewer's own turns | Fixed with a regression test. |

Checks: 850 Space and shared-UI tests pass; typecheck shows only the 34 known errors in 3 untouched files. Not yet checked: a real browser against a real gateway (the live proof posted through the API, so it never exercised the composer).

## 2. Proposed fix order

Size: XS under an hour, S a few hours, M a day or more.

| # | Item | Evidence | Size | Why this position |
|---|---|---|---|---|
| 1 | Done: SPA blockers (above) | unit tests, screenshots | done | The headline flow could not send. |
| 2 | DONE (50be21db0). People-search SQL: `searchPeopleSQL` never referenced `$2` but `callerID` was bound as `$2`, so Postgres could not type it (SQLSTATE 42P18) | the 5 directory live tests failed with this error; now all 6 pass on Postgres, plus a DB-free test that every placeholder is referenced | XS | The "add a member" picker failed 100% on a real database. |
| 3 | DONE (db4a9c4b1), and smaller than first written. `SEARCH_SQL` had no `post_kind` predicate, but recall is confined to the owner's unscoped threads (room posts cannot exist there) and the only caller uses the UI purpose, so this was a latent gap for the planned recall tool, not an active leak. Recall now excludes `post_kind = 'post'`; UI search still finds posts | 16 message_search tests pass on Postgres, incl. a new one that seeds a turn and a post | S | Defence in depth for the recall tool and any future Space-scoped model search. Still open from cluster 3: bind `post_kind` in the v1 digest (D1-073), the in-tx gate's second-caller risk, and the member INSERT never run on Postgres. |
| 4 | DONE in part (see section 2a). Cluster 1, ZDR on Space writes: each row was checked in code; three of the eight were wrong or stale, two were real gaps and are fixed, the rest are owner decisions | section 2a | M | Privacy. |
| 5 | DONE (30ef4888d, c74a5245c, 1d32179a8; section 4). Boot guards from the live proof: F10 (convex-core image bakes `startup.sh`) and F11 (a decision key id mismatch shows up as a generic 403) | live-proof findings | S | Both cost hours on the private stack. Make a key-id mismatch a loud boot error in session, capability and orchestrator cores; add the convex-core rebuild to the deploy checklist. |
| 6 | DONE (30ef4888d). F3: reverting 0044 fails once 0072 is installed (its triggers depend on `runs.retention_class`) | DB tier; `revert/0044` drops the column, `revert/0072` already exists | S | Rare path, but a rollback that fails when needed is dangerous. Make the 0044 revert drop the 0072 triggers first (or document and test the order). |
| 7 | F12: a same-org non-member gets 403 for a real room and 404 for a missing one on `context`, `threads`, `artifacts` | live proof (D and E probes) | M | Misses the M1-S6 "byte-equal 404" criterion. Severity is low: room refs are opaque random ids, so this only confirms a ref the caller already has. Needs an owner decision first (are org-visible rooms discoverable?). |
| 8 | Clusters 9, 2, 4, 5, 6, 10 as in the W1 report | W1 report | M each | After the above. Cluster 5 (production credential registry, owner question 6) blocks any staging flag. |

### 2a. Cluster 1 (ZDR on Space writes), row by row

| Row | What the code showed | Result |
|---|---|---|
| D1-401 `POST /v1/threads/:id/presentation` | Session Core refuses a title or preview for a ZDR token (typed 412); a pin or persona change stays allowed, and a test pins both. The model-gateway handler is not the store. | Not a gap. Registry row reclassified `refuse`, basis `guard:update_thread_presentation`. |
| D1-401 `PUT /v1/memories/controls` | Session Core's `set_memory_controls` refuses a ZDR or waived credential (`memory_grpc.rs`). | Not a gap. Row reclassified `refuse`, basis `guard:set_memory_controls`. |
| D1-401 `POST /v1/internal/mail-task/prepare-thread` | CORRECTION of my earlier reading. Control already refuses: `IssueMailTaskThreadDecision` rejects an org whose effect policy has `ZeroDataRetention` (tested in `spaces` and `http`); I had only searched the `mailtask` package. What was missing was Session Core, which did not check a ZDR claim at all. The pipeline behind the route is not wired (`mail_task_pipeline` is `None` in every build). | FIXED. The signed decision now states `zero_data_retention:false` explicitly (Control also refuses to sign a true one), and Session Core requires the claim and refuses true or missing, with tests on both sides. Row reclassified `refuse`. Remaining, general to every Control decision family: user-core's effect-policy ZDR bit and org-core's interactive ZDR switch are separate sources and user-core never reads org-core's posture; whether provisioning keeps them in step is unverified. |
| D1-403 BFF durable writes | `PATCH /spaces/{ref}/instructions` stored free text in Convex with no ZDR check, and `POST .../agent-action-grants` (create) had none either, though the registry classed both `refuse`. | FIXED. A shared `zdr::durable_write_refusal` (typed 412, or 503 `zdr_decision_unavailable` when org-core cannot answer) now runs before any Control or Convex call; human posts use the same helper. Grant revoke stays allowed. Tests: refusal comes before the Control role read and the Convex write, an org-core outage is its own typed 503, reads still work, a ZDR create never asks Control for a decision while a revoke does. |
| D1-638, D1-639, D1-174 missing rows | The two human-post routes, the room details and member-role routes, artifacts, the event stream and the model-gateway events route had no registry row. | FIXED. Nine rows added (6 BFF, 3 model-gateway), golden re-vendored and re-pinned, and both "known gap" lists in the lints are gone, so a new unclassified route now fails outright. |
| D1-151 room Ask sends its idempotency key for a ZDR org | `send_receipt_id` returns none for a ZDR turn, a ZDR turn creates no thread, and a test covers it. | Not a gap. |
| D1-390 observe mode | `SESSION_CORE_SPACE_ZDR=observe` is the default: a ZDR-flagged Space decision proceeds on the legacy create and append, and only a counter notes it. The gateway guards posts and the model-gateway runs ZDR turns without a thread, so this is reachable only by a caller that presents a ZDR-flagged decision directly. | OPEN, owner decision SD-12 (flip to enforce after a clean counter week). Not changed. |
| D1-402 seven BFF rows with `min_role: unverified` | Metadata accuracy only; no behaviour depends on it. | Left open, low value. |

Two small things to fold in: F4 (a session-core test seeds a thread that breaks `threads_origin_space_consistency_chk`) and F2 (7 user-core http live tests need the policy outbox wired) are test-seed fixes, not product defects.

## 3. UI parity track (Teams, Slack, Grok Bot)

Method: the Space page is served from a dev server with every `/api` call mocked (`tests/visual/space-ui-shots.mjs`), then screenshotted at desktop, thread-pane and phone widths. Run it with `pnpm dev --port 5174 --strictPort`, then `node tests/visual/space-ui-shots.mjs <outDir>`. This worktree's dev server does not see file edits (restart it between shots), and Vitest needs `--pool=forks` here.

What the room looked like before, against the three references:

| Gap | Reference | Needs backend? | Status |
|---|---|---|---|
| Composer sat at the bottom of an 1,800 px page, off screen | Slack, Teams dock it | no | Done (U1) |
| Hero heading ate 120 px above the feed | none has one | no | Done (U1) |
| Header wrapped to 3 lines beside the thread pane; room name vanished on a phone | all | no | Done (U1) |
| Room pulse plus thread pane squeezed the feed to 480 px (three columns) | one contextual right pane | no | Done (U1) |
| Viewer avatar read "S(" | initials | no | Done (U1) |
| Each post card repeated the whole thread inline, then the pane repeated it again | Slack: root message plus "N replies" and who replied; Teams: collapsed replies | no | Done (U2): root message plus one reply-summary row (distinct repliers' avatars, count, latest reply, how long ago) that opens the pane |
| No time beside a message (only the card footer has a date) | all three | yes: transcript turns carry no `created_at` | Done (U3; `SessionMessage.created_at`, 313bed724) |
| Same author, consecutive turns repeat avatar and name | Slack, Teams group them | no | Done (U3) |
| First post of a card showed a generic chat icon, replies showed initials | consistent initials avatars | no | Done (U2) |
| No "new messages" divider, only a per-card "Ny" badge | Slack, Teams | no | Done (U3) |
| Header has no member avatars or count; editing the name sits in the header | Slack, Teams: member stack, details panel | no | Done (U4) |
| Composer has no toolbar | Slack, Teams: format, attach, emoji, mention | attach and emoji: yes (WS12, WS14) | Out of scope now; mention and slash exist |
| No reactions, edit or delete on a message | all three | yes (WS12) | Wave 3 track; not started |
| No DM section in the room list | all three | yes (WS12) | Wave 3 track; not started |
| Agent activity: strip, pulse card, inline working row | Grok Bot | no | Already close; U2 can put the agent's plan and approval cards in the thread pane |
| On a phone the shell opens its sidebar drawer over the room on first visit | Slack and Teams show the room | no, but it is the shared shell | Done for the first visit (U4); the drawer still stays open after tapping a room from the rooms index |
| Dark mode and the 4 other tabs not yet reviewed | - | no | Dark mode done (visual round 1); the other tabs only lightly reviewed |

Order: U2 is done; U3 (time, grouping, divider, needs the `created_at` field) before U4 (header and details). Disk note: C: is at 98% (21 GB free); Docker's build cache holds about 21 GB that `docker builder prune` would free, and the worktree cargo targets (`C:/dev/space-target-*`) are large. Reactions, edit/delete, attachments and DMs stay with WS12 to WS15, which are not Wave 1.

## 4. Pass of 2026-10-31 (commits 30ef4888d..29dc07790)

Built by one workflow (5 backend and 2 SPA builders, a reviewer per slice, two visual critic and fix rounds, one final verify), then committed by area. Nothing is pushed. Nothing here ran against a live stack: the UI was checked only in the mocked-API harness.

| Commit | What |
|---|---|
| 30ef4888d | F11 in session-core (unknown key id logged with the configured ids, counter `mp_session_space_decision_unknown_key_total`, fingerprinted boot log, boot fails on a set-but-unparseable key), F3 (0044 revert drops the 0072 triggers first), F4 seed |
| c74a5245c | F11 in capability-core and orchestrator-core, plus `scripts/check-control-decision-key.sh` (opt-in, not wired into `compose.sh`) |
| 1d32179a8 | F10: convex image staleness warning in `run-application-plane.sh`, runbook step |
| c1fd3c073 | F2: live tests build the Space repository with the audit outbox, as `main.go` does (no production defect); full user-core live suite green on Postgres 16 |
| 313bed724 | `SessionMessage.created_at = 12` through session-core and model-gateway (`buf breaking` clean) |
| 13472fa25 | D1-352 archive and unarchive routes behind `room_archive_api`; filtered Control refusals and a 30/min ceiling on member writers (D1-342); cluster 9 read and decision-relay fixes |
| 5b8247dce | U3 (times, grouping, divider, typed denials in send and transcript load, D1-258 and D1-259), U4 (header, details dialog, archive banner, BFF features replace the `VITE_*` flags), and the two visual rounds (dark mode, flat rows, composer, avatars, tablet sheet, feedback pill moved into the navbar menu on `/spaces`) |
| 29dc07790 | A human post to an archived room now answers `room_archived`, not `space_authority_stale` |

Checks (final verify over the whole tree): gateway 940 unit tests (was 926) plus integration; session-core 750 bin tests and the full DB tier (1043 pass, the 2 NATS tests need a broker); model-gateway all targets except `zdr_refusal_inventory`; the three Go modules; SPA 1779 of 1780 tests; typecheck at the 34-error baseline; contract sync 96 pass, 30 pre-existing.

Pre-existing failures, not from this work: `zdr_refusal_inventory` flags `browser_run.rs:802` (from the quarryv2-next merge); `CoreShell.test.tsx` expects a room href without `?thread_id=...#chat` (stale since b4556868e); `quarry-client.test.ts` cannot resolve `@quarry/client`.

Open items from this pass, fixed on 2026-11-01 (section 5) except where noted.

## 5. Open list closed (2026-11-01, commits 9ba5e7b64..bd51143a1)

One workflow (7 builders, a reviewer per lane, one final verify), committed by area. Nothing pushed, nothing live-proven.

| Item | Commit | Result |
|---|---|---|
| D1-109 human posts have no per-user ceiling | 9ba5e7b64 | 60/min per user per org shared by create and reply, typed 429 `rate_limited`, no upstream call when refused |
| D1-110 model-gateway errors relayed unfiltered | 9ba5e7b64 | Same filter as the Control relays; ZDR codes rebuilt from the catalogue; a model-gateway 401 becomes 503 so the SPA does not sign the user out (found by the reviewer) |
| D1-214 Control sends typed refusals as 503 | 029bc2c93 | `writeSpaceDenial` answers the denial's own status (403); only real outages stay 503. Gateway correction kept as a fallback, comments updated (bd51143a1) |
| Decision-key decoders disagree | 11f8a3475 | session-core, execution-core and sandbox-manager follow the Go rule; nothing is narrower than before |
| D1-242 no retrieval-decline reason | 616813441 | Closed-set `retrieval_decline_reason` BFF to model-gateway to a `retrieval_notice` and a one-line composer note. Thread create only, live stream only, not on the ZDR direct path |
| SpaceThreadPanel typed 403 | 3b4801dee | Presenter copy, generic notice when no reason |
| Phone drawer stays open after a room pick | 3b4801dee | Closes on navigation to a room at the 767px overlay breakpoint (the reviewer fixed a 759/767 mismatch) |
| Stale `CoreShell.test.tsx` | 3b4801dee | Expectation matches the committed deep link |
| Visual round 2 leftovers | 5d1f786a2 | 13.5px tabs, pulse hides zero tiles and lists recent activity, archive set apart as a danger action. The 20px jump did not reproduce; the reviewer's likely cause is text re-wrapping when the pane narrows the timeline at about 1280px |
| Stale generated types | 16813116b | sessions/runs/events TS and Python regenerated with the pinned plugins |
| Rooms sidebar has no archived marker | bd51143a1 | `/api/v1/spaces` entries gain `archived` from Control's index `state`; the sidebar sorts them last with an Arkivert tag |

Final checks: gateway 950 unit tests plus integration; session-core 755 bin tests; model-gateway all targets except the pre-existing `zdr_refusal_inventory`; user-core, capability-core, orchestrator-core and sandbox-manager green; SPA 2270 of 2270 tests (plus the new sidebar test); typecheck at the 34-error baseline; contract sync at its baseline.

Still open:
- The service-to-service Control resolvers (import-execution, schedule-fire, scheduled-run, scheduled-step, sandbox, watch-observe) still send typed refusals as 503, so capability-core's watch authorizer retries them as outages.
- The decline note is not shown on later turns (`space_append_context`), on a reopened thread, or on the ZDR direct path.
- Rust key decoders keep the strict trailing-bits check that Go does not have (only matters for a hand-edited key).
- Other generated TS and Python types are still stale (browser, capabilities, execution, gateway, inference, memory, orchestration, sandboxes, retrieval_v2; space_events and space_room have none).
- `zdr_refusal_inventory` flags `browser_run.rs:802` (pre-existing, from the quarryv2-next merge).
- The post limiter's Dragonfly path is untested here; only the in-process fallback ran.
- Owner decisions SD-12 and F12 are unchanged.

## 6. Section 5 leftovers (2026-11-01)

One workflow (6 builders, a reviewer each, one final verify). Not pushed, nothing live-proven.

| Item | Result |
|---|---|
| Service resolvers send typed refusals as 503 | Control's six service resolvers answer the denial's status with the typed body. orchestrator-core and execution-core callers stop retrying a 4xx (401, 408, 425 and 429 still retry). capability-core's watch authorizers were already right and gained tests |
| Decline note missing on later turns, reopen, ZDR | Appends carry the reason (and a declined append now reads with no retrieval authority). The answer stores `retrievalDeclined` (closed codes only, streaming and fallback completions) and the SPA shows it on reopen. The ZDR direct stream emits the notice |
| `zdr_refusal_inventory` on `browser_run.rs` | The ZDR browser-run refusal is now the catalogue's `feature_not_supported`, status 412 unchanged. The test passes |
| Dragonfly limiter path | 3 ignored live tests (`SPACE_LIMITER_REDIS_URL`) plus a default test against an erroring loopback server. The live ones have not run, because Docker was down |
| Rust key decoders strict on trailing bits | session-core and execution-core accept trailing bits and skip embedded CR/LF, as Go does |
| Other stale generated types | 8 TS and Python modules regenerated with the pinned tools. A new mp-events-py test checks the top-level types against the protos |

Final verify: gateway 972 (all targets); session-core 759; model-gateway 2413 plus the inventory test; execution-core 1450; Go modules green; SPA full suite 5351 of 5352. The failures are 3 pre-existing files: 2 from unlinked workspace packages, plus 1 more. tsc is at 34.

Still open:
- The live-DB test for the Control resolvers and the Dragonfly tests have not run.
- Control must be rebuilt before the callers' new classification has anything to act on.
- The capability-core cron sweeper and `taskexec` end a schedule on any authorization error, a Control outage included. This needs its own decision.
- imports-core (Python) cannot tell a 403 from a 503 on import-execution.
- Each append now makes one extra advisory Control retrieval-decision call. The clean fix is for `thread-append-decision` to return the verdict.
- A granted append still reads unscoped.
- The session-core key loader in `grpc.rs` still refuses whitespace inside a key.
- SD-12 and F12 are unchanged.
