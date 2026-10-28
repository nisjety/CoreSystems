# Wave 1: decisions that need the owner, 2026-10-28

The Wave 1 triage (`docs/plans/model-plane-wave1-triage-2026-10-27.md`) left 83 rows that block Wave 1.
This file lists the ones that cannot be closed by code or by a live proof, because the next step is the
owner's choice. Each entry gives the question, what was measured, and the recommended answer. Nothing
here has been switched, pushed or accepted on the owner's behalf.

The live proofs of 2026-10-27/28 are in `docs/compliance/evidence/live-proof-wave1-depth-2026-10-27.txt`
(part 5).

## From the triage

### D1-003 (WS12-S1): may the integration branch be pushed so CI runs are recorded?

The branch `model-plane` is local only (standing rule: no push, no PR). Recorded CI runs, and with them
D1-086, D1-088 and D1-138 (the Linux release-artifact and ops-gates tests), need a pushed branch.

Recommendation: push `model-plane` to a branch of its own on the remote (not to the main branch), let CI
run, then decide about a PR separately. Until then those four rows stay open.

### D1-011 (WS05-S2, DEC-12 D2 and D5): may the MCP annotation-trust switch go beyond the proof stack?

D2 makes `annotation_trust` an admin's per-server choice, default untrusted. D5 is the org-scoped HMAC
route for observation writes: a compromised gateway holding the root secret could mint a token for any
org except `global`.

Recommendation: accept D2 as built (default untrusted, admin opt-in per server). Do not accept D5 beyond
the proof stack until the root secret is held by capability-core alone and the gateway receives only a
per-org derived key (or a short-lived per-org token), so a gateway compromise is bounded to the orgs it
serves. This needs a security review either way.

### D1-042 (WS01-S2, K2-11): may every org member read the replayed tool-action audit rows?

The 5000-row replay of the Model Plane session audit trail is waiting. Measured on 2026-10-27, with the
alert rules finally loaded on the Control Plane Prometheus: `ModelAuditOutboxTerminalRows` is pending
with value 4992, the terminal rows of session-core's audit outbox.

Recommendation: do not accept member-wide reads. Build the Space/role gate on `GET /api/v1/audit`
(WS06 / F-t1-031; the compliance endpoints already answer owner 200, admin 200, member 403) and replay
after it. Tool-action rows describe what members did with tools; widening who can read them is a privacy
change, not an audit fix.

### D1-482 (WS04-S7): shared idempotency without the run-control bus

A duplicate that arrives before the original has buffered a frame, with the bus off, gets the error that
names the original instead of attaching. Either buffer `connected` before the claim returns, or require
the bus for `MODEL_GATEWAY_SHARED_IDEMPOTENCY`.

Recommendation: require the bus. Since 08ae0b374 `MODEL_GATEWAY_RUN_CONTROL_BUS` defaults on and is
live-proven on two replicas; the gateway should refuse to enable shared idempotency (log it, keep it
off) when the bus is off. That is a small code change, and the switch stays off until D1-484's proof.

## New, found by the live proofs of 2026-10-27/28

### D1-808: prompt caching on the Anthropic route never reads

Six turns on claude-sonnet-5-5, consecutive turns 8 to 15 seconds apart: `cache_read_tokens` 0 on
every turn, `cache_write_tokens` equal to the whole input every turn, and cost-core classifies every
follow-up as `prefix_changed`. The chat prompt's leading system messages carry per-turn content
(grounding and session-core's context block in the legacy order), and the stable-prefix order (WS02-S8)
is deliberately off for Claude models (`prompt_context.rs`, `CacheRoute::of_turn`). So every Anthropic
turn pays the cache-write premium on its whole input and never the read discount.

Recommendation: approve a slice that extends the stable-prefix order to the Anthropic route (standing
blocks first and cached, per-turn blocks after the cache breakpoint, inference-core's adapter placing
the breakpoint at the end of the standing prefix), measured A/B on the dev stack with cost-core's
`cache_breakdown`. Until then, consider whether to stop sending cache-control on the Anthropic route at
all, since a write that is never read is a pure cost.

### D1-803: which values own the Application Plane runtime topology

The Control audit-extra provisioner and the Application Plane scripts disagreed (runtime consumers
`max_deliver` 5 vs unlimited; runtime streams 14 days / 1M messages vs 72 hours / unlimited). The fix
in round N aligns the Application Plane scripts to audit-core's values, because the Control Plane
rollout doc names audit-extra-nats-provisioner the owner of Application topology. It changes
conversation-core's and insight-core's redelivery from unlimited to five attempts, and the three
runtime streams already run with 14 days / 1M since the provisioner's partial run on 2026-10-27.

Recommendation: accept audit-core's values (five attempts is the safer poison-message bound). Say so if
the Application Plane should own them instead; then audit-core's provisioner must be changed instead.

### D1-804: leads-core has no Billing Core entitlement client on the dev stack

Every lead action returns 503 `entitlement_check_unavailable` because leads-core runs without a Billing
Core client configured. Decide whether the dev stack should have the lead-builder entitlement wired (the
Application Plane env chain) or whether lead actions stay off there.

### D1-811: longer retention on the Application Plane runtime streams

Follows from D1-803: VEREVON_APPLICATION, VEREVON_MODEL and VEREVON_INGESTION keep 336 hours and up to
1,000,000 messages (was 72 hours, unlimited), and their shared consumers stop after five deliveries.

Recommendation: confirm that no content-bearing event on those subjects needs the shorter window under zero
data retention; if one does, it should not be on a 14-day stream at all, whatever the provisioner says.

### Switch flips the live proofs now allow

- WS11-S5 effort ladder (D1-024): proven on claude-sonnet-5-5 (quick resolves to low with 5120 output
  tokens, deep to high with 8192; `/v1/models` lists the effort levels). Both switches are back off.
  Recommendation: flip `INFERENCE_CORE_ANTHROPIC_ADAPTIVE_THINKING` and then `MODEL_GATEWAY_EFFORT_LADDER`
  to default on.
- WS08-S4 images as parts (D1-637): proven on the Anthropic leg (two images read correctly, the user
  message stored as text only, Do and Temporary chat refused with their typed codes). The verevon-balance
  leg was not run (Anthropic-only test policy). Recommendation: flip `CHAT_VISION_AS_PARTS_ENABLED` together
  with `VITE_CHAT_VISION_AS_PARTS_ENABLED` only after the Verevon route is proven, or flip it for Claude
  models alone.

### D1-810: what a Stop of a Do run costs and leaves behind

The Do-run Stop (D1-229) is live-proven across replicas. Two consequences need a choice: a stopped Do run's
token usage is not ledgered (the abort drops the RunAgent answer that carries it), and a stopped park
continue is now cancelled in Session Core, the one exception to "the gateway never terminalizes a park".

Recommendation: have Execution Core read its own cancel latch and return the usage of the rounds it ran
(an Execution Core slice), and accept the park exception (a user Stop is an explicit cancel).

### Paused memory answers 409, the plan said 403

`POST /api/v1/memory` for a paused user answers 409 `memory_not_enabled` (reason `user_paused`); the WS07-S5
row expected 403. The typed code is right. Recommendation: keep 409 (a precondition, not a permission) and
correct the row's wording.

## Not owner decisions, but blocked on something only the owner has

- The Control Plane and the Application Plane on the dev stack run from the main checkout
  (`C:\dev\CoresSystem`), not from this branch. So the branch's Control Plane compose (the application audit
  bus required, the gateway's compliance scope on audit-core) and its user-core change (WS07-S5, no
  `memoryEnabled`) reach the stack only through a merge or a deploy from the branch. Tonight two of them
  were applied by replaying single containers; a compose recreate from the main checkout undoes that.
- D1-363, D1-364 (WS02-S8): the cache A/B "on staging" needs a staging environment.
- D1-220 to D1-225, D1-330 (WS01-S7, S13): the ZDR live proofs need a ZDR-TEST org seeded in org-core
  with the gateway's org-core credential (`zdr-seed-test-org.mjs --apply`) and the ordered Z-60 deploy.
  That can be done on the dev stack; it is a cross-plane data write and an ordered deploy, so it is
  planned as one session of its own rather than squeezed in.
