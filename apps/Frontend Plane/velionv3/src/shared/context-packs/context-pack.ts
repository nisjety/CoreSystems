import { actionRegistry } from '@/shared/actions/action-registry'

export type VisibleItem = {
  type: 'ticket' | 'source' | 'agent' | 'metric' | 'run'
  id: string
  label: string
  status: string
}

export type ModelContextPackInput = {
  route: string
  selectedEntity?: VisibleItem
  visibleItems: readonly VisibleItem[]
  filters?: Record<string, string>
  draftInput?: string
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
