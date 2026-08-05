'use client'

// U5-2 (ui-ux-verevon-gap.md §10): real notification preferences UI.
//
// Replaces the previous 5-boolean mock with a Novu-backed preference matrix.
// Each row is one (event_type) with per-channel toggles (in_app, email, sms,
// push). The matrix is derived by merging:
//   - notification-core /channels/config → which (event_type, channel)
//     combos the org has enabled and what the default for new subscribers is.
//   - notification-core /preferences → the user's explicit overrides.
//
// Toggles write through to /api/notifications/preferences/{eventType}/
// {channel} which in turn calls notification-core, which writes the local
// row and fans the change out to Novu via UpdateSubscriberPreference.

import { useEffect, useState } from 'react'
import { Check, Loader2 } from 'lucide-react'

import { accountService } from '../services/account-service'
import type {
  NotificationMatrix,
  NotificationMatrixEntry,
} from '../services/account-service'
import { AccountSection, InlineNotice } from './AccountFormPrimitives'

const CHANNEL_LABELS: Record<string, string> = {
  in_app: 'In-app',
  email: 'Email',
  sms: 'SMS',
  push: 'Push',
  chat: 'Chat',
}

// We always render these four columns in this order. Channels not present
// in CHANNEL_LABELS are tacked on at the end so new channels surface
// automatically once the org enables them.
const CHANNEL_ORDER = ['in_app', 'email', 'sms', 'push']

interface ToggleProps {
  checked: boolean
  available: boolean
  busy: boolean
  onChange: (next: boolean) => void
  ariaLabel: string
}

function Toggle({ checked, available, busy, onChange, ariaLabel }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={!available || busy}
      onClick={() => onChange(!checked)}
      className={[
        'relative inline-flex h-6 w-10 shrink-0 items-center rounded-full transition-colors duration-200',
        !available
          ? 'cursor-not-allowed bg-[#E5E7EB]'
          : checked
            ? 'bg-[#111111]'
            : 'bg-[#D8D8D8]',
      ].join(' ')}
    >
      <span
        className={[
          'absolute top-0.5 h-5 w-5 rounded-full bg-white shadow-sm transition-transform duration-200',
          checked ? 'translate-x-4' : 'translate-x-0.5',
        ].join(' ')}
      />
    </button>
  )
}

function getChannelOrder(entries: NotificationMatrixEntry[]): string[] {
  const seen = new Set<string>()
  for (const entry of entries) {
    for (const channel of Object.keys(entry.channels)) {
      seen.add(channel)
    }
  }
  const ordered: string[] = []
  for (const c of CHANNEL_ORDER) {
    if (seen.has(c)) ordered.push(c)
  }
  for (const c of seen) {
    if (!ordered.includes(c)) ordered.push(c)
  }
  return ordered
}

export function NotificationsSection() {
  const [matrix, setMatrix] = useState<NotificationMatrix | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  // Tracks per (eventType:channel) keys currently mid-flight so we can
  // disable the toggle and show a tiny spinner.
  const [busyKeys, setBusyKeys] = useState<Set<string>>(new Set())
  const [lastSaved, setLastSaved] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    accountService
      .getNotificationMatrix()
      .then((m) => {
        if (!cancelled) {
          setMatrix(m)
          setError(null)
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load preferences')
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  async function handleToggle(eventType: string, channel: string, next: boolean) {
    const key = `${eventType}:${channel}`
    setBusyKeys((prev) => new Set(prev).add(key))
    setMatrix((prev) => {
      if (!prev) return prev
      return {
        entries: prev.entries.map((e) =>
          e.eventType === eventType
            ? {
                ...e,
                channels: {
                  ...e.channels,
                  [channel]: {
                    ...e.channels[channel],
                    enabled: next,
                    hasOverride: true,
                  },
                },
              }
            : e,
        ),
      }
    })
    try {
      await accountService.setChannelPreference(eventType, channel, next)
      setLastSaved(`${eventType}:${channel}`)
    } catch (err) {
      // Roll back the optimistic update.
      setMatrix((prev) => {
        if (!prev) return prev
        return {
          entries: prev.entries.map((e) =>
            e.eventType === eventType
              ? {
                  ...e,
                  channels: {
                    ...e.channels,
                    [channel]: {
                      ...e.channels[channel],
                      enabled: !next,
                      hasOverride: e.channels[channel]?.hasOverride ?? false,
                    },
                  },
                }
              : e,
          ),
        }
      })
      setError(err instanceof Error ? err.message : 'Failed to save preference')
    } finally {
      setBusyKeys((prev) => {
        const updated = new Set(prev)
        updated.delete(key)
        return updated
      })
    }
  }

  return (
    <AccountSection
      id="notifications"
      title="Notifications"
      description="Control which events reach you on each channel. Defaults follow your organisation's policy; toggles here override on a per-channel basis."
    >
      {loading && (
        <div className="flex items-center gap-2 text-[13px] text-black/60">
          <Loader2 size={14} className="animate-spin" /> Loading preferences…
        </div>
      )}

      {error && !loading && (
        <InlineNotice tone="danger">
          {error} — preferences could not load. Please try again later or contact support if the error persists.
        </InlineNotice>
      )}

      {!loading && !error && matrix && (
        <>
          {matrix.entries.length === 0 ? (
            <p className="text-[13px] text-black/60">
              No notification categories configured for this organisation yet.
            </p>
          ) : (
            <NotificationMatrixTable
              matrix={matrix}
              busyKeys={busyKeys}
              onToggle={handleToggle}
            />
          )}
        </>
      )}

      {lastSaved && (
        <div className="mt-4">
          <InlineNotice tone="success">
            <span className="inline-flex items-center gap-2">
              <Check size={13} />
              Preferences saved
            </span>
          </InlineNotice>
        </div>
      )}
    </AccountSection>
  )
}

interface NotificationMatrixTableProps {
  matrix: NotificationMatrix
  busyKeys: Set<string>
  onToggle: (eventType: string, channel: string, next: boolean) => Promise<void> | void
}

function NotificationMatrixTable({
  matrix,
  busyKeys,
  onToggle,
}: NotificationMatrixTableProps) {
  const channels = getChannelOrder(matrix.entries)
  return (
    <div className="overflow-hidden rounded-md border border-[#F0F0F0]">
      <table className="w-full table-fixed border-collapse text-left text-[13px]">
        <thead>
          <tr className="border-b border-[#F0F0F0] bg-[#FAFAFA]">
            <th className="py-3 pl-4 pr-2 text-[11px] font-medium uppercase tracking-wide text-[#6B7280]">
              Event
            </th>
            {channels.map((channel) => (
              <th
                key={channel}
                className="w-[110px] py-3 px-2 text-center text-[11px] font-medium uppercase tracking-wide text-[#6B7280]"
              >
                {CHANNEL_LABELS[channel] ?? channel}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {matrix.entries.map((entry) => (
            <tr key={entry.eventType} className="border-b border-[#F0F0F0] last:border-b-0">
              <td className="py-4 pl-4 pr-2 align-top">
                <p className="font-medium text-[#111111]">{entry.label}</p>
                {entry.description && (
                  <p className="mt-1 text-[12px] leading-5 text-[#6B7280]">
                    {entry.description}
                  </p>
                )}
              </td>
              {channels.map((channel) => {
                const cell = entry.channels[channel]
                const key = `${entry.eventType}:${channel}`
                const available = Boolean(cell?.available)
                const checked = Boolean(cell?.enabled)
                return (
                  <td key={channel} className="px-2 py-4 align-middle text-center">
                    {cell ? (
                      <div className="inline-flex items-center justify-center">
                        <Toggle
                          checked={checked}
                          available={available}
                          busy={busyKeys.has(key)}
                          ariaLabel={`${entry.label} via ${CHANNEL_LABELS[channel] ?? channel}`}
                          onChange={(next) => void onToggle(entry.eventType, channel, next)}
                        />
                      </div>
                    ) : (
                      <span className="text-[11px] text-[#D1D5DB]">—</span>
                    )}
                  </td>
                )
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
