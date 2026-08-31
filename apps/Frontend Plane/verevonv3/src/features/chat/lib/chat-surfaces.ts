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
  hasGrounding: boolean
  artifactCount: number
  attachmentCount: number
  stepCount: number
  hasRun: boolean
}

export type ChatSurfaceSpec = {
  id: ChatTab
  label: string
  description: string
  icon: IconComponent
  priority: number
  available: (state: ChatSurfaceAvailability) => boolean
}

/**
 * The single allowlist for contextual chat destinations.
 *
 * Components may decide how to render a surface, but they must not invent a
 * new tab id or independently decide whether an empty destination is useful.
 * Availability is evidence-driven so the default Chat experience stays calm.
 */
export const CHAT_SURFACE_REGISTRY: readonly ChatSurfaceSpec[] = [
  {
    id: 'chat',
    label: 'Chat',
    description: 'Samtalen er alltid hovedflaten.',
    icon: MessageSquare,
    priority: 0,
    available: () => true,
  },
  {
    id: 'steps',
    label: 'Work',
    description: 'Følg plan, fremdrift og beslutninger.',
    icon: ListChecks,
    priority: 10,
    available: (state) => state.stepCount > 0 || state.hasRun,
  },
  {
    id: 'artifacts',
    label: 'Output',
    description: 'Åpne filer, dokumenter og sider fra denne samtalen.',
    icon: FileCode2,
    priority: 20,
    available: (state) => state.artifactCount > 0 || state.attachmentCount > 0,
  },
  {
    id: 'sources',
    label: 'Kilder',
    description: 'Spor grunnlaget som ble brukt i svaret.',
    icon: Link2,
    priority: 30,
    available: (state) => state.sourceCount > 0 || state.hasGrounding,
  },
  {
    id: 'trace',
    label: 'Trace',
    description: 'Se tekniske hendelser, godkjenninger og verifisering.',
    icon: Receipt,
    priority: 40,
    available: (state) => state.hasRun,
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
