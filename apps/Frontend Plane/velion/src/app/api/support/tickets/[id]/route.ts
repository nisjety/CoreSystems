import { NextRequest } from 'next/server'
import { z } from 'zod'

import { ZAMMAD_URL, zammadConfigured, zammadHeaders, notConfiguredResponse } from '../../_lib/zammad'

const updateTicketSchema = z.object({
  state_id: z.number().optional(),
  priority_id: z.number().optional(),
  owner_id: z.number().optional(),
  group_id: z.number().optional(),
  tags: z.array(z.string()).optional(),
})

export async function GET(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!zammadConfigured()) return notConfiguredResponse()
  const { id } = await params

  const res = await fetch(`${ZAMMAD_URL}/api/v1/tickets/${id}?expand=true`, {
    headers: zammadHeaders(),
    cache: 'no-store',
  })

  const data = await res.json()
  return Response.json(data, { status: res.status })
}

export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!zammadConfigured()) return notConfiguredResponse()
  const { id } = await params

  const raw = await request.json().catch(() => null)
  if (!raw) {
    return Response.json({ error: 'Invalid JSON body' }, { status: 400 })
  }

  const parsed = updateTicketSchema.safeParse(raw)
  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 })
  }

  const res = await fetch(`${ZAMMAD_URL}/api/v1/tickets/${id}`, {
    method: 'PATCH',
    headers: zammadHeaders(),
    body: JSON.stringify(parsed.data),
  })

  const data = await res.json()
  return Response.json(data, { status: res.status })
}
