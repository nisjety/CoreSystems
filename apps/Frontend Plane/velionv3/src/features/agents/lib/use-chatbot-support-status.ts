import { createQuery } from '@tanstack/solid-query'
import { requestJson } from '@/shared/api/http'

export type SupportIntegrationStatus = {
  status: 'loading' | 'connected' | 'not-configured' | 'error'
  agents: number
  groups: number
  macros: number
  message: string
}

type ChatbotRuntimeResponse = {
  support: {
    configured: boolean
    connected: boolean
    agents: number
    groups: number
    macros: number
    message: string
  }
}

export const initialSupportIntegrationStatus: SupportIntegrationStatus = {
  status: 'loading',
  agents: 0,
  groups: 0,
  macros: 0,
  message: 'Checking support integration…',
}

export const chatbotSupportStatusQueryKey = ['agents', 'chatbot', 'runtime'] as const

export async function fetchChatbotSupportStatus(): Promise<SupportIntegrationStatus> {
  const payload = await requestJson<ChatbotRuntimeResponse>('/api/v1/agents/chatbot/runtime')
  const support = payload.support

  return {
    status: support.connected ? 'connected' : support.configured ? 'error' : 'not-configured',
    agents: support.agents,
    groups: support.groups,
    macros: support.macros,
    message: support.message,
  }
}

export function createChatbotSupportStatusQuery() {
  return createQuery(() => ({
    queryFn: fetchChatbotSupportStatus,
    queryKey: chatbotSupportStatusQueryKey,
    retry: false,
    staleTime: 30_000,
  }))
}

export function resolveChatbotSupportStatus(query: ReturnType<typeof createChatbotSupportStatusQuery>): SupportIntegrationStatus {
  if (query.data) return query.data
  if (query.isError) {
    return {
      status: 'error',
      agents: 0,
      groups: 0,
      macros: 0,
      message: query.error instanceof Error ? query.error.message : 'Support integration could not be checked.',
    }
  }

  return initialSupportIntegrationStatus
}
