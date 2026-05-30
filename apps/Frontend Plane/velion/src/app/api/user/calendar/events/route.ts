import { NextRequest, NextResponse } from 'next/server'

import { authErrorResponse, requireSession } from '@/app/api/_lib/control-plane-auth'
import { getServerSidebarCalendarEvents } from '@/lib/server/sidebar-data'

export async function GET(request: NextRequest) {
  try {
    const session = await requireSession(request)
    const events = await getServerSidebarCalendarEvents({
      userId: session.user.id,
      email: session.user.email,
      name: session.user.name ?? undefined,
      cookieHeader: request.headers.get('cookie') ?? undefined,
    })

    return NextResponse.json(events, {
      headers: {
        'Cache-Control': 'no-store',
      },
    })
  } catch (error) {
    return authErrorResponse(error)
  }
}
