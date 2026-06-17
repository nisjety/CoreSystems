import { NextRequest } from 'next/server'

import { ZAMMAD_URL, zammadConfigured, zammadHeaders, notConfiguredResponse } from '../_lib/zammad'

interface ZammadGroup {
  id: number
  name: string
  [key: string]: unknown
}

export async function GET(_request: NextRequest) {
  if (!zammadConfigured()) return notConfiguredResponse()
  const res = await fetch(`${ZAMMAD_URL}/api/v1/groups`, {
    headers: zammadHeaders(),
    cache: 'no-store',
  })

  if (!res.ok) {
    return Response.json({ error: 'Failed to fetch groups' }, { status: res.status })
  }

  const data: ZammadGroup[] = await res.json()
  const groups = data.map(({ id, name }) => ({ id, name }))
  return Response.json(groups)
}
