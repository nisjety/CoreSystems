import { NextRequest } from 'next/server'
import { invokeReasoning } from '@/lib/model-plane/reasoning'

import { ZAMMAD_URL, zammadConfigured, zammadHeaders, notConfiguredResponse } from '../../../_lib/zammad'

interface ZammadArticle {
  id: number
  internal: boolean
  sender?: string
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

  const articlesRes = await fetch(
    `${ZAMMAD_URL}/api/v1/ticket_articles/by_ticket/${id}`,
    { headers: zammadHeaders(), cache: 'no-store' },
  )

  if (!articlesRes.ok) {
    return Response.json(
      { error: 'Failed to fetch ticket articles' },
      { status: articlesRes.status },
    )
  }

  const articles: ZammadArticle[] = await articlesRes.json()

  const customerArticles = articles.filter(
    (a) => !a.internal && a.sender?.toLowerCase() === 'customer',
  )

  if (customerArticles.length === 0) {
    return Response.json({ sentiment: 'neutral', score: 50 })
  }

  const latestArticle = customerArticles[customerArticles.length - 1]
  const messageText = latestArticle.body || ''

  const aiRes = await invokeReasoning(
    {
      query: messageText,
      strategy: 'fast',
      context: {
        system_prompt:
          'Classify the sentiment of this customer support message. Return ONLY a JSON object: {"sentiment": "positive"|"neutral"|"negative"|"frustrated", "score": 0-100}',
      },
    },
    // U2-5: mint a real Model Plane JWT from the support agent's session.
    { cookieHeader: request.headers.get('cookie') ?? '' },
  )

  if (!aiRes.ok) {
    return Response.json({ error: 'AI sentiment analysis failed' }, { status: 502 })
  }

  const { answer } = await aiRes.json()

  let result: { sentiment: string; score: number }
  try {
    const cleaned = answer.trim().replace(/^```(?:json)?\s*/i, '').replace(/\s*```$/, '')
    result = JSON.parse(cleaned)
    if (!result.sentiment || typeof result.score !== 'number') {
      throw new Error('Invalid structure')
    }
  } catch {
    return Response.json({ error: 'Failed to parse AI sentiment response' }, { status: 502 })
  }

  return Response.json({ sentiment: result.sentiment, score: result.score })
}
