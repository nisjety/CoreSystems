import type { ActionId } from '@/shared/actions/action-registry'

/**
 * Model eligibility is an execution claim, not a UI preference. An action is
 * listed here only after its owner exposes the governed operation contract,
 * Capability Core binds it to the agent runtime, and the owner enforces a
 * forged-call denial. An empty allowlist is the honest default; each entry has
 * to earn its place, and the evidence belongs next to it.
 *
 * `tickets.create` — admitted 2026-08-27. What was proven, in order:
 *
 *  1. Governed operation contract. conversation-core issues a durable owner
 *     receipt (`conversation_ticket_operations`), and execution-core reaches it
 *     only through Control: a signed run-action decision, a current-authority
 *     re-check, then reserve -> commit as the linearization point.
 *  2. Forged-call denial, on BOTH sides of the boundary. The owner rejects a
 *     decision whose run id, schema hash, idempotency key, payload digest, or
 *     subject does not match, and fails closed with no verifier at all
 *     (conversation-core-go internal/http/agent_ticket_operation_test.go). The
 *     gateway independently refuses to manufacture a receipt the owner did not
 *     issue — see the ticket_receipt_* tests in
 *     apps/gateway/src/domains/actions/dispatchers.rs.
 *  3. Idempotent fencing, observed live rather than inferred: two requests
 *     carrying one idempotency key produced ONE ticket and ONE operation
 *     receipt, the second returning `replayed: true` with the same
 *     operation and audit ids.
 *
 * What is deliberately NOT claimed by this entry: Capability Core does not yet
 * advertise tickets.create as available, because conversation-core's health
 * reporter needs CAPABILITY_CORE_HTTP_URL and its own credential. Listing the
 * action here makes it offerable to a model; the owner still decides every
 * call, and availability stays server-authoritative.
 *
 * `inbox.follow_conversation`, `inbox.set_csat_preference` — admitted
 * 2026-08-28, after an exhaustive audit of every other registered action
 * against the same three-part bar, independently adversarially re-verified
 * (3 skeptics per candidate, default-to-refuted).
 *
 *  1. Durable receipt. Both write a real Postgres row keyed on a natural,
 *     unforgeable key (conversation-core-go's `conversation_follows` on
 *     (org_id, conversation_id, user_id); `conversation_csat_preferences` on
 *     (org_id, contact_id)), queryable back afterward via the sibling GET
 *     route. Neither is an append-only per-call ledger — a later call
 *     overwrites an earlier one's trace — which is a real, narrower
 *     guarantee than tickets.create's, accepted here only because the
 *     action itself is a low-stakes, idempotent preference toggle, not a
 *     resource creation.
 *  2. Idempotent fencing via the same natural key: a genuine Postgres
 *     `ON CONFLICT ... DO UPDATE`/unique constraint, not a caller-supplied
 *     key, but a real structural collapse of duplicate submissions — not a
 *     fresh id minted per call.
 *  3. Forged-call denial, on BOTH sides, proven by tests that actually
 *     exercise the mechanism gating these routes (not by analogy): gateway
 *     side, `active_membership_uses_control_role_and_requires_exact_scope`
 *     (apps/gateway/src/upstream.rs) proves a claimed-vs-Control-verified
 *     org mismatch is rejected before `proxy_conversation_json` ever
 *     forwards the call; owner side, `TestVerifierRejectsBodyScopeRoleMethodAndQueryTampering`
 *     (conversation-core-go's delegation/verifier_test.go) proves a
 *     tampered org/user/role/method on a signed request invalidates it.
 *     Neither test names these two actions specifically — both actions
 *     sit behind the identical shared middleware/verifier chain that every
 *     other route in the same group sits behind, with no per-route
 *     override, so the tests are mechanically applicable, not aspirational.
 *
 * `membership.remove_member` was audited under the same process and
 * reached the same "not unanimously refuted" bar (1 of 3 skeptics
 * dissented), but is deliberately NOT admitted here. The dissent's specific
 * finding: no test on either side (gateway or auth-core) proves a
 * wrong-org actor is rejected for this action specifically — the
 * DB-level guard that would reject it (`apply_membership_mutation`'s
 * actor-role lookup) exists but is untested. Cross-org forgery is the
 * paradigmatic threat for an action that ends sessions and revokes org
 * access, so admitting it on a 2-of-3 vote would understate a real,
 * specifically-identified gap rather than a generic residual doubt. See
 * apps/verevon-web/plans/system-audits/AI_FIRST_CREED_STATUS_2026-08-28.md for the full evidence trail;
 * this is a flagged decision point for whoever owns the eligibility bar,
 * not a rejection of the underlying receipt/idempotency infrastructure
 * (which is genuinely solid).
 *
 * `chat.save_thread_snapshot`, `inbox.review_ai_action`, `org.mark_exported`,
 * `org.acknowledge_deletion` — admitted 2026-08-28, via a second pass over
 * actions whose owner-side protection was already known-solid but whose
 * gateway-side forged-actor evidence hadn't been traced to this specific
 * action's actual code path. All four were unanimously confirmed (3/3
 * skeptics, same default-to-refuted standard). Each dispatcher's org/actor
 * value was independently traced to originate from
 * `user.authorized_membership` (populated only by `resolve_active_membership`
 * against Control Plane, unit-tested at
 * `active_membership_uses_control_role_and_requires_exact_scope` in
 * apps/gateway/src/upstream.rs) or, for `chat.save_thread_snapshot`
 * (Model Plane), an equivalent independently-verified delegated-bearer
 * chain (model-gateway's `verify_delegated_user_bearer`, unit-tested at
 * `delegated_session_bearer_requires_exact_audience_and_matching_identity`
 * in model-gateway's src/auth.rs) plus session-core's own row-level
 * ownership check (`authorize_owner_row`, tested at
 * `ordinary_rows_are_unchanged_by_the_system_branch` in session-core's
 * src/auth.rs). Owner-side receipts and idempotent fencing for all four
 * are durable, real, and — for `inbox.review_ai_action` and
 * `org.mark_exported`/`org.acknowledge_deletion` — directly re-run this
 * session (`TestReviewAIAction`, `go test ./internal/delegation/...`) where
 * the environment allowed a live pass.
 *
 * `chat.submit_feedback` — admitted 2026-08-28, after a live bug fix. It
 * reached the same evidentiary bar as the four above on receipt,
 * idempotency, and owner-side forged-actor denial, but the recalibration
 * pass caught something more serious than an eligibility gap: the gateway
 * dispatcher never forwarded the delegated session bearer model-gateway's
 * `/v1/feedback` route requires (`VerifiedModelBearer`, a local alias for
 * `VerifiedSessionBearer` in that service, populated by a single shared
 * `require_auth` middleware layered once over the whole router — verified
 * there is no per-route audience split this fix could have gotten wrong),
 * so a real call would 401 before ever reaching `resolve_feedback_target`,
 * the durable-receipt insert, or the idempotent `ON CONFLICT` — a live
 * defect, not a documentation gap. Fixed in
 * `apps/gateway/src/domains/chat/json_handlers.rs`'s `submit_feedback`
 * (and the identical missing-delegation shape in `queue_invocation_input`,
 * found by the same pass) to mint and forward the session token via
 * `shared::session_token`/`proxy_model_json_with_session`, matching the
 * already-established pattern in `get_thread_messages`/`get_thread_context`.
 * A follow-up, independent verification confirmed the fix closes the gap
 * end-to-end (audience match, header-name match, and the forwarded token is
 * actually used as a real gRPC credential downstream, not merely present)
 * with no new gap introduced. `cargo check` clean; gateway `domains::chat`
 * (19/19) and a full `domains::` sweep (307/307) both pass with the fix.
 */
const MODEL_EXECUTABLE_ACTION_IDS = new Set<ActionId>([
  'tickets.create',
  'inbox.follow_conversation',
  'inbox.set_csat_preference',
  'chat.save_thread_snapshot',
  'inbox.review_ai_action',
  'org.mark_exported',
  'org.acknowledge_deletion',
  'chat.submit_feedback',
])

export function isModelExecutableAction(actionId: string): actionId is ActionId {
  return MODEL_EXECUTABLE_ACTION_IDS.has(actionId as ActionId)
}
