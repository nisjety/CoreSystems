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

  const publicArticles = articles.filter((a) => !a.internal)
  if (publicArticles.length === 0) {
    return Response.json({ summary: 'No public messages in this conversation.' })
  }

  const conversationText = publicArticles
    .map((a) => `[${a.from || 'unknown'}]: ${a.body || ''}`)
    .join('\n\n')

  const aiRes = await invokeReasoning(
    {
      query: conversationText,
      strategy: 'fast',
      context: {
        system_prompt:
          'You are a support analyst. Summarize this support conversation in 2-3 bullet points for agent handoff. Be concise and factual.',
      },
    },
    // U2-5: mint a real Model Plane JWT from the support agent's session.
    { cookieHeader: request.headers.get('cookie') ?? '' },
  )

  if (!aiRes.ok) {
    return Response.json({ error: 'AI summarization failed' }, { status: 502 })
  }

  const { answer } = await aiRes.json()
  return Response.json({ summary: answer })
}
