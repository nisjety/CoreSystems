import { NextRequest } from 'next/server'
import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'

// U2-10 (ui-ux-velion-gap.md §10 + Option A consolidation):
//
// Voice-input transcription proxy now hits the Model Plane v1 Rust
// gateway at `/v1/ai/transcribe` (Azure Speech via
// speech_routes::transcribe_azure). Previously this hit ai-core (v2
// Python).
//
// Velion's chat input still posts FormData with an `audio` blob — we
// convert it to base64 + JSON here to match the gateway's contract.

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'
// U2-5: JWT minted per-request via auth-core (see getModelPlaneTokenFromSession).

interface TranscribeResponse {
  request_id?: string
  provider?: string
  transcript?: string
  duration_ms?: number
  // Compat field for legacy callers that expected `text`.
  text?: string
}

export async function POST(request: NextRequest): Promise<Response> {
  try {
    const formData = await request.formData()
    const audioField = formData.get('audio')

    if (!(audioField instanceof Blob)) {
      return Response.json(
        { error: 'Missing audio field (expected multipart/form-data with key "audio")' },
        { status: 400 },
      )
    }

    const arrayBuffer = await audioField.arrayBuffer()
    const audioBase64 = Buffer.from(arrayBuffer).toString('base64')
    const mime = audioField.type || 'audio/webm'
    const language = request.nextUrl.searchParams.get('lang') ?? 'en-US'

    const token = await getModelPlaneTokenFromSession(request)
    const upstreamRes = await fetch(`${MODEL_GATEWAY_URL}/v1/ai/transcribe`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({
        audio_base64: audioBase64,
        mime,
        language,
        provider: 'azure',
      }),
      signal: AbortSignal.timeout(60_000),
    })

    if (!upstreamRes.ok) {
      const detail = await upstreamRes.text().catch(() => '')
      return Response.json(
        { error: 'Transcription failed', upstream_status: upstreamRes.status, detail },
        { status: upstreamRes.status === 401 ? 502 : upstreamRes.status },
      )
    }

    const data = (await upstreamRes.json()) as TranscribeResponse
    return Response.json(
      {
        ...data,
        // Legacy alias: the chat input reads `data.text`.
        text: data.transcript ?? data.text ?? '',
      },
      { status: 200 },
    )
  } catch (error: unknown) {
    return Response.json(
      {
        error: 'Transcription service unavailable',
        message: error instanceof Error ? error.message : 'unknown',
      },
      { status: 502 },
    )
  }
}
