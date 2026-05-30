import { NextResponse } from 'next/server'

import { ChatStoreError, resolveChatActor } from '../_lib/session-store'

export const dynamic = 'force-dynamic'
export const revalidate = 0

export async function GET() {
  try {
    const actor = await resolveChatActor()

    return NextResponse.json(actor, {
      headers: {
        'Cache-Control': 'no-store',
      },
    })
  } catch (error) {
    const statusCode = error instanceof ChatStoreError ? error.statusCode : 500
    const message = error instanceof Error ? error.message : 'Failed to resolve chat actor'

    // Log on the server so operators can debug — the client only sees
    // the public message, not the underlying stack. ChatStoreError is a
    // domain-expected failure (auth, upstream), so log it at warn level.
    console.error('[chat/actor] resolve failed', {
      statusCode,
      message,
      stack: error instanceof Error ? error.stack : undefined,
    })

    return NextResponse.json(
      {
        error: 'Chat actor unavailable',
        message,
        statusCode,
      },
      { status: statusCode },
    )
  }
}
