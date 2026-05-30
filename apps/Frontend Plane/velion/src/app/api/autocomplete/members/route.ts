import { NextRequest, NextResponse } from 'next/server'

import { ChatStoreError, resolveChatActor } from '../../chat/_lib/session-store'

const ORG_CORE_URL =
  process.env.ORG_CORE_URL ||
  process.env.ORG_SERVICE_URL ||
  'http://org-core:8080'

const INTERNAL_API_KEY =
  (process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET) as string

if (!INTERNAL_API_KEY) {
  throw new Error(
    'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET environment variable is required for inter-service authentication'
  )
}

export async function GET(request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    const q = request.nextUrl.searchParams.get('q') ?? ''
    const limit = request.nextUrl.searchParams.get('limit') ?? '6'

    const upstream = await fetch(
      `${ORG_CORE_URL}/orgs/${actor.orgId}/members/search?q=${encodeURIComponent(q)}&limit=${limit}`,
      {
        headers: {
          'Content-Type': 'application/json',
          'X-Internal-Api-Key': INTERNAL_API_KEY,
        },
        cache: 'no-store',
      },
    )

    if (!upstream.ok) {
      return NextResponse.json({ suggestions: [] })
    }

    // org-core returns a JSON array of MemberSuggestion — normalise to { suggestions }
    const raw = (await upstream.json()) as Array<{
      user_id: string
      display_name: string
      email: string
      avatar_url?: string
      role: string
      status: string
    }>

    return NextResponse.json({ suggestions: Array.isArray(raw) ? raw : [] })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    // Autocomplete failures should be silent — return empty rather than 500
    return NextResponse.json({ suggestions: [] })
  }
}
