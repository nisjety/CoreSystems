import { requestJson } from '@/shared/api/http'
import { getActionDescriptor, type ActionId } from '@/shared/actions/action-registry'
import type { ActionActor, ActionExecution, ActionPreview } from '@/shared/actions/types'
import type { z } from 'zod'

export type ActionExecutionOptions = Readonly<{
  /**
   * Reuse this exact key after an ambiguous response. The owner—not the
   * browser—decides whether it is a replay or a conflicting request.
   */
  idempotencyKey?: string
}>

const riskCost = {
  low: 'low token budget',
  medium: 'moderate token budget',
  high: 'approval-gated budget',
} as const

function formatValidationIssues(error: z.ZodError): readonly string[] {
  return error.issues.map((issue) => issue.path.join('.') || issue.message)
}

export async function previewAction(actionId: ActionId, input: unknown): Promise<ActionPreview> {
  const descriptor = getActionDescriptor(actionId)

  if (!descriptor) {
    throw new Error(`Unknown action: ${actionId}`)
  }

  const validation = descriptor.inputSchema.safeParse(input)
  const missingInputs = validation.success ? [] : formatValidationIssues(validation.error)

  return {
    actionId,
    risk: descriptor.risk,
    requiresApproval: descriptor.requiresApproval,
    estimatedCost: riskCost[descriptor.risk],
    summary: descriptor.description,
    missingInputs,
  }
}

export async function executeAction(
  actionId: ActionId,
  actor: ActionActor,
  input: unknown,
  options?: ActionExecutionOptions,
): Promise<ActionExecution> {
  const descriptor = getActionDescriptor(actionId)

  if (!descriptor) {
    throw new Error(`Unknown action: ${actionId}`)
  }

  const validation = descriptor.inputSchema.safeParse(input)

  if (!validation.success) {
    const issues = formatValidationIssues(validation.error).join(', ')
    throw new Error(`Invalid action input for ${actionId}: ${issues}`)
  }

  // The gateway owns live action availability. Keeping a second handwritten
  // browser allowlist caused shipping.get_quotes to be denied even though its
  // authenticated gateway dispatcher existed. The source-level contract test
  // protects registry-to-dispatcher parity; runtime availability remains a
  // server decision and must never be fabricated by the client.
  const suppliedKey = options?.idempotencyKey?.trim()
  if (suppliedKey !== undefined && (!suppliedKey || suppliedKey.length > 200)) {
    throw new Error('Invalid action idempotency key')
  }

  return requestJson<ActionExecution>('/api/v1/actions/execute', {
    method: 'POST',
    body: JSON.stringify({
      actionId,
      idempotencyKey: suppliedKey ?? crypto.randomUUID(),
      input: validation.data,
    }),
    headers: { 'x-verevon-org-id': actor.orgId },
  })
}

/**
 * Read the owner receipt for a tickets.create request whose original response
 * may have been lost. This never replays the create request itself.
 */
export async function reconcileTicketCreate(idempotencyKey: string): Promise<ActionExecution> {
  const key = idempotencyKey.trim()
  if (!key || key.length > 200 || /[\u0000-\u001f\u007f]/.test(key)) {
    throw new Error('Invalid action idempotency key')
  }
  return requestJson<ActionExecution>(
    `/api/v1/actions/tickets/create/${encodeURIComponent(key)}`,
    { method: 'GET' },
  )
}
