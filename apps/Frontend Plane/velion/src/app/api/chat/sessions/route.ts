import { NextRequest, NextResponse } from 'next/server'

import {
  ChatStoreError,
  createChatSession,
  listChatSessions,
  resolveChatActor,
} from '../_lib/session-store'

export const dynamic = 'force-dynamic'
export const revalidate = 0

export async function GET() {
  try {
    const actor = await resolveChatActor()
    const sessions = await listChatSessions(actor)

    return NextResponse.json(
      { sessions, totalCount: sessions.length },
      {
        headers: {
          'Cache-Control': 'no-store',
        },
      },
    )
  } catch (error) {
    const statusCode = error instanceof ChatStoreError ? error.statusCode : 500
    const message = error instanceof Error ? error.message : 'Failed to load chat sessions'

    return NextResponse.json(
      {
        error: 'Chat sessions unavailable',
        message,
        statusCode,
      },
      { status: statusCode },
    )
  }
}

export async function POST(request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    let title = 'Ny samtale'

    const body = await request.json()
    if (body.title) title = body.title

    const session = await createChatSession(actor, title)

    return NextResponse.json(session, { status: 201 })
  } catch (error) {
    const statusCode = error instanceof ChatStoreError ? error.statusCode : 500
    const message = error instanceof Error ? error.message : 'Failed to create chat session'

    return NextResponse.json(
      {
        error: 'Chat session unavailable',
        message,
        statusCode,
      },
      { status: statusCode },
    )
  }
}
