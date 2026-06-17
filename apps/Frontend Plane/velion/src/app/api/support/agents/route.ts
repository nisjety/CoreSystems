import { NextRequest } from 'next/server'

import { ZAMMAD_URL, zammadConfigured, zammadHeaders, notConfiguredResponse } from '../_lib/zammad'

interface ZammadUser {
  id: number
  firstname: string
  lastname: string
  email: string
  [key: string]: unknown
}

export async function GET(_request: NextRequest) {
  if (!zammadConfigured()) return notConfiguredResponse()
  const res = await fetch(`${ZAMMAD_URL}/api/v1/users?role=Agent`, {
    headers: zammadHeaders(),
    cache: 'no-store',
  })

  if (!res.ok) {
    return Response.json({ error: 'Failed to fetch agents' }, { status: res.status })
  }

  const data: ZammadUser[] = await res.json()
  const agents = data.map(({ id, firstname, lastname, email }) => ({
    id,
    firstname,
    lastname,
    email,
  }))
  return Response.json(agents)
}
