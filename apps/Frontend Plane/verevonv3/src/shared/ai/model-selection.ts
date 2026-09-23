import { listChatGptSubscriptions, OPENAI_CODEX_SUBSCRIPTION_PROVIDER } from '@/shared/api/chatgpt-subscription-client'
import { VEREVON_BALANCE_MODE_ID } from '@/shared/api/chat-client'

export type AiModelSelection = Readonly<{
  model: string
  label: string
  provider?: string
  subscriptionConnectionId?: string
}>

const STORAGE_PREFIX = 'verevon.ai-model-selection.v1:'

export const DEFAULT_AI_MODEL_SELECTION: AiModelSelection = {
  model: VEREVON_BALANCE_MODE_ID,
  label: 'Verevon Balance',
}

function storage(): Storage | undefined {
  try {
    return typeof window === 'undefined' ? undefined : window.localStorage
  } catch {
    return undefined
  }
}

function storageKey(orgId: string): string {
  return `${STORAGE_PREFIX}${orgId.trim() || 'unknown'}`
}

function normalizeSelection(value: unknown): AiModelSelection | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined
  const raw = value as Record<string, unknown>
  const model = typeof raw.model === 'string' ? raw.model.trim() : ''
  const label = typeof raw.label === 'string' ? raw.label.trim() : ''
  const provider = typeof raw.provider === 'string' ? raw.provider.trim() : undefined
  const subscriptionConnectionId = typeof raw.subscriptionConnectionId === 'string'
    ? raw.subscriptionConnectionId.trim()
    : undefined
  if (!model || !label) return undefined
  if (provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER && !subscriptionConnectionId) {
    return { model, label, provider }
  }
  return {
    model,
    label,
    ...(provider ? { provider } : {}),
    ...(subscriptionConnectionId ? { subscriptionConnectionId } : {}),
  }
}

export function readAiModelSelection(orgId: string): AiModelSelection {
  try {
    const raw = storage()?.getItem(storageKey(orgId))
    return raw ? normalizeSelection(JSON.parse(raw)) ?? DEFAULT_AI_MODEL_SELECTION : DEFAULT_AI_MODEL_SELECTION
  } catch {
    return DEFAULT_AI_MODEL_SELECTION
  }
}

export function rememberAiModelSelection(orgId: string, selection: AiModelSelection): void {
  const normalized = normalizeSelection(selection)
  if (!normalized) return
  try {
    storage()?.setItem(storageKey(orgId), JSON.stringify(normalized))
  } catch {
    // A blocked/full storage area must not prevent model selection for this tab.
  }
}

/** Resolve the org-owned connection at send time. Persisted connection ids are
 * hints only: reconnect can replace one, so a fresh active connection wins. */
export async function resolveAiModelSelection(orgId: string): Promise<AiModelSelection> {
  const selected = readAiModelSelection(orgId)
  if (selected.provider !== OPENAI_CODEX_SUBSCRIPTION_PROVIDER) return selected
  const connections = await listChatGptSubscriptions(orgId)
  const active = connections.find((connection) =>
    connection.status.trim().toLocaleLowerCase() === 'active'
      && connection.id === selected.subscriptionConnectionId,
  ) ?? connections.find((connection) => connection.status.trim().toLocaleLowerCase() === 'active')
  if (!active) throw new Error('ChatGPT-abonnementet er ikke tilkoblet. Koble til i Integrasjoner eller velg en annen modell.')
  return { ...selected, subscriptionConnectionId: active.id }
}

export function resetAiModelSelectionForTests(orgId: string): void {
  try {
    storage()?.removeItem(storageKey(orgId))
  } catch {
    // Test helper only.
  }
}
