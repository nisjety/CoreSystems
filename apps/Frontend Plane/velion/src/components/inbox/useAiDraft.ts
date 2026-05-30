'use client'

import { useCallback, useRef, useState } from 'react'

interface ConversationMessage {
  role: 'user' | 'assistant'
  content: string
}

interface UseAiDraftOptions {
  conversationHistory: ConversationMessage[]
  customerName: string
  agentId?: string
  onDraftReady: (draft: string) => void
}

interface UseAiDraftResult {
  suggestReply: () => void
  isStreaming: boolean
  error: string | null
  cancelDraft: () => void
  /** True when the current replyText was set by AI (resets when user types) */
  wasAiGenerated: boolean
  /** Call when user has manually edited the draft — clears wasAiGenerated */
  markEdited: () => void
}

export function useAiDraft({
  conversationHistory,
  customerName,
  agentId,
  onDraftReady,
}: UseAiDraftOptions): UseAiDraftResult {
  const [isStreaming, setIsStreaming] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [wasAiGenerated, setWasAiGenerated] = useState(false)
  const abortRef = useRef<AbortController | null>(null)

  const cancelDraft = useCallback(() => {
    abortRef.current?.abort()
    setIsStreaming(false)
  }, [])

  const suggestReply = useCallback(async () => {
    if (isStreaming) return

    abortRef.current?.abort()
    const controller = new AbortController()
    abortRef.current = controller

    setIsStreaming(true)
    setError(null)

    let accumulated = ''

    try {
      const response = await fetch('/api/inbox/draft', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          conversationHistory,
          customerName,
          agentId,
        }),
        signal: controller.signal,
      })

      if (!response.ok) {
        const data = await response.json().catch(() => ({}))
        throw new Error(data.error ?? `Request failed: ${response.status}`)
      }

      if (!response.body) {
        throw new Error('No response body')
      }

      const reader = response.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''

      while (true) {
        const { done, value } = await reader.read()
        if (done) break

        buffer += decoder.decode(value, { stream: true })
        const lines = buffer.split('\n')
        buffer = lines.pop() ?? ''

        for (const line of lines) {
          if (!line.startsWith('data: ')) continue
          const raw = line.slice(6).trim()
          if (!raw || raw === '[DONE]') continue

          try {
            const parsed: unknown = JSON.parse(raw)
            if (
              parsed &&
              typeof parsed === 'object' &&
              'type' in parsed &&
              'content' in parsed &&
              parsed.type === 'chunk' &&
              typeof parsed.content === 'string'
            ) {
              accumulated += parsed.content
              onDraftReady(accumulated)
            }
          } catch {
            // malformed SSE chunk — skip
          }
        }
        if (accumulated) {
          setWasAiGenerated(true)
        }
      }
    } catch (err) {
      if (err instanceof Error && err.name === 'AbortError') return
      setError(err instanceof Error ? err.message : 'Failed to generate draft')
    } finally {
      setIsStreaming(false)
    }
  }, [isStreaming, conversationHistory, customerName, agentId, onDraftReady])

  const markEdited = useCallback(() => setWasAiGenerated(false), [])

  return { suggestReply, isStreaming, error, cancelDraft, wasAiGenerated, markEdited }
}
