import type { z } from 'zod'

type PlaneOwner = 'application' | 'control' | 'data' | 'ingestion' | 'model'
type ActorType = 'human' | 'model'
type ActionRisk = 'low' | 'medium' | 'high'
type RunStatus = 'queued' | 'planning' | 'waiting_approval' | 'executing' | 'completed' | 'failed'

export type ActionActor = {
  type: ActorType
  userId: string
  orgId: string
  runId?: string
}

/**
 * Who runs an action (O-26, D-f09-5; not the ModelExecutor of model-access.ts). `gateway`: the browser sends it to
 * POST /api/v1/actions/execute and the gateway runs it through its arm. `client`: the browser calls its own REST
 * route (`clientRoute`) or runs it in the page, so the execute endpoint refuses it and names that route. The gateway
 * reads the same answer from apps/gateway/src/domains/actions/action-executors.json, generated from this registry.
 */
export type ActionExecutor = 'gateway' | 'client'

export type ActionDescriptor<Input extends z.ZodType = z.ZodType, Output extends z.ZodType = z.ZodType> = {
  id: string
  label: string
  description: string
  ownerPlane: PlaneOwner
  risk: ActionRisk
  requiresApproval: boolean
  reversible: boolean
  inputSchema: Input
  outputSchema: Output
} & (
  | { executor: 'gateway'; clientRoute?: undefined }
  | { executor: 'client'; clientRoute: string }
)

export type ActionPreview = {
  actionId: string
  risk: ActionRisk
  requiresApproval: boolean
  estimatedCost: string
  summary: string
  missingInputs: readonly string[]
}

export type ActionExecution = {
  actionId: string
  runId: string
  status: RunStatus
  auditId: string
  eventStream: string
  /** Owner-issued operation receipt when an action has migrated off synthetic gateway IDs. */
  operationId?: string
  /** Durable owner audit-event identifier paired with operationId. */
  auditEventId?: string
  /** True only when the owner returned the receipt for an exact idempotent replay. */
  replayed?: boolean
  /** What the owner dispatcher returned for the action (its `data`), when the action has one. */
  result?: unknown
  /** Present when a ticket action durably creates or mutates a ticket. */
  ticketId?: string
}
