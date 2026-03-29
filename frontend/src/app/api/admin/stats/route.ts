import { NextResponse } from 'next/server'

const AUTH_SERVICE_URL = process.env.AUTH_SERVICE_URL || 'http://auth-service:3011'
const INTERNAL_SERVICE_SECRET =
  process.env.INTERNAL_SERVICE_SECRET || process.env.INTERNAL_API_KEY || 'change-me-internal-service-secret'

// GET /api/admin/stats
export async function GET() {
  try {
    const res = await fetch(`${AUTH_SERVICE_URL}/api/v2/auth/admin/system/stats`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'x-internal-service-secret': INTERNAL_SERVICE_SECRET,
      },
      body: JSON.stringify({}),
      signal: AbortSignal.timeout(8_000),
    })

    const data = (await res.json()) as {
      success?: boolean
      totalUsers?: number
      totalOrganizations?: number
      activeUsers?: number
      suspendedUsers?: number
    }

    if (data.success === false) {
      // Fallback to list-based count if stats endpoint fails
      const listRes = await fetch(`${AUTH_SERVICE_URL}/api/v2/auth/admin/users/list`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'x-internal-service-secret': INTERNAL_SERVICE_SECRET,
        },
        body: JSON.stringify({ limit: 1, offset: 0 }),
        signal: AbortSignal.timeout(5_000),
      })
      const listData = (await listRes.json()) as { total?: number; pagination?: { total?: number } }
      const totalUsers = listData.total ?? listData.pagination?.total ?? 0
      return NextResponse.json({
        totalUsers,
        totalOrganizations: 0,
        activeUsers: totalUsers,
        suspendedUsers: 0,
        totalQuotaUsage: { apiCalls: 0, users: totalUsers, storage: 0 },
      })
    }

    return NextResponse.json({
      totalUsers: data.totalUsers ?? 0,
      totalOrganizations: data.totalOrganizations ?? 0,
      activeUsers: data.activeUsers ?? data.totalUsers ?? 0,
      suspendedUsers: data.suspendedUsers ?? 0,
      totalQuotaUsage: { apiCalls: 0, users: data.totalUsers ?? 0, storage: 0 },
    })
  } catch (error: any) {
    return NextResponse.json({ error: error.message || 'Failed to fetch stats' }, { status: 500 })
  }
}
