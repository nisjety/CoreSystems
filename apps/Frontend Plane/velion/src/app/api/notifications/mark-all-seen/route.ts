import { NextRequest, NextResponse } from 'next/server'
import { getSessionUser } from '../_lib/auth-session'
import { markAllSeen } from '@/lib/notifications/client'

export async function POST(request: NextRequest) {
  try {
    const user = await getSessionUser(request)
    if (!user?.id) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }

    await markAllSeen(user.id)
    return NextResponse.json({ ok: true })
  } catch (error) {
    console.error('[api/notifications/mark-all-seen] Failed:', error)
    return NextResponse.json({ error: 'Failed to mark all as seen' }, { status: 500 })
  }
}
