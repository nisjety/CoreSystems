import { requestJson } from './http'

export type InboxWorkspaceState = {
  pinnedConversationIds: string[]
  readConversationIds: string[]
}

export function getInboxWorkspaceState(signal?: AbortSignal): Promise<InboxWorkspaceState> {
  return requestJson<InboxWorkspaceState>('/api/v1/inbox/workspace', { signal })
}

export function setInboxConversationPinned(conversationId: string, enabled: boolean): Promise<InboxWorkspaceState> {
  return requestJson<InboxWorkspaceState>('/api/v1/inbox/workspace/pins', {
    method: 'POST',
    body: JSON.stringify({ conversationId, enabled }),
  })
}

export function setInboxConversationRead(conversationId: string, enabled: boolean): Promise<InboxWorkspaceState> {
  return requestJson<InboxWorkspaceState>('/api/v1/inbox/workspace/read', {
    method: 'POST',
    body: JSON.stringify({ conversationId, enabled }),
  })
}
