import { NextRequest } from 'next/server'
import { invokeReasoning } from '@/lib/model-plane/reasoning'

import { ZAMMAD_URL, zammadConfigured, zammadHeaders, notConfiguredResponse } from '../../../_lib/zammad'

interface ZammadArticle {
  id: number
  internal: boolean
  from?: string
  body?: string
  created_at?: string
}

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!zammadConfigured()) return notConfiguredResponse()
  const { id } = await params

  const [ticketRes, articlesRes] = await Promise.all([
    fetch(`${ZAMMAD_URL}/api/v1/tickets/${id}?expand=true`, {
      headers: zammadHeaders(),
      cache: 'no-store',
    }),
    fetch(`${ZAMMAD_URL}/api/v1/ticket_articles/by_ticket/${id}`, {
      headers: zammadHeaders(),
      cache: 'no-store',
    }),
  ])

  if (!ticketRes.ok) {
    return Response.json({ error: 'Failed to fetch ticket' }, { status: ticketRes.status })
  }
  if (!articlesRes.ok) {
    return Response.json({ error: 'Failed to fetch articles' }, { status: articlesRes.status })
  }

  const articles: ZammadArticle[] = await articlesRes.json()
  const recentPublic = articles.filter((a) => !a.internal).slice(-5)

  const conversationText = recentPublic
    .map((a) => `[${a.from || 'unknown'}]: ${a.body || ''}`)
    .join('\n\n')

  const aiRes = await invokeReasoning(
    {
      query: conversationText,
      strategy: 'fast',
      context: {
        system_prompt:
          'You are a customer support expert. Generate exactly 3 different reply options to the customer\'s latest message. Each option should be 1-2 sentences. Format as a JSON array of strings: ["option1","option2","option3"]. Return ONLY the JSON array.',
      },
    },
    // U2-5: mint a real Model Plane JWT from the support agent's session.
    { cookieHeader: request.headers.get('cookie') ?? '' },
  )

  if (!aiRes.ok) {
    return Response.json({ error: 'AI quick-replies generation failed' }, { status: 502 })
  }

  const { answer } = await aiRes.json()

  let options: string[]
  try {
    const cleaned = answer.trim().replace(/^```(?:json)?\s*/i, '').replace(/\s*```$/, '')
    options = JSON.parse(cleaned)
    if (!Array.isArray(options) || options.length === 0) {
      throw new Error('Not an array')
    }
  } catch {
    return Response.json({ error: 'Failed to parse AI response as JSON array' }, { status: 502 })
  }

  return Response.json({ options })
}
