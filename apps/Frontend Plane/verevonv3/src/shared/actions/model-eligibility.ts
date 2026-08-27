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
 */
const MODEL_EXECUTABLE_ACTION_IDS = new Set<ActionId>(['tickets.create'])

export function isModelExecutableAction(actionId: string): actionId is ActionId {
  return MODEL_EXECUTABLE_ACTION_IDS.has(actionId as ActionId)
}
