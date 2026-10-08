import { createSignal } from 'solid-js'
import { readClientJson, removeClientValue, writeClientJson } from '@/shared/session/client-storage'

export const CHAT_APPEARANCE_KEY = 'verevon.chat-appearance-preview.v3'
// Earlier versions saved every switch against an all-off default, so their
// values cannot tell a deliberate opt-out from that old default. Ignore them.
const OLDER_CHAT_APPEARANCE_KEYS = ['verevon.chat-appearance-preview.v1', 'verevon.chat-appearance-preview.v2']
export const chatAppearanceKeys = ['welcome', 'dock', 'composer', 'history', 'navigation', 'organizer', 'context', 'dashboard'] as const
export type ChatAppearanceKey = typeof chatAppearanceKeys[number]
export type ChatAppearanceChoices = Record<ChatAppearanceKey, boolean>

// Every refinement is the standard appearance. Production always renders it.
// Development starts from it; the preview can switch any of them off, and
// Original (every switch off) compares against the legacy UI.
const standard: ChatAppearanceChoices = {
  welcome: true, dock: true, composer: true, history: true, navigation: true, organizer: true, context: true, dashboard: true,
}

function isChoices(value: unknown): value is Partial<ChatAppearanceChoices> {
  return typeof value === 'object' && value !== null &&
    chatAppearanceKeys.every((key) => !(key in value) || typeof (value as Record<string, unknown>)[key] === 'boolean')
}

// Development opt-outs stay in this browser. Production ignores them.
export const [chatAppearanceChoices, setChoices] = createSignal<ChatAppearanceChoices>(
  import.meta.env.DEV ? { ...standard, ...readClientJson(CHAT_APPEARANCE_KEY, isChoices) } : standard,
)
export const [comparingOriginal, compareChatOriginal] = createSignal(false)

export function chatAppearanceActive(key: ChatAppearanceKey): boolean {
  if (!import.meta.env.DEV) return standard[key]
  return !comparingOriginal() && chatAppearanceChoices()[key]
}

export function chatAppearanceTokens(): string {
  return chatAppearanceKeys.filter(chatAppearanceActive).join(' ')
}

export function chooseChatAppearance(key: ChatAppearanceKey, enabled: boolean) {
  setChoices((current) => {
    const next = { ...current, [key]: enabled }
    // Store only departures from the standard, so a later change to it is not
    // masked by a snapshot of today's defaults.
    const departures = Object.fromEntries(chatAppearanceKeys.filter((name) => next[name] !== standard[name]).map((name) => [name, next[name]]))
    if (Object.keys(departures).length) writeClientJson(CHAT_APPEARANCE_KEY, departures)
    else removeClientValue(CHAT_APPEARANCE_KEY)
    return next
  })
  compareChatOriginal(false)
}

export function resetChatAppearance() {
  for (const key of [CHAT_APPEARANCE_KEY, ...OLDER_CHAT_APPEARANCE_KEYS]) removeClientValue(key)
  setChoices({ ...standard })
  compareChatOriginal(false)
}
