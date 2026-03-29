import { NextRequest } from 'next/server'

// Server-side only — Reasoning Plane's ai-core
const AI_CORE_URL = process.env.AI_CORE_URL ?? 'http://localhost:8100'

const SEARCH_SYSTEM_PROMPT = `Du er en intelligent søkeassistent for organisasjonskunnskap. \
Brukeren søker i sin organisasjons indekserte innhold — nettsider, dokumenter og integrasjoner. \
Svar presist og kortfattet på norsk. Strukturer svaret godt. \
Oppgi alltid relevante kildehenvisninger hvis du vet om dem.`

export async function POST(request: NextRequest) {
  let query = ''
  let limit = 8

  try {
    const body = await request.json()
    query = body.query ?? ''
    limit = body.limit ?? 8
  } catch {
    return new Response('Bad request', { status: 400 })
  }

  if (!query.trim()) {
    return new Response('Query required', { status: 400 })
  }

  const encoder = new TextEncoder()

  const stream = new ReadableStream({
    async start(controller) {
      const enqueue = (chunk: object) =>
        controller.enqueue(encoder.encode(`data: ${JSON.stringify(chunk)}\n\n`))

      try {
        const res = await fetch(`${AI_CORE_URL}/stream/chat`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            messages: [
              { role: 'system', content: SEARCH_SYSTEM_PROMPT },
              {
                role: 'user',
                content: `Søk: ${query}\n\nBegrens til de ${limit} mest relevante funnene.`,
              },
            ],
            model: 'gpt-4o-mini',
            temperature: 0.3,
          }),
          signal: AbortSignal.timeout(30_000),
        })

        if (!res.ok || !res.body) {
          throw new Error(`ai-core returned ${res.status}`)
        }

        const reader = res.body.getReader()
        const decoder = new TextDecoder()
        let buf = ''

        while (true) {
          const { done, value } = await reader.read()
          if (done) break
          buf += decoder.decode(value, { stream: true })

          const lines = buf.split('\n')
          buf = lines.pop() ?? ''

          for (const line of lines) {
            if (!line.startsWith('data: ')) continue
            const raw = line.slice(6).trim()
            if (!raw) continue

            try {
              const chunk = JSON.parse(raw) as { type: string; content?: string; error?: string }

              if (chunk.type === 'content' && chunk.content) {
                enqueue({ type: 'answer_chunk', content: chunk.content })
              } else if (chunk.type === 'error') {
                enqueue({ type: 'error', error: chunk.error ?? 'Unknown error' })
              }
              // skip 'metadata' and 'done' — we emit our own done below
            } catch {
              // skip malformed chunks
            }
          }
        }

        enqueue({ type: 'done' })
      } catch (err) {
        // ai-core unreachable — emit error so search-api.ts mock fallback activates
        if (err instanceof Error && err.name === 'AbortError') {
          enqueue({ type: 'error', error: 'Timeout' })
        } else {
          enqueue({ type: 'error', error: 'ai-core unavailable' })
        }
      } finally {
        controller.close()
      }
    },
  })

  return new Response(stream, {
    headers: {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      Connection: 'keep-alive',
    },
  })
}
