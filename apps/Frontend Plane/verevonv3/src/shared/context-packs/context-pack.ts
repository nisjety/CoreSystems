import { actionRegistry } from '@/shared/actions/action-registry'

export type VisibleItem = {
  type: 'ticket' | 'source' | 'agent' | 'metric' | 'run'
  id: string
  label: string
  status: string
}

export type SupportKnowledgeLink = {
  id?: string
  title: string
  uri?: string
  excerpt?: string
}

/** Shared redacted context for Verevon in Support and Ticketing. */
export type SupportAssistantContext = {
  organization?: { id: string; name?: string }
  conversation?: {
    id: string
    title?: string
    channel?: string
    status?: string
    customer?: { id?: string; name?: string; email?: string }
  }
  ticket?: {
    id: string
    key?: string
    status?: string
    slaState?: string
    priority?: string
    severity?: string
    category?: string
    intent?: string
    assignee?: string
    team?: string
  }
  relatedConversations?: Array<{ id: string; title?: string; channel?: string; status?: string }>
  knowledgeLinks?: SupportKnowledgeLink[]
  availableActions?: readonly string[]
  permissions?: readonly string[]
}

export type ModelContextPackInput = {
  route: string
  selectedEntity?: VisibleItem
  visibleItems: readonly VisibleItem[]
  filters?: Record<string, string>
  draftInput?: string
  support?: SupportAssistantContext
}

export type ModelContextPack = ModelContextPackInput & {
  currentView: string
  availableActions: readonly string[]
  redactionPolicy: 'ids-and-summaries-only'
}

export function buildModelContextPack(input: ModelContextPackInput): ModelContextPack {
  const currentView = input.route.replace(/^\//, '').replaceAll('/', '.') || 'dashboard'

  return {
    ...input,
    currentView,
    availableActions: actionRegistry.map((action) => action.id),
    redactionPolicy: 'ids-and-summaries-only',
  }
}
