import { NextRequest, NextResponse } from 'next/server'

const AUTH_SERVICE_URL = process.env.AUTH_SERVICE_URL || 'http://localhost:3011'
const INTERNAL_API_KEYS = Array.from(
  new Set([process.env.INTERNAL_API_KEY, process.env.INTERNAL_SERVICE_SECRET].filter(Boolean))
) as string[]

const prefsStore = new Map<string, Record<string, unknown>>()

async function fetchSession(request: NextRequest) {
  const authUrls = [AUTH_SERVICE_URL, 'http://localhost:3011', 'http://auth-service:3011']
  const cookieHeader = request.headers.get('cookie') || ''
  const internalKey = INTERNAL_API_KEYS[0]
  for (const baseUrl of authUrls) {
    try {
      // G30 v3: use Better Auth's native /api/auth/get-session (GET).
      const res = await fetch(`${baseUrl}/api/auth/get-session`, {
        method: 'GET',
        headers: { 'Content-Type': 'application/json', 'x-internal-api-key': internalKey, cookie: cookieHeader },
      })
      if (res.ok) return await res.json()
    } catch { /* try next */ }
  }
  return null
}

export async function GET(request: NextRequest) {
  const session = await fetchSession(request)
  if (!session?.user) return NextResponse.json({ error: 'Not authenticated' }, { status: 401 })
  return NextResponse.json(prefsStore.get(session.user.id) ?? {})
}

export async function PATCH(request: NextRequest) {
  const session = await fetchSession(request)
  if (!session?.user) return NextResponse.json({ error: 'Not authenticated' }, { status: 401 })
  const body = await request.json()
  const updated = { ...(prefsStore.get(session.user.id) ?? {}), ...body }
  prefsStore.set(session.user.id, updated)
  return NextResponse.json(updated)
}
