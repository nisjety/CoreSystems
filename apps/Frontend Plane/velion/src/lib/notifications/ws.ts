'use client'

import { useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { queryKeys } from '@/lib/rpc/hooks'

const MAX_RETRY_DELAY = 30_000
const BASE_RETRY_DELAY = 2_000

function getWSUrl(userID: string): string {
  const base = process.env.NEXT_PUBLIC_NOTIFICATION_WS_URL
  if (!base) return ''
  const token = process.env.NEXT_PUBLIC_NOTIFICATION_WS_TOKEN ?? ''
  return `${base}/ws?userId=${encodeURIComponent(userID)}&token=${encodeURIComponent(token)}`
}

/**
 * Opens a WebSocket to notification-core and invalidates React Query caches
 * when the server pushes notification_received or unread_count_changed events.
 *
 * Uses exponential backoff (2s → 4s → 8s → … → 30s cap) to avoid hammering
 * the server when it's unavailable. Resets backoff on successful connection.
 *
 * Falls back gracefully — if WS is unavailable, polling via refetchInterval
 * in useUnreadCount / useUnseenCount provides eventual consistency.
 */
export function useNotificationWS(userId: string | null | undefined) {
  const queryClient = useQueryClient()
  const retryTimeout = useRef<ReturnType<typeof setTimeout> | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const retryCountRef = useRef(0)

  useEffect(() => {
    if (!userId) return
    const wsUrl = getWSUrl(userId)
    if (!wsUrl) return // WS endpoint not configured — polling fallback is active

    let active = true

    function connect() {
      if (!active) return

      const ws = new WebSocket(wsUrl)
      wsRef.current = ws

      ws.onopen = () => {
        // Reset backoff on successful connection
        retryCountRef.current = 0
      }

      ws.onmessage = (event) => {
        try {
          const msg = JSON.parse(event.data as string) as { event: string }
          if (
            msg.event === 'notification_received' ||
            msg.event === 'unread_count_changed'
          ) {
            void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.unreadCount })
            void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all })
          }
          if (msg.event === 'unseen_count_changed') {
            void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.unseenCount })
          }
        } catch {
          // malformed frame — ignore
        }
      }

      ws.onclose = () => {
        wsRef.current = null
        if (active) {
          // Exponential backoff: 2s, 4s, 8s, 16s, 30s cap
          const delay = Math.min(
            BASE_RETRY_DELAY * 2 ** retryCountRef.current,
            MAX_RETRY_DELAY,
          )
          retryCountRef.current += 1
          retryTimeout.current = setTimeout(connect, delay)
        }
      }

      ws.onerror = () => {
        ws.close()
      }
    }

    connect()

    return () => {
      active = false
      if (retryTimeout.current) clearTimeout(retryTimeout.current)
      const ws = wsRef.current
      wsRef.current = null
      if (ws) {
        // Null handlers first so no callbacks fire during teardown (handles
        // React Strict Mode where cleanup runs while socket is still CONNECTING).
        ws.onopen = null
        ws.onclose = null
        ws.onerror = null
        ws.onmessage = null
        ws.close()
      }
    }
  }, [userId, queryClient])
}
