import { createSignal } from 'solid-js'
import { readClientJson, removeClientValue, writeClientJson } from '@/shared/session/client-storage'

export const CHAT_APPEARANCE_KEY = 'verevon.chat-appearance-preview.v1'
export const chatAppearanceKeys = ['welcome', 'dock', 'composer', 'history', 'navigation', 'organizer', 'context', 'dashboard'] as const
export type ChatAppearanceKey = typeof chatAppearanceKeys[number]
export type ChatAppearanceChoices = Record<ChatAppearanceKey, boolean>

const original: ChatAppearanceChoices = {
  welcome: false, dock: false, composer: false, history: false, navigation: false, organizer: false, context: false, dashboard: false,
}

function isChoices(value: unknown): value is Partial<ChatAppearanceChoices> {
  return typeof value === 'object' && value !== null &&
    chatAppearanceKeys.every((key) => !(key in value) || typeof (value as Record<string, unknown>)[key] === 'boolean')
}

// This experiment is opt-in and local to the development browser. Production
// always renders the existing appearance, even if this key exists in storage.
export const [chatAppearanceChoices, setChoices] = createSignal<ChatAppearanceChoices>(
  import.meta.env.DEV ? { ...original, ...readClientJson(CHAT_APPEARANCE_KEY, isChoices) } : original,
)
export const [comparingOriginal, compareChatOriginal] = createSignal(false)

export function chatAppearanceActive(key: ChatAppearanceKey): boolean {
  return import.meta.env.DEV && !comparingOriginal() && chatAppearanceChoices()[key]
}

export function chatAppearanceTokens(): string {
  return chatAppearanceKeys.filter(chatAppearanceActive).join(' ')
}

export function chooseChatAppearance(key: ChatAppearanceKey, enabled: boolean) {
  setChoices((current) => {
    const next = { ...current, [key]: enabled }
    writeClientJson(CHAT_APPEARANCE_KEY, next)
    return next
  })
  compareChatOriginal(false)
}

export function tryAllChatAppearance() {
  const next = Object.fromEntries(chatAppearanceKeys.map((key) => [key, true])) as ChatAppearanceChoices
  writeClientJson(CHAT_APPEARANCE_KEY, next)
  setChoices(next)
  compareChatOriginal(false)
}

export function resetChatAppearance() {
  removeClientValue(CHAT_APPEARANCE_KEY)
  setChoices({ ...original })
  compareChatOriginal(false)
}
