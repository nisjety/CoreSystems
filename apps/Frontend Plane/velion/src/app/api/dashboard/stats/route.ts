import { NextRequest, NextResponse } from 'next/server'

import { getDashboardStatsRPC } from '@/lib/rpc/server'

export async function GET(request: NextRequest) {
  const cookieHeader = request.headers.get('cookie') ?? undefined
  const stats = await getDashboardStatsRPC(cookieHeader)

  return NextResponse.json(stats, {
    headers: { 'Cache-Control': 'public, s-maxage=60, stale-while-revalidate=120' },
  })
}
