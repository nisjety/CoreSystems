import { NextRequest } from 'next/server'
import { z } from 'zod'

import { ZAMMAD_URL, zammadConfigured, zammadHeaders, notConfiguredResponse } from '../../../_lib/zammad'

const executeSchema = z.object({
  ticketId: z.number(),
})

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!zammadConfigured()) return notConfiguredResponse()
  const { id } = await params

  const raw = await request.json().catch(() => null)
  if (!raw) {
    return Response.json({ error: 'Invalid JSON body' }, { status: 400 })
  }

  const parsed = executeSchema.safeParse(raw)
  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 })
  }

  const { ticketId } = parsed.data

  const res = await fetch(`${ZAMMAD_URL}/api/v1/macros/${id}/execute`, {
    method: 'POST',
    headers: zammadHeaders(),
    body: JSON.stringify({ ticket_id: ticketId }),
  })

  const data = await res.json()
  return Response.json(data, { status: res.status })
}
