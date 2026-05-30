import { NextRequest } from 'next/server'

import { ZAMMAD_URL, zammadConfigured, zammadHeaders, notConfiguredResponse } from '../_lib/zammad'

export async function GET(_request: NextRequest) {
  if (!zammadConfigured()) return notConfiguredResponse()
  const res = await fetch(`${ZAMMAD_URL}/api/v1/macros`, {
    headers: zammadHeaders(),
    cache: 'no-store',
  })

  const data = await res.json()
  return Response.json(data, { status: res.status })
}
