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
}

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
}
