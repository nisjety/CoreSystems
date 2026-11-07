import { createSignal, untrack } from 'solid-js'
import { assistantChatDraftActionSchema, type AssistantChatDraftAction } from '@/shared/actions/assistant-chat-draft'
import { getSession } from '@/shared/session/session-store'

type PendingDraft = { action: AssistantChatDraftAction; userId: string; orgId: string }

const [pendingDraft, setPendingDraft] = createSignal<PendingDraft | null>(null)
let expiry: ReturnType<typeof setTimeout> | undefined

export const pendingAssistantChatDraft = pendingDraft

export function clearAssistantChatDraft(): void {
  clearTimeout(expiry)
  expiry = undefined
  setPendingDraft(null)
}

/** The handoff crosses one SPA route in memory and has no send or storage side effect. */
export function queueAssistantChatDraft(action: AssistantChatDraftAction): boolean {
  const parsed = assistantChatDraftActionSchema.safeParse(action)
  const session = getSession()
  const userId = session.user?.id
  const orgId = session.activeOrg?.id
  if (!parsed.success || !userId || !orgId) return false

  clearTimeout(expiry)
  expiry = setTimeout(clearAssistantChatDraft, 60_000)
  setPendingDraft({ action: parsed.data, userId, orgId })
  return true
}

export function takeAssistantChatDraft(pending: PendingDraft): AssistantChatDraftAction | null {
  if (untrack(pendingDraft) !== pending) return null
  clearAssistantChatDraft()
  return untrack(() => {
    const session = getSession()
    return session.user?.id === pending.userId && session.activeOrg?.id === pending.orgId ? pending.action : null
  })
}
