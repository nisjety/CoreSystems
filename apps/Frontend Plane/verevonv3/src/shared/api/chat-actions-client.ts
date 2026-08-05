import { loadComposerSettingsItems } from './composer-settings-client'

// "Specialized actions" power the composer's `/` menu. Built-ins map to local
// composer behaviors (attach file, image mode, web search); skills /
// capabilities / connectors are fetched from the backend through the gateway
// and, when activated, are threaded into the chat invoke `tools` array.

export type SpecializedActionKind = 'builtin' | 'skill' | 'capability' | 'connector'

export type SpecializedAction = {
  /** Raw backend id (skill/capability id, provider key) or builtin sentinel. */
  id: string
  name: string
  description?: string
  kind: SpecializedActionKind
  /** Present only for built-ins — selects the local composer behavior. */
  builtin?: 'file' | 'image' | 'web_search'
}

export const BUILTIN_ACTIONS: SpecializedAction[] = [
  {
    id: '__file',
    name: 'Upload file',
    description: 'Attach a file or photo to your message.',
    kind: 'builtin',
    builtin: 'file',
  },
  {
    id: '__image',
    name: 'Generate image',
    description: 'Create an image from your prompt.',
    kind: 'builtin',
    builtin: 'image',
  },
  {
    id: 'web_search',
    name: 'Web search',
    description: 'Let Verevon search the web while it answers.',
    kind: 'builtin',
    builtin: 'web_search',
  },
]

/** Stable de-dup / list key for an action (raw ids can collide across kinds). */
export function actionKey(action: Pick<SpecializedAction, 'id' | 'kind'>): string {
  return `${action.kind}:${action.id}`
}

/**
 * Fetch skills + capabilities + connectors in parallel and merge with the
 * built-ins. Each source degrades to empty on failure so the `/` menu always
 * works (built-ins are always present even if the backend is unreachable).
 */
export async function loadSpecializedActions(signal?: AbortSignal): Promise<SpecializedAction[]> {
  const [skills, capabilities, connectors] = await Promise.all([
    fetchActions('/api/v1/skills', 'skills', 'skill', signal),
    fetchActions('/api/v1/capabilities', 'capabilities', 'capability', signal),
    fetchActions('/api/v1/integrations/providers', 'providers', 'connector', signal),
  ])
  return [...BUILTIN_ACTIONS, ...skills, ...capabilities, ...connectors]
}

async function fetchActions(
  endpoint: string,
  itemKey: string,
  kind: SpecializedActionKind,
  signal?: AbortSignal,
): Promise<SpecializedAction[]> {
  try {
    const items = await loadComposerSettingsItems(endpoint, itemKey, signal)
    return items.map((item) => ({
      id: item.id,
      name: item.name,
      description: item.description,
      kind,
    }))
  } catch {
    // Tolerate unavailable/unauthorized surfaces — the menu keeps its built-ins.
    return []
  }
}
