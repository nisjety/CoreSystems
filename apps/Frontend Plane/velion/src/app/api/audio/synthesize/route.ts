import { NextRequest } from 'next/server'
import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'

// U2-11 (ui-ux-velion-gap.md §10 + Option A consolidation):
//
// Text-to-speech proxy now hits the Model Plane v1 Rust gateway at
// `/v1/ai/speech` (Azure Speech via speech_routes::synthesize_azure).
// Previously this hit ai-core (v2 Python).
//
// Request shape (unchanged from prior version):
//   POST /api/audio/synthesize
//   { text: string, voice?: string, language?: string }
//
// The gateway returns JSON `{ audio_base64, format, ... }`. We decode
// base64 here and stream the raw bytes back to the browser so the chat
// UI can `<audio src=URL.createObjectURL(blob)>` it.

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'
// U2-5: JWT minted per-request via auth-core (see getModelPlaneTokenFromSession).

interface SynthesizeBody {
  text?: string
  voice?: string
  language?: string
}

interface GatewaySpeechResponse {
  request_id?: string
  provider?: string
  format?: string
  voice?: string
  audio_base64?: string
  bytes?: number
}

export async function POST(request: NextRequest): Promise<Response> {
  try {
    const body = (await request.json()) as SynthesizeBody
    const text = (body.text ?? '').trim()
    if (!text) {
      return Response.json({ error: 'Missing required field: text' }, { status: 400 })
    }
    if (text.length > 4096) {
      return Response.json(
        { error: 'text too long (max 4096 characters)' },
        { status: 413 },
      )
    }

    const token = await getModelPlaneTokenFromSession(request)
    const upstreamRes = await fetch(`${MODEL_GATEWAY_URL}/v1/ai/speech`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({
        input: text,
        // Default to the Norwegian neural voice; the gateway will pick
        // the same default when the field is omitted, so this is
        // documentation more than a real override.
        voice: body.voice ?? 'nb-NO-FinnNeural',
        provider: 'azure',
        format: 'mp3',
      }),
      signal: AbortSignal.timeout(30_000),
    })

    if (!upstreamRes.ok) {
      const detail = await upstreamRes.text().catch(() => '')
      return Response.json(
        { error: 'Synthesis failed', upstream_status: upstreamRes.status, detail },
        { status: upstreamRes.status },
      )
    }

    const data = (await upstreamRes.json()) as GatewaySpeechResponse
    if (!data.audio_base64) {
      return Response.json(
        { error: 'Gateway returned no audio bytes', detail: data },
        { status: 502 },
      )
    }

    const audioBytes = Buffer.from(data.audio_base64, 'base64')
    const mime =
      data.format === 'wav'
        ? 'audio/wav'
        : data.format === 'ogg'
          ? 'audio/ogg'
          : 'audio/mpeg'
    return new Response(audioBytes, {
      status: 200,
      headers: {
        'Content-Type': mime,
        'Cache-Control': 'no-store',
      },
    })
  } catch (error: unknown) {
    return Response.json(
      {
        error: 'Synthesis service unavailable',
        message: error instanceof Error ? error.message : 'unknown',
      },
      { status: 502 },
    )
  }
}
