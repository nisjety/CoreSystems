import { NextRequest, NextResponse } from 'next/server'

const AUTH_SERVICE_URL = process.env.AUTH_SERVICE_URL || 'http://auth-service:3011'
const INTERNAL_SERVICE_SECRET =
  process.env.INTERNAL_SERVICE_SECRET || process.env.INTERNAL_API_KEY || 'change-me-internal-service-secret'

function buildHeaders(): Record<string, string> {
  return {
    'Content-Type': 'application/json',
    'x-internal-service-secret': INTERNAL_SERVICE_SECRET,
  }
}

// GET /api/admin/users/[id]
export async function GET(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  try {
    const { id } = await params
    const res = await fetch(`${AUTH_SERVICE_URL}/api/v2/auth/admin/users/get`, {
      method: 'POST',
      headers: buildHeaders(),
      body: JSON.stringify({ userId: id }),
      signal: AbortSignal.timeout(10_000),
    })
    const data = await res.json()
    return NextResponse.json(data, { status: res.status })
  } catch (error: any) {
    return NextResponse.json({ error: error.message || 'Failed to fetch user' }, { status: 500 })
  }
}

// PUT /api/admin/users/[id]  — update role/status
export async function PUT(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  try {
    const { id } = await params
    const body = await request.json()

    // If body has a role field, update role; otherwise suspend/unsuspend
    if (body.role !== undefined) {
      const res = await fetch(`${AUTH_SERVICE_URL}/api/v2/auth/admin/users/set-role`, {
        method: 'POST',
        headers: buildHeaders(),
        body: JSON.stringify({ userId: id, role: body.role }),
        signal: AbortSignal.timeout(10_000),
      })
      const data = await res.json()
      return NextResponse.json(data, { status: res.status })
    }

    // Default: return the user info after a no-op update (some fields are read-only)
    const res = await fetch(`${AUTH_SERVICE_URL}/api/v2/auth/admin/users/get`, {
      method: 'POST',
      headers: buildHeaders(),
      body: JSON.stringify({ userId: id }),
      signal: AbortSignal.timeout(10_000),
    })
    const data = await res.json()
    return NextResponse.json(data, { status: res.status })
  } catch (error: any) {
    return NextResponse.json({ error: error.message || 'Failed to update user' }, { status: 500 })
  }
}

// DELETE /api/admin/users/[id]
export async function DELETE(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  try {
    const { id } = await params
    const res = await fetch(`${AUTH_SERVICE_URL}/api/v2/auth/admin/users/remove`, {
      method: 'POST',
      headers: buildHeaders(),
      body: JSON.stringify({ userId: id }),
      signal: AbortSignal.timeout(10_000),
    })
    const data = await res.json().catch(() => ({ message: 'Deleted' }))
    return NextResponse.json(data, { status: res.status })
  } catch (error: any) {
    return NextResponse.json({ error: error.message || 'Failed to delete user' }, { status: 500 })
  }
}
