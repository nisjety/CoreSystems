import { NextRequest, NextResponse } from 'next/server'
import { getSessionUser } from '../../../../notifications/_lib/auth-session'

const NOTIFICATION_SERVICE_URL = (
  process.env.NOTIFICATION_SERVICE_URL ?? 'http://notification-core:3140'
).replace(/\/$/, '')

function getInternalKey(): string {
  const key =
    process.env.NOTIFICATION_INTERNAL_KEY ??
    process.env.INTERNAL_API_KEY ??
    process.env.INTERNAL_SERVICE_SECRET

  if (!key) {
    throw new Error('NOTIFICATION_INTERNAL_KEY (or INTERNAL_API_KEY) is not configured')
  }

  return key
}

interface RouteParams {
  params: Promise<{ id: string }>
}

export async function POST(request: NextRequest, { params }: RouteParams) {
  try {
    const { id } = await params
    const notificationId = id?.trim()

    if (!notificationId || !/^[a-zA-Z0-9_-]{1,128}$/.test(notificationId)) {
      return NextResponse.json({ error: 'Invalid notification ID' }, { status: 400 })
    }

    const user = await getSessionUser(request)

    if (!user?.id) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }

    const res = await fetch(
      `${NOTIFICATION_SERVICE_URL}/api/v1/notifications/${encodeURIComponent(notificationId)}/read`,
      {
        method: 'POST',
        headers: {
          'x-internal-api-key': getInternalKey(),
          'x-user-id': user.id,
          'Content-Type': 'application/json',
        },
        cache: 'no-store',
      },
    )

    if (!res.ok) {
      // Degrade gracefully when the read endpoint isn't implemented yet
      if (res.status === 404 || res.status === 501) {
        return NextResponse.json({ ok: true })
      }

      const text = await res.text().catch(() => '')
      return NextResponse.json(
        { error: `Upstream error ${res.status}: ${text}` },
        { status: res.status },
      )
    }

    return NextResponse.json({ ok: true })
  } catch (error) {
    const message = error instanceof Error ? error.message : 'Failed to mark as read'
    console.warn('[api/support/notifications/read] Failed:', message)
    // Return ok:true so the UI can optimistically update without breaking
    return NextResponse.json({ ok: true })
  }
}
