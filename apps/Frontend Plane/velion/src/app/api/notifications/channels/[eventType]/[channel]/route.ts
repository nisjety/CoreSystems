import { NextRequest, NextResponse } from 'next/server'
import { setChannelEnabled } from '@/lib/notifications/client'
import type { NotificationChannel, NotificationEventType } from '@/lib/notifications/types'

interface RouteParams {
  params: { eventType: string; channel: string }
}

export async function PATCH(request: NextRequest, { params }: RouteParams) {
  try {
    const body = (await request.json()) as { enabled?: boolean }
    if (typeof body.enabled !== 'boolean') {
      return NextResponse.json({ error: '`enabled` boolean required' }, { status: 400 })
    }
    await setChannelEnabled(
      params.eventType as NotificationEventType,
      params.channel as NotificationChannel,
      body.enabled,
    )
    return NextResponse.json({ ok: true })
  } catch (error) {
    console.error('[api/notifications/channels/:eventType/:channel] Failed:', error)
    return NextResponse.json({ error: 'Failed to update channel config' }, { status: 500 })
  }
}
