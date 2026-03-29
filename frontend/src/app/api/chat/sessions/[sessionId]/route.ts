import { NextRequest, NextResponse } from 'next/server'

import {
  deleteChatSession,
  getChatSession,
  updateChatSessionTitle,
} from '../../_lib/session-store'

type Params = Promise<{ sessionId: string }>

export const dynamic = 'force-dynamic'
export const revalidate = 0

export async function GET(_request: NextRequest, { params }: { params: Params }) {
  const { sessionId } = await params
  const session = getChatSession(sessionId)

  if (!session) {
    return NextResponse.json(
      {
        error: 'Not found',
        message: 'Chat session not found',
        statusCode: 404,
      },
      {
        status: 404,
        headers: {
          'Cache-Control': 'no-store',
        },
      },
    )
  }

  return NextResponse.json(session, {
    headers: {
      'Cache-Control': 'no-store',
    },
  })
}

export async function PATCH(request: NextRequest, { params }: { params: Params }) {
  const { sessionId } = await params
  let title: string | undefined

  try {
    const body = await request.json()
    title = body.title
  } catch {
    return NextResponse.json({ error: 'Bad request' }, { status: 400 })
  }

  const session = title
    ? updateChatSessionTitle(sessionId, title)
    : ensureChatSession({ sessionId, title: 'Samtale' })

  return NextResponse.json(session)
}

export async function DELETE(_request: NextRequest, { params }: { params: Params }) {
  const { sessionId } = await params
  deleteChatSession(sessionId)
  return new Response(null, { status: 204 })
}
