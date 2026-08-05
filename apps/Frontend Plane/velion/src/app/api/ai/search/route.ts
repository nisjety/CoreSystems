import { NextRequest } from 'next/server'

import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'

// W4-3 (ui-ux-verevon-gap.md §13): the search proxy used to POST to a
// non-existent `/stream/chat` endpoint on the model-gateway — every
// request returned `ai-core unavailable` and the client-side
// `search-api.ts` silently fell back to a mock. Users hit the `/search`
// page, got a confident-looking answer, but the answer was synthesised
// client-side from hardcoded fixtures.
//
// The fix rewires the route to the real model-gateway surface
// (`POST /v1/invoke`). Streaming is dropped — `/v1/invoke` is unary,
// and `/v1/invoke/stream` is SSE-shaped but a different envelope.
// The simpler unary path is fine here because the /search UI doesn't
// rely on token-by-token rendering; it shows a typed-in answer block
// once complete. The SSE wire (`data: …\n\n`) is preserved so the
// existing `search-api.ts` client keeps working with no changes.

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'

const SEARCH_SYSTEM_PROMPT = `Du er en intelligent søkeassistent for organisasjonskunnskap. \
Brukeren søker i sin organisasjons indekserte innhold — nettsider, dokumenter og integrasjoner. \
Svar presist og kortfattet på norsk. Strukturer svaret godt. \
Oppgi alltid relevante kildehenvisninger hvis du vet om dem.`

interface InvokeResponse {
  content?: string
  model_used?: string
  stop_reason?: string
}

export async function POST(request: NextRequest): Promise<Response> {
  let query = ''
  let limit = 8

  try {
    const body = await request.json()
    query = (body.query as string | undefined) ?? ''
    limit = (body.limit as number | undefined) ?? 8
  } catch {
    return new Response('Bad request', { status: 400 })
  }

  if (!query.trim()) {
    return new Response('Query required', { status: 400 })
  }

  const encoder = new TextEncoder()

  const stream = new ReadableStream({
    async start(controller) {
      const enqueue = (chunk: object): void =>
        controller.enqueue(encoder.encode(`data: ${JSON.stringify(chunk)}\n\n`))

      try {
        const token = await getModelPlaneTokenFromSession(request)

        // /v1/invoke accepts the same shape verevon's chat path uses, with
        // browse_web=true so the gateway runs a Brave search before calling
        // the LLM and grounds the answer in fresh results. `system_prompt`
        // overrides the gateway's default and keeps the Norwegian-only,
        // citation-preferring tone.
        const upstream = await fetch(`${MODEL_GATEWAY_URL}/v1/invoke`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${token}`,
          },
          body: JSON.stringify({
            content: `Søk: ${query}\n\nBegrens til de ${limit} mest relevante funnene.`,
            model: 'gpt-4o-mini',
            response_mode: 'auto',
            browse_web: true,
            system_prompt: SEARCH_SYSTEM_PROMPT,
          }),
          signal: AbortSignal.timeout(45_000),
        })

        if (!upstream.ok) {
          const detail = await upstream.text().catch(() => '')
          throw new Error(
            `model-gateway returned ${upstream.status}: ${detail.slice(0, 200)}`,
          )
        }

        const payload = (await upstream.json()) as InvokeResponse
        const answer = (payload.content ?? '').trim()
        if (!answer) {
          enqueue({ type: 'error', error: 'Empty answer from model-gateway' })
        } else {
          // Emit as a single answer_chunk so the existing client-side
          // streaming UI renders it as one block. If we want token-by-token
          // later, switch to `/v1/invoke/stream` and forward chunks.
          enqueue({ type: 'answer_chunk', content: answer })
        }

        enqueue({ type: 'done' })
      } catch (err: unknown) {
        if (err instanceof Error && err.name === 'AbortError') {
          enqueue({ type: 'error', error: 'Timeout' })
        } else {
          enqueue({
            type: 'error',
            error:
              err instanceof Error
                ? `model-gateway unavailable: ${err.message}`
                : 'model-gateway unavailable',
          })
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
