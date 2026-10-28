# Space depth pass A: fix order, and the UI parity track (2026-10-28)

Updated later on 2026-10-28 (items 2, 3 and 4 fixed, U2 done). Inputs: `space-w1-report.md` (clusters), `space-depth-register-w1.md`, `space-w1-live-proof.md`, and a code check of each item below. Nothing here starts Wave 2. Branch `space`, nothing pushed.

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
| 5 | Boot guards from the live proof: F10 (convex-core image bakes `startup.sh`) and F11 (a decision key id mismatch shows up as a generic 403) | live-proof findings | S | Both cost hours on the private stack. Make a key-id mismatch a loud boot error in session, capability and orchestrator cores; add the convex-core rebuild to the deploy checklist. |
| 6 | F3: reverting 0044 fails once 0072 is installed (its triggers depend on `runs.retention_class`) | DB tier; `revert/0044` drops the column, `revert/0072` already exists | S | Rare path, but a rollback that fails when needed is dangerous. Make the 0044 revert drop the 0072 triggers first (or document and test the order). |
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
| No time beside a message (only the card footer has a date) | all three | yes: transcript turns carry no `created_at` | U3, needs a gateway/session-core field |
| Same author, consecutive turns repeat avatar and name | Slack, Teams group them | no | U3 |
| First post of a card showed a generic chat icon, replies showed initials | consistent initials avatars | no | Done (U2) |
| No "new messages" divider, only a per-card "Ny" badge | Slack, Teams | no | U3 |
| Header has no member avatars or count; editing the name sits in the header | Slack, Teams: member stack, details panel | no | U4: member stack, move edit into a details panel |
| Composer has no toolbar | Slack, Teams: format, attach, emoji, mention | attach and emoji: yes (WS12, WS14) | Out of scope now; mention and slash exist |
| No reactions, edit or delete on a message | all three | yes (WS12) | Wave 3 track; not started |
| No DM section in the room list | all three | yes (WS12) | Wave 3 track; not started |
| Agent activity: strip, pulse card, inline working row | Grok Bot | no | Already close; U2 can put the agent's plan and approval cards in the thread pane |
| On a phone the shell opens its sidebar drawer over the room on first visit | Slack and Teams show the room | no, but it is the shared shell | Logged; the default should be closed on a room route |
| Dark mode and the 4 other tabs not yet reviewed | - | no | U4 |

Order: U2 is done; U3 (time, grouping, divider, needs the `created_at` field) before U4 (header and details). Disk note: C: is at 98% (21 GB free); Docker's build cache holds about 21 GB that `docker builder prune` would free, and the worktree cargo targets (`C:/dev/space-target-*`) are large. Reactions, edit/delete, attachments and DMs stay with WS12 to WS15, which are not Wave 1.
