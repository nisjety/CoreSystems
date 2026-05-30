import { NextRequest, NextResponse } from 'next/server'

import {
  ChatStoreError,
  deleteChatSession,
  getChatSession,
  resolveChatActor,
  updateChatSessionTitle,
} from '../../_lib/session-store'

type Params = Promise<{ sessionId: string }>

export const dynamic = 'force-dynamic'
export const revalidate = 0

export async function GET(_request: NextRequest, { params }: { params: Params }) {
  try {
    const actor = await resolveChatActor()
    const { sessionId } = await params
    const session = await getChatSession(actor, sessionId)

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
  } catch (error) {
    const statusCode = error instanceof ChatStoreError ? error.statusCode : 500
    const message = error instanceof Error ? error.message : 'Failed to load chat session'

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

export async function PATCH(request: NextRequest, { params }: { params: Params }) {
  try {
    const actor = await resolveChatActor()
    const { sessionId } = await params
    const body = await request.json()
    const title = body.title

    if (!title) {
      return NextResponse.json({ error: 'Bad request' }, { status: 400 })
    }

    const session = await updateChatSessionTitle(actor, sessionId, title)
    return NextResponse.json(session)
  } catch (error) {
    const statusCode = error instanceof ChatStoreError ? error.statusCode : 400
    const message = error instanceof Error ? error.message : 'Failed to update chat session'

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

export async function DELETE(_request: NextRequest, { params }: { params: Params }) {
  try {
    const actor = await resolveChatActor()
    const { sessionId } = await params
    await deleteChatSession(actor, sessionId)
    return new Response(null, { status: 204 })
  } catch (error) {
    const statusCode = error instanceof ChatStoreError ? error.statusCode : 500
    const message = error instanceof Error ? error.message : 'Failed to delete chat session'

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
