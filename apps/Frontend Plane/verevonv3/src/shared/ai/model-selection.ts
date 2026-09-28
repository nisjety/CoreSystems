import {
  listChatGptSubscriptions,
  OPENAI_CODEX_SUBSCRIPTION_PROVIDER,
  type ChatGptSubscriptionConnection,
} from '@/shared/api/chatgpt-subscription-client'
import { VEREVON_BALANCE_MODE_ID } from '@/shared/api/chat-client'
import { normalizePrivacyTier, type PrivacyTier } from '@/shared/api/privacy-tier'

export type AiModelSelection = Readonly<{
  model: string
  label: string
  provider?: string
  subscriptionConnectionId?: string
  /**
   * The tier `/api/v1/models` last reported for this model and provider
   * (`'unspecified'` = it claimed none). Only a fallback: a composer sends it
   * when the catalogue has not answered in time, never in place of a loaded
   * catalogue's answer.
   */
  privacyTier?: PrivacyTier
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
  // A garbled tier is dropped, not guessed: without one the composer falls
  // back to the strictest tier.
  const privacyTier = normalizePrivacyTier(raw.privacyTier)
  if (!model || !label) return undefined
  if (provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER && !subscriptionConnectionId) {
    return { model, label, provider, ...(privacyTier ? { privacyTier } : {}) }
  }
  return {
    model,
    label,
    ...(provider ? { provider } : {}),
    ...(subscriptionConnectionId ? { subscriptionConnectionId } : {}),
    ...(privacyTier ? { privacyTier } : {}),
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

/**
 * Record the catalogue's current tier on the stored selection, when that
 * selection is still this model and provider. A no-op when nothing changed,
 * or when another tab has since stored a different model.
 */
export function rememberAiModelPrivacyTier(
  orgId: string,
  selection: Readonly<{ model: string; provider?: string }>,
  privacyTier: PrivacyTier,
): void {
  const stored = readAiModelSelection(orgId)
  if (stored.model !== selection.model || stored.provider !== selection.provider) return
  if (stored.privacyTier === privacyTier) return
  rememberAiModelSelection(orgId, { ...stored, privacyTier })
}

/**
 * The stored last-known tier, only when the stored selection is this model
 * and provider.
 */
export function lastKnownAiModelPrivacyTier(
  orgId: string,
  selection: Readonly<{ model: string; provider?: string }>,
): PrivacyTier | undefined {
  const stored = readAiModelSelection(orgId)
  return stored.model === selection.model && stored.provider === selection.provider
    ? stored.privacyTier
    : undefined
}

function isActiveSubscription(connection: ChatGptSubscriptionConnection): boolean {
  return connection.status.trim().toLocaleLowerCase() === 'active'
}

/** The active connection to send on: `preferredId` while it is still active, else the first active one. */
function activeSubscriptionConnection(
  connections: readonly ChatGptSubscriptionConnection[],
  preferredId: string | undefined,
): ChatGptSubscriptionConnection | undefined {
  return connections.find((connection) => isActiveSubscription(connection) && connection.id === preferredId)
    ?? connections.find(isActiveSubscription)
}

/** Resolve the org-owned connection at send time. Persisted connection ids are
 * hints only: reconnect can replace one, so a fresh active connection wins. */
export async function resolveAiModelSelection(orgId: string): Promise<AiModelSelection> {
  const selected = readAiModelSelection(orgId)
  if (selected.provider !== OPENAI_CODEX_SUBSCRIPTION_PROVIDER) return selected
  const connections = await listChatGptSubscriptions(orgId)
  const active = activeSubscriptionConnection(connections, selected.subscriptionConnectionId)
  if (!active) throw new Error('ChatGPT-abonnementet er ikke tilkoblet. Koble til under Profil → ChatGPT-abonnement, eller velg en annen modell.')
  return { ...selected, subscriptionConnectionId: active.id }
}

/**
 * The connection a ChatGPT-subscription turn is sent on again (regenerate,
 * "Prøv igjen", continue, rerun, edit). The turn stored the connection it was
 * first sent on, but that id is a hint only. Reconnecting ChatGPT creates a
 * new connection and leaves the old one waiting for renewal, so resending the
 * stored id after the reader reconnected, as the reauth notice asks, would be
 * refused with the same notice again.
 *
 * The stored id is kept while that connection is active; otherwise the active
 * one is used. With none active, or when the lookup fails, the stored id is
 * kept, so the broker says why it cannot answer. Only the connection is
 * chosen here: the provider and model are never changed.
 */
export async function resolveReplaySubscriptionConnectionId(
  orgId: string,
  storedId: string | undefined,
): Promise<string | undefined> {
  if (!orgId.trim()) return storedId
  try {
    return activeSubscriptionConnection(await listChatGptSubscriptions(orgId), storedId)?.id ?? storedId
  } catch {
    return storedId
  }
}

export function resetAiModelSelectionForTests(orgId: string): void {
  try {
    storage()?.removeItem(storageKey(orgId))
  } catch {
    // Test helper only.
  }
}
