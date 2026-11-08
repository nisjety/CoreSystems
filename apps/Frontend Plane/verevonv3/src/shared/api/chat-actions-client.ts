import { listAuthored, type AuthoredBehaviour } from './authored-kinds-client'
import { loadComposerSettingsItems } from './composer-settings-client'
import { requestJson } from './http'

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
  builtin?: 'file' | 'image' | 'web_search' | 'learn'
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

/**
 * `/learn` (EP-11 S7): ask Verevon to distil this conversation into a skill proposal for the
 * learning inbox. Not in `BUILTIN_ACTIONS`: the composer registers it only where
 * `learnCommandAvailable` says so, through its slash-command registry.
 */
export const LEARN_ACTION: SpecializedAction = {
  id: '__learn',
  name: 'Learn from this conversation',
  description: 'Verevon proposes a skill from this conversation; nothing is used until you accept it.',
  kind: 'builtin',
  builtin: 'learn',
}

/** The longest note a `/learn` carries, as the gateway and capability-core bound it. */
export const LEARN_NOTE_MAX_CHARS = 500

export type LearnCommandContext = {
  /** The saved conversation the command would learn from; absent before the first send. */
  threadId?: string | null
  /** A temporary chat runs under zero data retention: nothing is kept to learn from. */
  temporaryChat: boolean
  /** The session itself is zero data retention. */
  zeroRetention: boolean
  /** The memory controls' `learning_allowed`: the owner and the organization allow learning. */
  learningAllowed: boolean
}

/**
 * Whether `/learn` is offered. Hidden on a temporary or ZDR chat, before the conversation is
 * saved, and whenever the memory controls say learning is off (the person's pause, or an
 * organization that has not opted in). The server refuses each of these on its own; hiding the
 * command only keeps the person from asking for something that cannot happen.
 */
export function learnCommandAvailable(context: LearnCommandContext): boolean {
  return (
    !context.temporaryChat &&
    !context.zeroRetention &&
    context.learningAllowed &&
    Boolean(context.threadId?.trim())
  )
}

export type LearnRequestState = { state: string }

/**
 * Asks Verevon to learn from one of the caller's own conversations. Answers once the request is
 * queued; the proposal reaches the learning inbox when the review is done. Whose conversation it
 * is comes from the session on the server, never from anything sent here.
 */
export async function requestLearnFromThread(threadId: string, note?: string): Promise<LearnRequestState> {
  const trimmed = note?.trim() ?? ''
  return requestJson<LearnRequestState>('/api/v1/learning/learn', {
    method: 'POST',
    body: JSON.stringify(trimmed ? { threadId, note: trimmed } : { threadId }),
  })
}

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

/**
 * The person's `/` templates (EP-11 S6). Kept apart from `loadSpecializedActions` on purpose: a
 * specialized action rides the invoke `tools` array, while a template only fills the draft and
 * never reaches the model as anything but the text the person sends. Empty while the server's
 * authored kinds are off, and on any failure, so the menu keeps working.
 */
export async function loadSlashTemplates(signal?: AbortSignal): Promise<AuthoredBehaviour[]> {
  try {
    const { items, kindsEnabled } = await listAuthored('template', signal)
    return kindsEnabled ? items : []
  } catch {
    return []
  }
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
