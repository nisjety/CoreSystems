import { z } from 'zod'
import { actionRegistry } from '@/shared/actions/action-registry'
import type { ActionDescriptor } from '@/shared/actions/types'

export type AgentActionSelection = {
  id: string
  name?: string
  kind?: 'skill' | 'capability' | 'connector' | 'tool'
  description?: string
}

export type AgentToolSpec = {
  name: string
  description: string
  /** JSON Schema for the tool's parameters, serialized for the Model Plane wire contract. */
  parametersJson: string
}

export type VelionActionToolDefinition = {
  __toolSide: 'definition'
  name: string
  description: string
  inputSchema: z.ZodType
  outputSchema: z.ZodType
  needsApproval: boolean
  metadata: Record<string, unknown>
}

export const EMPTY_TOOL_PARAMETERS_JSON = '{"type":"object","properties":{}}'

export const WEB_SEARCH_AGENT_TOOL: AgentToolSpec = {
  name: 'web_search',
  description: 'Search the public web for current, factual information and return relevant results.',
  parametersJson: JSON.stringify({
    type: 'object',
    properties: {
      query: { type: 'string', description: 'The search query.' },
    },
    required: ['query'],
  }),
}

function actionMetadata(action: ActionDescriptor): Record<string, unknown> {
  return {
    actionId: action.id,
    ownerPlane: action.ownerPlane,
    risk: action.risk,
    requiresApproval: action.requiresApproval,
    reversible: action.reversible,
  }
}

function actionDescription(action: ActionDescriptor): string {
  return [
    action.description,
    `Owner plane: ${action.ownerPlane}.`,
    `Risk: ${action.risk}.`,
    `Requires approval: ${action.requiresApproval ? 'yes' : 'no'}.`,
    `Reversible: ${action.reversible ? 'yes' : 'no'}.`,
  ].join(' ')
}

function schemaToParametersJson(schema: z.ZodType): string {
  try {
    return JSON.stringify(z.toJSONSchema(schema))
  } catch {
    return EMPTY_TOOL_PARAMETERS_JSON
  }
}

function safeToolName(name: string): string | null {
  const trimmed = name.trim()
  if (!trimmed || trimmed.length > 128) return null
  return /^[A-Za-z0-9_.:-]+$/.test(trimmed) ? trimmed : null
}

function dedupeTools(tools: readonly AgentToolSpec[]): AgentToolSpec[] {
  const seen = new Set<string>()
  const out: AgentToolSpec[] = []

  for (const tool of tools) {
    const name = safeToolName(tool.name)
    if (!name || seen.has(name)) continue
    seen.add(name)
    out.push({ ...tool, name })
  }

  return out
}

function dynamicActionTool(action: AgentActionSelection): AgentToolSpec | null {
  const name = safeToolName(action.id)
  if (!name) return null

  return {
    name,
    description: action.description
      ?? (action.name ? `${action.kind ?? 'tool'}: ${action.name}` : `${action.kind ?? 'tool'} ${name}`),
    parametersJson: EMPTY_TOOL_PARAMETERS_JSON,
  }
}

export function createVelionActionToolSpec(action: ActionDescriptor): AgentToolSpec {
  return {
    name: action.id,
    description: actionDescription(action),
    parametersJson: schemaToParametersJson(action.inputSchema),
  }
}

export function createVelionActionToolSpecs(
  actions: readonly ActionDescriptor[] = actionRegistry,
): AgentToolSpec[] {
  return dedupeTools(actions.map(createVelionActionToolSpec))
}

export function createVelionActionToolDefinitions(
  actions: readonly ActionDescriptor[] = actionRegistry,
): VelionActionToolDefinition[] {
  return actions.map((action) => ({
    __toolSide: 'definition',
    name: action.id,
    description: actionDescription(action),
    inputSchema: action.inputSchema,
    outputSchema: action.outputSchema,
    needsApproval: action.requiresApproval,
    metadata: actionMetadata(action),
  }))
}

export function createSelectedAgentToolSpecs(input: {
  actions?: readonly AgentActionSelection[]
  browseWeb?: boolean
  explicitTools?: readonly Partial<AgentToolSpec>[]
}): AgentToolSpec[] {
  const registryTools = new Map(
    createVelionActionToolSpecs().map((tool) => [tool.name, tool] as const),
  )
  const selected: AgentToolSpec[] = []

  if (input.browseWeb) selected.push(WEB_SEARCH_AGENT_TOOL)

  for (const action of input.actions ?? []) {
    if (action.id === WEB_SEARCH_AGENT_TOOL.name) {
      selected.push(WEB_SEARCH_AGENT_TOOL)
      continue
    }

    const tool = registryTools.get(action.id) ?? dynamicActionTool(action)
    if (tool) selected.push(tool)
  }

  for (const tool of input.explicitTools ?? []) {
    if (!tool.name) continue
    selected.push({
      name: tool.name,
      description: tool.description ?? '',
      parametersJson: tool.parametersJson ?? EMPTY_TOOL_PARAMETERS_JSON,
    })
  }

  return dedupeTools(selected)
}
