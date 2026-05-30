import { NextRequest, NextResponse } from 'next/server'
import { getSessionUser } from '../../notifications/_lib/auth-session'

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

export async function GET(request: NextRequest) {
  try {
    const user = await getSessionUser(request)

    if (!user?.id) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }

    // Fetch notifications filtered to the support feed / source
    const qs = new URLSearchParams({
      recipient_id: user.id,
      feed: 'support',
      limit: '50',
    })

    const res = await fetch(
      `${NOTIFICATION_SERVICE_URL}/api/v1/notifications?${qs.toString()}`,
      {
        headers: {
          'x-internal-api-key': getInternalKey(),
          'x-user-id': user.id,
          'Content-Type': 'application/json',
        },
        cache: 'no-store',
      },
    )

    if (!res.ok) {
      // Degrade gracefully — the support feed endpoint may not exist yet
      if (res.status === 404 || res.status === 501) {
        return NextResponse.json({ notifications: [], total_count: 0, has_more: false })
      }

      const text = await res.text().catch(() => '')
      return NextResponse.json(
        { error: `Upstream error ${res.status}: ${text}` },
        { status: res.status },
      )
    }

    const data: unknown = await res.json()
    return NextResponse.json(data)
  } catch (error) {
    // Degrade gracefully when notification-core is unreachable
    const message = error instanceof Error ? error.message : 'Upstream unavailable'
    console.warn('[api/support/notifications] Upstream unavailable:', message)
    return NextResponse.json({ notifications: [], total_count: 0, has_more: false })
  }
}
