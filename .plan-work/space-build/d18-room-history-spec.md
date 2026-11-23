# D18 — per-member room history visibility (owner decision, confirmed twice)

Owner's words: "they should be able to see conversation before they sign in. but we should have an invite
version where the user can select to not share past conversation, and in this version we let them in but
they can not see the past conversations, that means we hide them. the room admin can later change it so
they can see past conversations if that is need. with a date filter too."

## Behaviour
1. Default: a new member sees the room's whole past (as today with `history_mode = full`).
2. Invite option "don't share past conversation": the member joins, but everything posted before their
   join is hidden from them (their floor = their join point). The room's own `history_mode` is unchanged.
3. A room admin can later change one member's visibility to: (a) full history, (b) history from a chosen
   date (everything on/after that date visible, older hidden), or (c) back to join-only.
4. The canvas follows the same rule as the transcript for that member (no blanket refusal once a
   per-member window can be expressed for canvas content; see "Canvas" below).
5. Per-member setting overrides the room's `history_mode` for that member. Room `since_join` with no
   per-member row keeps behaving exactly as WS04-S11 does today.

## What the window covers (D21b, owner ruling 2026-11-19)
- The window narrows two surfaces only: the conversation (transcript, timeline reads, and the thread activity and attention built from them) and the canvas.
- Room files, knowledge retrieval and the sandbox workspace stay shared under D18. The window does not narrow them by join point or by date, and their access rules are unchanged. A room file posted before a member's floor stays visible to that member, even though the message that posted it is hidden. The window hides messages, not room files.
- Interpretation, pending owner confirmation (not part of the ruling): conversation-derived artifacts and room memory are not named in the ruling. Read fail-closed, they keep the window, including when retrieval returns them, until the owner rules otherwise.
- Interpretation, pending owner confirmation (not part of the ruling): an item that is not clearly one of the three shared surfaces is treated as conversation content and hidden from a windowed member (fail closed).
- Not yet implemented in code: the Data Plane retrieval engine (`retrieval-engine-rs`) still applies the member floor to document and wiki listings and retrieval, and Control user-core copies the member's floor onto every shared retrieval decision. This over-hides (it exposes nothing). Removing it is a code change tracked as D21b-CODE (dat, ctl), not part of this document; until it lands, do not record this ruling as implemented.
- Optional, not built (tracked as a spa item): the member-facing hidden-history note ("Earlier messages are hidden") may also say that room files stay visible.
- Owner to confirm: a pre-floor room file stays visible while the message that posted it is hidden. This follows from the ruling as written.

## Post-floor content and creation time (D21c, owner ruling 2026-11-19)
- Post-floor content that restates pre-floor turns is acceptable. A message created after a windowed member's floor stays visible to that member even when it quotes, repeats or summarises what was said before the floor. The window does not check content for that overlap.
- The window goes by creation time. For a per-member window (D18), a message created before the floor instant (`history_floor_at`: the member's join, or the chosen date) is hidden, and one created at or after it passes this floor. The code applies this as `m.created_at >= floor_at` (`message_floor_sql` and `floor_admits_at` in `apps/Model Plane/rust/services/session-core/src/space/audience_window.rs`).
- An edit does not move a message's creation time: the code writes revisions to `message_revisions` and leaves the message's `created_at` alone. So a message created before the floor stays hidden after it is edited past the floor. This follows from the ruling as written.
- The ruling covers post-floor content only. It does not make a pre-floor message visible to a windowed member, and a stored thread title or preview is not shown under a floor.
- The WS04-S11 since-join floor (`history_floor_revision`, on the audience revision) is a separate test, and this rule does not change it.
- Owner to confirm (not decided by the ruling, and not changed by this item): (a) room memory is judged by its message sources, so every linked message must exist and be at or after the floor, and a memory with no message link is shown only when it is the member's own save or share and was first written at or after the floor; (b) agent-scope notes (`scope = 'agent'`) are always hidden to a windowed member, because their text is made from the whole run context. Both rules are in `history_floor_admits_sql` (`apps/Model Plane/rust/services/session-core/src/space/memory_scope.rs`). Both can hide post-floor content that restates pre-floor turns, which the ruling accepts.

## Existing machinery to build on (do not duplicate)
- Control user-core `internal/spaces/history_window.go`: `history_mode` on `registered_spaces`,
  `applyHistoryFloor` derives `HistoryFloorRevision` from `space_recipient_audiences` snapshots and puts it
  on the Control-signed shared thread-read decision (`decision.go` `HistoryFloorRevision`), behind the
  `SPACE_HISTORY_WINDOW_CLAIM` rollout switch (a rollout control, never an authorization input).
- Session Core applies that floor on reads (`src/space/audience_window.rs`, `history.rs`, timeline reads).
- Canvas: Control `canvas_decision.go` refuses with `space_history_floor_unsupported` whenever a floor is
  present; wiki-store-go `internal/spacedecision/verify.go` has the matching `CodeHistoryFloor`.
- Known gap (user-core test pass, P1): `MembershipFeed` never applies the WS04-S11 floor to late joiners.
  Fix it as part of D18 (same floor source).

## Hard rules
- The window is a presentation narrowing of an already-authorised read: it may only HIDE, never widen any
  ACL. It must fail closed toward hiding where the code is new (a member with a hide-history row and an
  unreadable/absent floor must not be shown full history by new code paths).
- Every change to a member's visibility is a Control write with an audit row (Control owns audit),
  admin-only (room owner/admin role), and is a Control-signed decision input — never a header/body claim
  trusted by Session Core.
- Date mode: Control converts the admin's date into the floor Session Core already understands (first
  audience revision / post sequence at or after that timestamp) OR adds a timestamp floor field to the
  signed decision — pick whichever the existing Session Core read path supports with the smaller change,
  and record the choice. The claim must stay additive (old Session Core ignores unknown fields and that
  is the documented fail-open on a presentation rule from WS04-S11).
- ZDR: the setting stores no content (a mode + a timestamp), so it is allowed under every posture; say so
  in the registry row.
- New flags default OFF in production; never an authorization input.
- Every new route gets a `space-surface.v1.json` row (then re-sync both vendored fixtures + their
  `.sha256` sidecars + the golden's `.sha256` + the pin in `scripts/tests/zdr-contract-sync.test.sh` ~line
  235, and run that script to green).

## Split (three builders, in this order)
1. Backend (Control user-core + Session Core): migration for per-member visibility (+ revert), invite
   option on the existing invite/add-member route, admin route to change one member's visibility, floor
   derivation honouring per-member mode/date, MembershipFeed late-joiner fix, signed-decision field if
   needed, Session Core read path, audit rows, tests.
2. Canvas (wiki-store-go + Control canvas_decision.go): replace the blanket `space_history_floor_unsupported`
   refusal with filtering canvas content by the same floor where canvas items carry a time/revision; if
   some canvas content cannot be windowed, keep refusing THAT content only and document it.
3. Gateway BFF + SPA UI: gateway routes for the invite option and the admin change, action contracts in
   `src/shared/actions` first, invite-dialog toggle "Share past conversation" (default on), member-list
   admin control "History: full / from date [date picker] / since joined", a small "Earlier messages are
   hidden" marker at the top of the timeline for a floored member, i18n NO/EN.
