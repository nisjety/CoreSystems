'use client'

import React from 'react'
import { Bell, Mail, Smartphone } from 'lucide-react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ChannelConfig, NotificationChannel, NotificationEventType } from '@/lib/notifications/types'
import { ALL_EVENT_TYPES, EVENT_TYPE_LABELS } from '@/lib/notifications/events'

const CHANNEL_LABELS: Record<NotificationChannel, { label: string; icon: React.ReactNode }> = {
  in_app: { label: 'In-app', icon: <Smartphone className="h-4 w-4" /> },
  email: { label: 'Email', icon: <Mail className="h-4 w-4" /> },
}

async function fetchConfigs(): Promise<ChannelConfig[]> {
  const res = await fetch('/api/notifications/channels', { credentials: 'include' })
  if (!res.ok) throw new Error('Failed to load notification settings')
  const data = (await res.json()) as { configs: ChannelConfig[] }
  return data.configs ?? []
}

async function patchChannel(
  eventType: NotificationEventType,
  channel: NotificationChannel,
  enabled: boolean,
): Promise<void> {
  const res = await fetch(
    `/api/notifications/channels/${encodeURIComponent(eventType)}/${encodeURIComponent(channel)}`,
    {
      method: 'PATCH',
      credentials: 'include',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled }),
    },
  )
  if (!res.ok) throw new Error('Failed to update notification setting')
}

function isEnabled(configs: ChannelConfig[], eventType: NotificationEventType, channel: NotificationChannel): boolean {
  const found = configs.find((c) => c.event_type === eventType && c.channel === channel)
  return found?.enabled ?? false
}

export default function NotificationSettingsPage() {
  const queryClient = useQueryClient()

  const { data: configs = [], isLoading, error } = useQuery({
    queryKey: ['notifications', 'channels', 'config'],
    queryFn: fetchConfigs,
  })

  const toggle = useMutation({
    mutationFn: ({ eventType, channel, enabled }: {
      eventType: NotificationEventType
      channel: NotificationChannel
      enabled: boolean
    }) => patchChannel(eventType, channel, enabled),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['notifications', 'channels', 'config'] }),
  })

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-white">
      <div className="mx-auto w-full max-w-[580px] px-6 py-10">
        <h1 className="mb-10 text-[22px] font-semibold tracking-tight text-[#111111]">
          Notification channels
        </h1>

        <div className="mb-2 grid grid-cols-[1fr_100px_100px] items-center border-b border-[#F0F0F0] pb-2">
          <span className="text-[11px] font-medium uppercase tracking-wide text-[#9BA3AF]">Event</span>
          {(['in_app', 'email'] as NotificationChannel[]).map((ch) => (
            <div key={ch} className="flex items-center justify-center gap-1.5 text-[11px] font-medium uppercase tracking-wide text-[#9BA3AF]">
              {CHANNEL_LABELS[ch].icon}
              {CHANNEL_LABELS[ch].label}
            </div>
          ))}
        </div>

        {isLoading && (
          <div className="flex items-center justify-center py-12">
            <div className="h-4 w-4 animate-spin rounded-full border-2 border-[#E0E0E0] border-t-[#111111]" />
          </div>
        )}

        {error && (
          <div className="py-8 text-[13px] text-[#DC2626]">
            Failed to load notification settings.
          </div>
        )}

        {!isLoading && !error && ALL_EVENT_TYPES.map((eventType, i) => (
          <div
            key={eventType}
            className={`grid grid-cols-[1fr_100px_100px] items-center py-4 ${
              i < ALL_EVENT_TYPES.length - 1 ? 'border-b border-[#F0F0F0]' : ''
            }`}
          >
            <div>
              <div className="flex items-center gap-2">
                <Bell className="h-3.5 w-3.5 text-[#6B7280]" />
                <span className="text-[13px] font-medium text-[#111111]">
                  {EVENT_TYPE_LABELS[eventType]}
                </span>
              </div>
              <span className="mt-0.5 block text-[11px] text-[#9BA3AF]">{eventType}</span>
            </div>

            {(['in_app', 'email'] as NotificationChannel[]).map((channel) => {
              const enabled = isEnabled(configs, eventType, channel)
              const isPending = toggle.isPending && toggle.variables?.eventType === eventType && toggle.variables?.channel === channel

              return (
                <div key={channel} className="flex items-center justify-center">
                  <button
                    type="button"
                    role="switch"
                    aria-checked={enabled}
                    disabled={isPending}
                    onClick={() => toggle.mutate({ eventType, channel, enabled: !enabled })}
                    className={`relative h-6 w-10 rounded-full transition-colors disabled:opacity-50 ${
                      enabled ? 'bg-[#111111]' : 'bg-[#D8D8D8]'
                    }`}
                  >
                    <span
                      className={`absolute top-0.5 h-5 w-5 rounded-full bg-white shadow-sm transition-transform ${
                        enabled ? 'translate-x-4' : 'translate-x-0.5'
                      }`}
                    />
                  </button>
                </div>
              )
            })}
          </div>
        ))}

        <p className="mt-8 text-[12px] leading-5 text-[#9BA3AF]">
          <strong className="font-medium text-[#6B7280]">In-app</strong> notifications appear in the sidebar bell and the notifications page.{' '}
          <strong className="font-medium text-[#6B7280]">Email</strong> notifications are sent to your registered email address.
        </p>
      </div>
    </div>
  )
}
