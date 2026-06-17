import { NextRequest, NextResponse } from 'next/server'
import { getSessionUser } from '../../_lib/auth-session'
import { markRead } from '@/lib/notifications/client'

interface RouteParams {
  params: { id: string }
}

export async function POST(request: NextRequest, { params }: RouteParams) {
  try {
    const notificationId = params.id?.trim()
    if (!/^[a-zA-Z0-9_-]{1,128}$/.test(notificationId)) {
      return NextResponse.json({ error: 'Invalid notification ID' }, { status: 400 })
    }

    const user = await getSessionUser(request)
    if (!user?.id) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }

    await markRead(user.id, notificationId)
    return NextResponse.json({ ok: true })
  } catch (error) {
    console.error(`[api/notifications/${params.id}/read] Failed:`, error)
    return NextResponse.json({ error: 'Failed to mark as read' }, { status: 500 })
  }
}
