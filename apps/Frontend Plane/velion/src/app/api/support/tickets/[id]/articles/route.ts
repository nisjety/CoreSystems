import { NextRequest } from 'next/server'
import { z } from 'zod'

import { ZAMMAD_URL, zammadConfigured, zammadHeaders, notConfiguredResponse } from '../../../_lib/zammad'

const addArticleSchema = z.object({
  body: z.string().min(1),
  internal: z.boolean(),
  type: z.string().optional(),
})

export async function GET(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!zammadConfigured()) return notConfiguredResponse()
  const { id } = await params

  const res = await fetch(`${ZAMMAD_URL}/api/v1/ticket_articles/by_ticket/${id}`, {
    headers: zammadHeaders(),
    cache: 'no-store',
  })

  const data = await res.json()
  return Response.json(data, { status: res.status })
}

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

  const parsed = addArticleSchema.safeParse(raw)
  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 })
  }

  const { body, internal, type } = parsed.data

  const payload = {
    ticket_id: Number(id),
    body,
    internal,
    type: type || 'note',
    content_type: 'text/html',
  }

  const res = await fetch(`${ZAMMAD_URL}/api/v1/ticket_articles`, {
    method: 'POST',
    headers: zammadHeaders(),
    body: JSON.stringify(payload),
  })

  const data = await res.json()
  return Response.json(data, { status: res.status })
}
