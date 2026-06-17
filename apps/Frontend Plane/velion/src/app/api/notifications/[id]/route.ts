import { NextRequest, NextResponse } from 'next/server'
import { getSessionUser } from '../_lib/auth-session'
import { deleteNotification } from '@/lib/notifications/client'

interface RouteParams {
  params: { id: string }
}

export async function DELETE(request: NextRequest, { params }: RouteParams) {
  try {
    const notificationId = params.id?.trim()
    if (!/^[a-zA-Z0-9_-]{1,128}$/.test(notificationId)) {
      return NextResponse.json({ error: 'Invalid notification ID' }, { status: 400 })
    }

    const user = await getSessionUser(request)
    if (!user?.id) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }

    const permanent = request.nextUrl.searchParams.get('permanent') === 'true'
    await deleteNotification(user.id, notificationId, permanent)
    return NextResponse.json({ ok: true })
  } catch (error) {
    console.error(`[api/notifications/${params.id}] DELETE failed:`, error)
    return NextResponse.json({ error: 'Failed to delete notification' }, { status: 500 })
  }
}
