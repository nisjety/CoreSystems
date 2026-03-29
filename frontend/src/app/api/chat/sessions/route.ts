import { NextRequest, NextResponse } from 'next/server'

import { createChatSession, listChatSessions } from '../_lib/session-store'

export const dynamic = 'force-dynamic'
export const revalidate = 0

export async function GET() {
  const sessions = listChatSessions()

  return NextResponse.json(
    { sessions, totalCount: sessions.length },
    {
      headers: {
        'Cache-Control': 'no-store',
      },
    },
  )
}

export async function POST(request: NextRequest) {
  let title = 'Ny samtale'
  let userId: string | undefined

  try {
    const body = await request.json()
    if (body.title) title = body.title
    if (body.userId) userId = body.userId
  } catch {
    // use default title
  }

  const session = createChatSession(title, userId)

  return NextResponse.json(session, { status: 201 })
}
