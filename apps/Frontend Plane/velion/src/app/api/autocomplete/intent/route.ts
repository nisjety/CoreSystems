import { NextRequest, NextResponse } from 'next/server'

import { ChatStoreError, resolveChatActor } from '../../chat/_lib/session-store'

const AI_CORE_URL =
  process.env.AI_CORE_URL ||
  process.env.AI_CORE_API_URL ||
  'http://localhost:8100'

const INTERNAL_API_KEY =
  (process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET) as string

if (!INTERNAL_API_KEY) {
  throw new Error(
    'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET environment variable is required for inter-service authentication'
  )
}

export async function POST(request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    const body = (await request.json()) as { text?: string; cursor_pos?: number }

    const upstream = await fetch(`${AI_CORE_URL}/v1/autocomplete/intent`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Internal-Api-Key': INTERNAL_API_KEY,
      },
      body: JSON.stringify({
        text: body.text ?? '',
        cursor_pos: body.cursor_pos ?? null,
        org_id: actor.orgId,
        user_id: actor.userId,
      }),
      cache: 'no-store',
    })

    if (!upstream.ok) {
      return NextResponse.json({ entity: null, context_window: '' })
    }

    const data = await upstream.json()
    return NextResponse.json(data)
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    return NextResponse.json({ entity: null, context_window: '' })
  }
}
