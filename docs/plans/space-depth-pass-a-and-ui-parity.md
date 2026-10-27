# Space depth pass A: fix order, and the UI parity track (2026-10-28)

Inputs: `space-w1-report.md` (clusters), `space-depth-register-w1.md`, `space-w1-live-proof.md`, and a code check of each item below. Nothing here starts Wave 2. Branch `space`, nothing pushed.

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
| 2 | People-search SQL: `searchPeopleSQL` never references `$2` but `callerID` is bound as `$2`, so Postgres cannot type it (SQLSTATE 42P18) | `people.go:108-192`; all 5 directory live tests fail with this error | XS | The "add a member" picker fails 100% on real Postgres; one-line fix (drop the bind, renumber). |
| 3 | `message_search` has no `post_kind` predicate: `m.role IN ('user','assistant')` returns human posts, and the recall flag `$9` only excludes ZDR threads | `message_search.rs:56-77` (cluster 3, now confirmed in code; callers still to check) | S | Plan M1-S1 requires that model recall returns zero posts. Add the predicate for recall mode, bind `post_kind` in the v1 digest (D1-073), test on Postgres. |
| 4 | Cluster 1, ZDR on Space writes: observe mode stores posts, two model-gateway store-routes unguarded, 8 routes without a surface-registry row | W1 report; live proof showed the ZDR happy path only | M | Privacy. The live tour covered posts, stream and artifacts; these are the remaining paths. |
| 5 | Boot guards from the live proof: F10 (convex-core image bakes `startup.sh`) and F11 (a decision key id mismatch shows up as a generic 403) | live-proof findings | S | Both cost hours on the private stack. Make a key-id mismatch a loud boot error in session, capability and orchestrator cores; add the convex-core rebuild to the deploy checklist. |
| 6 | F3: reverting 0044 fails once 0072 is installed (its triggers depend on `runs.retention_class`) | DB tier; `revert/0044` drops the column, `revert/0072` already exists | S | Rare path, but a rollback that fails when needed is dangerous. Make the 0044 revert drop the 0072 triggers first (or document and test the order). |
| 7 | F12: a same-org non-member gets 403 for a real room and 404 for a missing one on `context`, `threads`, `artifacts` | live proof (D and E probes) | M | Misses the M1-S6 "byte-equal 404" criterion. Severity is low: room refs are opaque random ids, so this only confirms a ref the caller already has. Needs an owner decision first (are org-visible rooms discoverable?). |
| 8 | Clusters 9, 2, 4, 5, 6, 10 as in the W1 report | W1 report | M each | After the above. Cluster 5 (production credential registry, owner question 6) blocks any staging flag. |

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
| Each post card repeats the whole thread inline, then the pane repeats it again | Slack: root message plus "N replies" and who replied | no | Next (U2): collapse a card to root plus reply summary, open the pane for the rest |
| No time beside a message (only the card footer has a date) | all three | yes: transcript turns carry no `created_at` | U3, needs a gateway/session-core field |
| Same author, consecutive turns repeat avatar and name | Slack, Teams group them | no | U3 |
| First post of a card shows a generic chat icon, replies show initials | consistent initials avatars | no | U2 |
| No "new messages" divider, only a per-card "Ny" badge | Slack, Teams | no | U3 |
| Header has no member avatars or count; editing the name sits in the header | Slack, Teams: member stack, details panel | no | U4: member stack, move edit into a details panel |
| Composer has no toolbar | Slack, Teams: format, attach, emoji, mention | attach and emoji: yes (WS12, WS14) | Out of scope now; mention and slash exist |
| No reactions, edit or delete on a message | all three | yes (WS12) | Wave 3 track; not started |
| No DM section in the room list | all three | yes (WS12) | Wave 3 track; not started |
| Agent activity: strip, pulse card, inline working row | Grok Bot | no | Already close; U2 can put the agent's plan and approval cards in the thread pane |
| On a phone the shell opens its sidebar drawer over the room on first visit | Slack and Teams show the room | no, but it is the shared shell | Logged; the default should be closed on a room route |
| Dark mode and the 4 other tabs not yet reviewed | - | no | U4 |

Order: U2 (feed model and avatars) before U3 (time, grouping, divider, needs the `created_at` field) before U4 (header and details). Reactions, edit/delete, attachments and DMs stay with WS12 to WS15, which are not Wave 1.
