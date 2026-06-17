import { requestJson } from '@/shared/api/http'
import { getActionDescriptor, type ActionId } from '@/shared/actions/action-registry'
import type { ActionActor, ActionExecution, ActionPreview } from '@/shared/actions/types'
import type { z } from 'zod'

const LIVE_ACTIONS = new Set<ActionId>([
  'knowledge.recrawl_source',
  'knowledge.scrape_url',
  'knowledge.crawl_site',
  'knowledge.import_source',
  'knowledge.upload_files',
  'knowledge.connect_source',
  'security.check_url_reputation',
  'security.investigate_url',
  'workflows.toggle_policy',
])

const riskCost = {
  low: 'low token budget',
  medium: 'moderate token budget',
  high: 'approval-gated budget',
} as const

function createId(prefix: string) {
  return `${prefix}_${crypto.randomUUID()}`
}

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

  if (LIVE_ACTIONS.has(actionId)) {
    return requestJson<ActionExecution>('/api/v1/actions/execute', {
      method: 'POST',
      body: JSON.stringify({ actionId, input: validation.data }),
      headers: { 'x-velion-org-id': actor.orgId },
    })
  }

  const runId = actor.runId ?? createId('run')

  return {
    actionId,
    runId,
    status: descriptor.requiresApproval ? 'waiting_approval' : 'queued',
    auditId: createId('audit'),
    eventStream: `/api/v1/runs/${runId}/events`,
  }
}
