import { NextRequest, NextResponse } from 'next/server'
import { getSessionUser } from '../../_lib/auth-session'
import { countUnseen } from '@/lib/notifications/client'

export async function GET(request: NextRequest) {
  try {
    const user = await getSessionUser(request)
    if (!user?.id) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }

    const result = await countUnseen(user.id)
    return NextResponse.json(result)
  } catch (error) {
    console.error('[api/notifications/unseen/count] Failed:', error)
    return NextResponse.json({ count: 0 })
  }
}
