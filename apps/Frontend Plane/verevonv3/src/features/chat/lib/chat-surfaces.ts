import {
  FileCode2,
  Link2,
  ListChecks,
  MessageSquare,
  Receipt,
} from '@/shared/icons'
import type { ChatTab, IconComponent } from '../components/chat-types'

export type ChatContextualTab = Exclude<ChatTab, 'chat'>

export type ChatSurfaceAvailability = {
  sourceCount: number
  /**
   * Sources actually READ, as opposed to found-but-never-fetched leads. Deep
   * research can list two dozen candidates and read three of them, and a lead
   * is not evidence -- so it may populate the tab (`available`) without being
   * worth pulling the user out of the conversation for (`claimsFocus`).
   * Optional and, when absent, treated as "none read" rather than falling back
   * to `sourceCount`: guessing is what made the panel open on a lead in the
   * first place, and the same reasoning applies here as to `workStepCount`.
   */
  readSourceCount?: number
  hasGrounding: boolean
  artifactCount: number
  attachmentCount: number
  stepCount: number
  hasRun: boolean
  /**
   * Steps that are work rather than bookkeeping (see `isWorkStep`). Optional
   * because not every caller can compute it, but a caller that omits it gets
   * no Work destination unless a durable run exists -- both `available` and
   * `claimsFocus` read it, so guessing from `stepCount` would reopen the bug
   * items 18 and 27 closed.
   */
  workStepCount?: number
  /** Tool calls the model actually made across the thread. Optional, as above. */
  toolCallCount?: number
}

export type ChatSurfaceSpec = {
  id: ChatTab
  label: string
  description: string
  icon: IconComponent
  priority: number
  available: (state: ChatSurfaceAvailability) => boolean
  /**
   * Stricter than `available`: may this surface pull focus away from the
   * conversation on its own? A tab can be offered (evidence exists) without
   * being worth interrupting for. VEREVON_CHAT_DESIGN.md section 3.1 -- first
   * citation opens Sources, first durable artifact opens Output, first
   * effectful or multi-step run opens Work; bookkeeping never opens anything.
   */
  claimsFocus: (state: ChatSurfaceAvailability) => boolean
}

/**
 * The single allowlist for contextual chat destinations.
 *
 * Components may decide how to render a surface, but they must not invent a
 * new tab id or independently decide whether an empty destination is useful.
 * Availability is evidence-driven so the default Chat experience stays calm.
 */
/**
 * Evidence that the agent did work, as opposed to the lifecycle bookkeeping
 * every answer emits (connect, compose, model selected, usage recorded ...).
 * `isWorkStep` in chat-normalizers.ts owns the classification.
 */
function hasWorkEvidence(state: ChatSurfaceAvailability): boolean {
  return (state.workStepCount ?? 0) > 0 || (state.toolCallCount ?? 0) > 0
}

export const CHAT_SURFACE_REGISTRY: readonly ChatSurfaceSpec[] = [
  {
    id: 'chat',
    label: 'Chat',
    description: 'Samtalen er alltid hovedflaten.',
    icon: MessageSquare,
    priority: 0,
    available: () => true,
    claimsFocus: () => false,
  },
  {
    id: 'steps',
    label: 'Work',
    description: 'Følg plan, fremdrift og beslutninger.',
    icon: ListChecks,
    priority: 10,
    // Lifecycle steps (connect, compose, model, usage, memory ...) are not a
    // destination: a plain answer produces six of them and nothing to inspect,
    // and UX spec section 4 says only evidence-backed destinations appear. The
    // durable run stands on its own -- it has receipts and a Trace either way.
    available: (state) => hasWorkEvidence(state) || state.hasRun,
    claimsFocus: (state) => state.hasRun || hasWorkEvidence(state),
  },
  {
    id: 'artifacts',
    label: 'Output',
    description: 'Åpne filer, dokumenter og sider fra denne samtalen.',
    icon: FileCode2,
    priority: 20,
    available: (state) => state.artifactCount > 0 || state.attachmentCount > 0,
    // The user's own attachments are not the agent's output.
    claimsFocus: (state) => state.artifactCount > 0,
  },
  {
    id: 'sources',
    label: 'Kilder',
    description: 'Spor grunnlaget som ble brukt i svaret.',
    icon: Link2,
    priority: 30,
    available: (state) => state.sourceCount > 0 || state.hasGrounding,
    // A grounding summary with no sources is not evidence worth interrupting
    // for -- and neither is a source we found but never read. Offering the tab
    // uses the full count above; claiming focus uses only what was actually
    // read, so the panel cannot pop open to show a page nobody fetched.
    claimsFocus: (state) => (state.readSourceCount ?? 0) > 0,
  },
  {
    id: 'trace',
    label: 'Trace',
    description: 'Se tekniske hendelser, godkjenninger og verifisering.',
    icon: Receipt,
    priority: 40,
    available: (state) => state.hasRun,
    // Becomes available on completion without ever stealing focus.
    claimsFocus: () => false,
  },
] as const

export const CHAT_SURFACE_IDS = CHAT_SURFACE_REGISTRY.map((surface) => surface.id) as readonly ChatTab[]

export function chatSurfaceSpec(id: ChatTab): ChatSurfaceSpec {
  return CHAT_SURFACE_REGISTRY.find((surface) => surface.id === id) ?? CHAT_SURFACE_REGISTRY[0]!
}

export function isChatTab(value: unknown): value is ChatTab {
  return typeof value === 'string' && CHAT_SURFACE_IDS.includes(value as ChatTab)
}

export function availableChatSurfaces(state: ChatSurfaceAvailability): ChatSurfaceSpec[] {
  return CHAT_SURFACE_REGISTRY
    .filter((surface) => surface.available(state))
    .sort((a, b) => a.priority - b.priority)
}

export function isChatSurfaceAvailable(id: ChatTab, state: ChatSurfaceAvailability): boolean {
  return chatSurfaceSpec(id).available(state)
}

/** Whether the surface may open itself right now. See `ChatSurfaceSpec.claimsFocus`. */
export function chatSurfaceClaimsFocus(id: ChatTab, state: ChatSurfaceAvailability): boolean {
  return chatSurfaceSpec(id).claimsFocus(state)
}
