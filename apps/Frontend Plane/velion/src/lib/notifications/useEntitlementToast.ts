'use client'

// G44 — Entitlement toast feed.
//
// notification-core's control-session subscriber (G14 §8.17) converts every
// `app.session.entitlements_changed` NATS event into a `Notification` row
// with `event_type='control_session.entitlements_changed'`. This hook
// watches the notifications feed and fires a sonner toast for any *new*
// such entry that arrives while the dashboard is mounted.
//
// The "first-paint" entitlement notifications (i.e. ones already in the
// feed at mount time) are suppressed via a per-mount baseline — otherwise
// every page load would re-toast historical events. We only toast for IDs
// that materialise *after* the baseline snapshot.

import { useEffect, useRef } from 'react'
import { toast } from 'sonner'

import { useNotifications } from '@/components/core/navbar/hooks/useCorebar'
import type { Notification } from '@/lib/notifications/types'

const ENTITLEMENTS_TYPE: Notification['event_type'] = 'control_session.entitlements_changed'

/**
 * Mount once in the dashboard layout. Returns nothing — fires side-effects
 * (toast) on new entitlement-changed notifications.
 *
 * Pairs with `useNotificationWS` for low-latency invalidation, but works
 * correctly even when only the polling fallback is active (the toast
 * appears within `useNotifications`'s `staleTime` window — currently 2min).
 */
export function useEntitlementToast(): void {
  const { data: notifications } = useNotifications()
  const seenIdsRef = useRef<Set<string> | null>(null)

  useEffect(() => {
    if (!notifications) return

    // First observation: capture the baseline set of entitlement notification
    // IDs already in the feed at mount time. These are historical; we don't
    // toast for them. Subsequent renders compare against this baseline.
    if (seenIdsRef.current === null) {
      seenIdsRef.current = new Set(
        notifications
          .filter((n) => n.event_type === ENTITLEMENTS_TYPE)
          .map((n) => n.id),
      )
      return
    }

    const seen = seenIdsRef.current
    for (const notification of notifications) {
      if (notification.event_type !== ENTITLEMENTS_TYPE) continue
      if (seen.has(notification.id)) continue

      // New entitlement event — toast + remember the id.
      seen.add(notification.id)
      const title = notification.title || 'Plan updated'
      const body = notification.body || 'Your entitlements have changed.'
      toast.success(title, {
        description: body,
        // Open the linked page if notification-core supplied one (e.g.
        // `/settings/billing` after a plan upgrade).
        action: notification.action_url
          ? {
              label: 'View',
              onClick: () => {
                window.location.href = notification.action_url as string
              },
            }
          : undefined,
      })
    }
  }, [notifications])
}
