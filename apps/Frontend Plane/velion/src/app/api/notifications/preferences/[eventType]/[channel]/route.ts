import { NextRequest, NextResponse } from 'next/server'
import { getSessionUser } from '../../../_lib/auth-session'
import { setPreference } from '@/lib/notifications/client'
import type { NotificationChannel, NotificationEventType } from '@/lib/notifications/types'

interface RouteParams {
  params: { eventType: string; channel: string }
}

export async function PUT(request: NextRequest, { params }: RouteParams) {
  try {
    const user = await getSessionUser(request)
    if (!user?.id) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }

    const body = await request.json() as { enabled?: boolean }
    if (typeof body.enabled !== 'boolean') {
      return NextResponse.json({ error: '`enabled` boolean required' }, { status: 400 })
    }

    await setPreference(
      user.id,
      params.eventType as NotificationEventType,
      params.channel as NotificationChannel,
      body.enabled,
    )
    return NextResponse.json({ ok: true })
  } catch (error) {
    console.error(`[api/notifications/preferences/${params.eventType}/${params.channel}] Failed:`, error)
    return NextResponse.json({ error: 'Failed to update preference' }, { status: 500 })
  }
}
