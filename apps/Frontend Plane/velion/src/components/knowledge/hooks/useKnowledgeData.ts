'use client'

import { useQuery } from '@tanstack/react-query'

import type {
  KnowledgeDocumentsResponse,
  KnowledgeIntegrationsResponse,
  KnowledgeSourcesResponse,
} from '@/lib/integrations/types'

async function fetchJson<T>(url: string) {
  const response = await fetch(url, {
    credentials: 'include',
    cache: 'no-store',
  })

  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    const detail =
      payload && typeof payload === 'object'
        ? (payload as { error?: string }).error
        : null
    throw new Error(detail || `Request failed: ${response.status}`)
  }

  return (await response.json()) as T
}

export function useKnowledgeIntegrations() {
  return useQuery({
    queryKey: ['knowledge', 'integrations'],
    queryFn: () => fetchJson<KnowledgeIntegrationsResponse>('/api/knowledge/integrations'),
    staleTime: 30 * 1000,
    refetchOnWindowFocus: false,
  })
}

export function useKnowledgeSources() {
  return useQuery({
    queryKey: ['knowledge', 'sources'],
    queryFn: () => fetchJson<KnowledgeSourcesResponse>('/api/knowledge/sources'),
    staleTime: 30 * 1000,
    refetchOnWindowFocus: false,
  })
}

export function useKnowledgeDocuments(options?: {
  q?: string
  limit?: number
}) {
  const params = new URLSearchParams()
  if (options?.q) {
    params.set('q', options.q)
  }
  if (typeof options?.limit === 'number') {
    params.set('limit', String(options.limit))
  }

  const query = params.toString()
  const url = query ? `/api/knowledge/documents?${query}` : '/api/knowledge/documents'

  return useQuery({
    queryKey: ['knowledge', 'documents', options?.q ?? '', options?.limit ?? 0],
    queryFn: () => fetchJson<KnowledgeDocumentsResponse>(url),
    staleTime: 30 * 1000,
    refetchOnWindowFocus: false,
  })
}
