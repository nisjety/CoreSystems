import type { ActionId } from '@/shared/actions/action-registry'

/**
 * Model eligibility is an execution claim, not a UI preference. An action is
 * listed here only after its owner exposes the governed operation contract,
 * Capability Core binds it to the agent runtime, and the owner enforces a
 * forged-call denial. Until that work lands, an empty allowlist is the honest
 * and safe default.
 */
const MODEL_EXECUTABLE_ACTION_IDS = new Set<ActionId>()

export function isModelExecutableAction(actionId: string): actionId is ActionId {
  return MODEL_EXECUTABLE_ACTION_IDS.has(actionId as ActionId)
}
