import { NextRequest, NextResponse } from 'next/server'
import { getSessionUser } from '../_lib/auth-session'
import { listPreferences } from '@/lib/notifications/client'

export async function GET(request: NextRequest) {
  try {
    const user = await getSessionUser(request)
    if (!user?.id) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }

    const preferences = await listPreferences(user.id)
    return NextResponse.json({ preferences })
  } catch (error) {
    console.error('[api/notifications/preferences] Failed:', error)
    return NextResponse.json({ error: 'Failed to fetch preferences' }, { status: 500 })
  }
}
