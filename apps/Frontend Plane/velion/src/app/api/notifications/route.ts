import { NextRequest, NextResponse } from 'next/server'
import { getSessionUser } from './_lib/auth-session'
import { listNotifications } from '@/lib/notifications/client'

export async function GET(request: NextRequest) {
  try {
    const user = await getSessionUser(request)
    if (!user?.id) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }

    const searchParams = request.nextUrl.searchParams
    const page = Number(searchParams.get('page') ?? '0')
    const limit = Number(searchParams.get('limit') ?? '20')
    const readParam = searchParams.get('read')
    const read = readParam === 'true' ? true : readParam === 'false' ? false : undefined

    const feed = await listNotifications(user.id, { page, limit, read })
    return NextResponse.json(feed)
  } catch (error) {
    // notification-core's feed API is not yet implemented — return an empty feed
    // so the UI degrades gracefully rather than showing an error state.
    console.warn('[api/notifications] Upstream unavailable, returning empty feed:', error)
    return NextResponse.json({ notifications: [], total_count: 0, has_more: false })
  }
}
