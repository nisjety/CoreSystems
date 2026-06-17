import { NextRequest, NextResponse } from 'next/server'

const AUTH_SERVICE_URL = process.env.AUTH_SERVICE_URL || 'http://auth-service:3011'
const INTERNAL_SERVICE_SECRET =
  (process.env.INTERNAL_SERVICE_SECRET || process.env.INTERNAL_API_KEY) as string
if (!INTERNAL_SERVICE_SECRET) throw new Error('INTERNAL_SERVICE_SECRET or INTERNAL_API_KEY must be set')

function buildHeaders(): Record<string, string> {
  return {
    'Content-Type': 'application/json',
    'x-internal-service-secret': INTERNAL_SERVICE_SECRET,
  }
}

// GET /api/admin/users?page=1&search=...
export async function GET(request: NextRequest) {
  try {
    const { searchParams } = request.nextUrl
    const page = Number(searchParams.get('page') ?? 1)
    const search = searchParams.get('search') ?? undefined
    const limit = Number(searchParams.get('limit') ?? 20)
    const offset = (page - 1) * limit

    const res = await fetch(`${AUTH_SERVICE_URL}/api/v2/auth/admin/users/list`, {
      method: 'POST',
      headers: buildHeaders(),
      body: JSON.stringify({ limit, offset, search }),
      signal: AbortSignal.timeout(10_000),
    })

    const data = await res.json()
    return NextResponse.json(data, { status: res.status })
  } catch (error: any) {
    return NextResponse.json({ error: error.message || 'Failed to fetch users' }, { status: 500 })
  }
}

// POST /api/admin/users  (create user)
export async function POST(request: NextRequest) {
  try {
    const body = await request.json()
    const res = await fetch(`${AUTH_SERVICE_URL}/api/v2/auth/admin/users/create`, {
      method: 'POST',
      headers: buildHeaders(),
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(10_000),
    })

    const data = await res.json()
    return NextResponse.json(data, { status: res.status })
  } catch (error: any) {
    return NextResponse.json({ error: error.message || 'Failed to create user' }, { status: 500 })
  }
}
