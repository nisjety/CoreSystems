import { NextRequest } from 'next/server'
import { z } from 'zod'

const ZAMMAD_URL = process.env.ZAMMAD_API_URL || 'http://zammad-railsserver:3000'
const ZAMMAD_TOKEN = process.env.ZAMMAD_API_TOKEN || ''

// U7-2 (ui-ux-velion-gap.md): when Zammad isn't deployed, fail gracefully
// instead of crashing the /helpdesk page with `getaddrinfo ENOTFOUND
// zammad-railsserver`. The /helpdesk feature is scaffolded but the Zammad
// service is not yet part of the local stack. Treat the absence of
// ZAMMAD_API_TOKEN as the canonical signal that the integration is not
// configured — both env files ship with the token empty.
function zammadConfigured(): boolean {
  return ZAMMAD_TOKEN.length > 0
}

function notConfigured(): Response {
  return Response.json(
    {
      error: 'support_not_configured',
      message:
        'The support / helpdesk integration (Zammad) is not configured in this environment. Set ZAMMAD_API_URL + ZAMMAD_API_TOKEN to enable.',
    },
    { status: 503 },
  )
}

function zammadHeaders() {
  return {
    Authorization: `Token token=${ZAMMAD_TOKEN}`,
    'Content-Type': 'application/json',
  }
}

const createTicketSchema = z.object({
  title: z.string().min(1),
  body: z.string().min(1),
  customer_email: z.string().email(),
  group: z.string().optional(),
  priority: z.string().optional(),
})

export async function GET(request: NextRequest) {
  if (!zammadConfigured()) return notConfigured()

  const { searchParams } = new URL(request.url)
  const page = searchParams.get('page') || '1'
  const state = searchParams.get('state')
  const group = searchParams.get('group')

  const qs = new URLSearchParams({ expand: 'true', per_page: '50', page })
  if (state) qs.set('state', state)
  if (group) qs.set('group', group)

  const res = await fetch(`${ZAMMAD_URL}/api/v1/tickets?${qs}`, {
    headers: zammadHeaders(),
    cache: 'no-store',
  })

  const data = await res.json()
  return Response.json(data, { status: res.status })
}

export async function POST(request: NextRequest) {
  if (!zammadConfigured()) return notConfigured()

  const raw = await request.json().catch(() => null)
  if (!raw) {
    return Response.json({ error: 'Invalid JSON body' }, { status: 400 })
  }

  const parsed = createTicketSchema.safeParse(raw)
  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 })
  }

  const { title, body, customer_email, group, priority } = parsed.data

  const payload = {
    title,
    group: group || 'Users',
    priority: priority || '2 normal',
    customer: customer_email,
    article: {
      subject: title,
      body,
      type: 'note',
      internal: false,
      content_type: 'text/html',
    },
  }

  const res = await fetch(`${ZAMMAD_URL}/api/v1/tickets`, {
    method: 'POST',
    headers: zammadHeaders(),
    body: JSON.stringify(payload),
  })

  const data = await res.json()
  return Response.json(data, { status: res.status })
}
