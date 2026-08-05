import { NextRequest } from 'next/server'
import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'

// U2-12 (ui-ux-verevon-gap.md §10 + Option A consolidation):
//
// Translation proxy now hits the Model Plane v1 Rust gateway at
// `/v1/ai/translate` (Azure Translator REST via translate_routes.rs).
// Previously it hit ai-core (v2 Python); the consolidation moves all
// chat-feature surfaces onto the single v1 stack.
//
// Auth: gateway requires `Authorization: Bearer <token>` (the dev bypass
// accepts any non-empty token in local dev).
//
// Verevon request shape (unchanged from the v2 contract):
//   POST /api/ai/translate
//   { text: string, target_language: string, source_language?: string }
//
// Response (also unchanged — the Rust handler returns the same shape):
//   { request_id, translated_text, source_language_detected?, model_used }

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'
// U2-5: BEARER fallback is intentionally kept for the dev-bypass path only.
// When a user session cookie is present we mint a real RS256 JWT via
// `getModelPlaneTokenFromSession` and that takes precedence. The literal
// `dev-bypass` is only sent when the helper throws and the gateway is
// running with MODEL_GATEWAY_AUTH_DEV_BYPASS=1.

interface TranslateBody {
  text?: string
  target_language?: string
  source_language?: string
}

interface TranslateResponse {
  request_id?: string
  translated_text: string
  source_language_detected?: string
  model_used?: string
}

export async function POST(request: NextRequest): Promise<Response> {
  try {
    const body = (await request.json()) as TranslateBody
    const text = (body.text ?? '').trim()
    const target = (body.target_language ?? '').trim()

    if (!text) {
      return Response.json({ error: 'Missing required field: text' }, { status: 400 })
    }
    if (text.length > 10000) {
      return Response.json(
        { error: 'text too long (max 10000 characters)' },
        { status: 413 },
      )
    }
    if (!target) {
      return Response.json(
        { error: 'Missing required field: target_language (ISO-639-1 or BCP-47)' },
        { status: 400 },
      )
    }

    const token = await getModelPlaneTokenFromSession(request)
    const upstreamRes = await fetch(`${MODEL_GATEWAY_URL}/v1/ai/translate`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({
        text,
        target_language: target,
        source_language: body.source_language ?? '',
      }),
      signal: AbortSignal.timeout(30_000),
    })

    if (!upstreamRes.ok) {
      const detail = await upstreamRes.text().catch(() => '')
      return Response.json(
        { error: 'Translation failed', upstream_status: upstreamRes.status, detail },
        { status: upstreamRes.status },
      )
    }

    const data = (await upstreamRes.json()) as TranslateResponse
    return Response.json(data, { status: 200 })
  } catch (error: unknown) {
    return Response.json(
      {
        error: 'Translation service unavailable',
        message: error instanceof Error ? error.message : 'unknown',
      },
      { status: 502 },
    )
  }
}
