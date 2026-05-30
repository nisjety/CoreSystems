import { NextRequest, NextResponse } from 'next/server'
import { getSessionUser } from '../../_lib/auth-session'
import { countUnread } from '@/lib/notifications/client'

export async function GET(request: NextRequest) {
  try {
    const user = await getSessionUser(request)
    if (!user?.id) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }
    const result = await countUnread(user.id)
    return NextResponse.json(result)
  } catch (error) {
    // notification-core's feed API is not yet implemented — return zero count
    // so the corebar badge doesn't show an error state.
    console.warn('[api/notifications/unread/count] Upstream unavailable, returning 0:', error)
    return NextResponse.json({ count: 0 })
  }
}
