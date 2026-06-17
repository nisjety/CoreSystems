import { NextRequest } from 'next/server'
import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'

// U2-15 closed (velion ui-ux-velion-gap.md §10): real image-generation
// proxy. Forwards to Model Plane v1 Rust gateway at `/v1/ai/images`
// (Azure OpenAI `gpt-image-1` via `image_routes::generate`).
//
// Velion request shape:
//   POST /api/ai/images
//   {
//     prompt: string,                    // required, ≤4000 chars
//     size?: "1024x1024" | "1024x1536" | "1536x1024" | "auto",
//     quality?: "low" | "medium" | "high" | "auto",
//     n?: 1..4
//   }
//
// Response shape:
//   {
//     request_id, model, size, mime,
//     image_base64,         // primary image
//     bytes,                // estimated raw byte size
//     data_url,             // ready-to-use data URL for chat composer
//     additional_images?,   // additional images when n > 1
//   }
//
// Image gen is slow (gpt-image-1 typically takes 15-45 s). We extend the
// fetch timeout accordingly and stream-buffer the response.

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'
// U2-5: the gateway JWT is minted per-request via auth-core. See
// `getModelPlaneTokenFromSession` below; the env-var fallback is only
// used when the helper throws and MODEL_PLANE_USE_DEV_BYPASS is opted in.

interface ImageRequestBody {
  prompt?: string
  size?: string
  quality?: string
  n?: number
}

interface GatewayImageResponse {
  request_id?: string
  model?: string
  size?: string
  mime?: string
  image_base64: string
  bytes?: number
  additional_images?: string[]
}

const ALLOWED_SIZES = new Set([
  '1024x1024',
  '1024x1536',
  '1536x1024',
  'auto',
])
const ALLOWED_QUALITIES = new Set(['low', 'medium', 'high', 'auto'])

export async function POST(request: NextRequest): Promise<Response> {
  try {
    const body = (await request.json()) as ImageRequestBody
    const prompt = (body.prompt ?? '').trim()
    if (!prompt) {
      return Response.json(
        { error: 'Missing required field: prompt' },
        { status: 400 },
      )
    }
    if (prompt.length > 4000) {
      return Response.json(
        { error: 'prompt too long (max 4000 characters)' },
        { status: 413 },
      )
    }

    const size = body.size ?? '1024x1024'
    if (!ALLOWED_SIZES.has(size)) {
      return Response.json(
        {
          error: `Invalid size; allowed: ${Array.from(ALLOWED_SIZES).join(', ')}`,
        },
        { status: 400 },
      )
    }
    const quality = body.quality ?? 'auto'
    if (!ALLOWED_QUALITIES.has(quality)) {
      return Response.json(
        {
          error: `Invalid quality; allowed: ${Array.from(ALLOWED_QUALITIES).join(', ')}`,
        },
        { status: 400 },
      )
    }
    const n = Math.max(1, Math.min(4, body.n ?? 1))

    const token = await getModelPlaneTokenFromSession(request)
    const upstreamRes = await fetch(`${MODEL_GATEWAY_URL}/v1/ai/images`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({ prompt, size, quality, n }),
      // gpt-image-1 routinely takes 20-45 seconds for a 1024x1024 image.
      signal: AbortSignal.timeout(120_000),
    })

    if (!upstreamRes.ok) {
      const detail = await upstreamRes.text().catch(() => '')
      return Response.json(
        {
          error: 'Image generation failed',
          upstream_status: upstreamRes.status,
          detail: detail.slice(0, 400),
        },
        { status: upstreamRes.status === 401 ? 502 : upstreamRes.status },
      )
    }

    const data = (await upstreamRes.json()) as GatewayImageResponse
    if (!data.image_base64) {
      return Response.json(
        { error: 'Gateway returned no image data', upstream: data },
        { status: 502 },
      )
    }

    const mime = data.mime ?? 'image/png'
    return Response.json(
      {
        ...data,
        // Convenience: a ready-to-use data URL the chat composer can
        // drop straight into a message preview.
        data_url: `data:${mime};base64,${data.image_base64}`,
        // We don't echo the giant raw base64 a second time — clients
        // that need it have `image_base64` already.
      },
      { status: 200 },
    )
  } catch (error: unknown) {
    return Response.json(
      {
        error: 'Image generation service unavailable',
        message: error instanceof Error ? error.message : 'unknown',
      },
      { status: 502 },
    )
  }
}
