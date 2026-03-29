import { NextRequest } from 'next/server'

const QUARRY_URL = process.env.QUARRY_API_URL || 'http://localhost:8090'

export const dynamic = 'force-dynamic'

export async function GET(
    request: NextRequest,
    context: { params: Promise<{ id: string }> }
) {
    const { id } = await context.params

    const response = await fetch(`${QUARRY_URL}/v1/jobs/${id}/stream`, {
        headers: {
            'Accept': 'text/event-stream',
            'X-API-Key': process.env.QUARRY_API_KEY || 'dev-test-key-12345',
        }
    })

    if (!response.ok) {
        return new Response(response.body, { status: response.status })
    }

    return new Response(response.body, {
        headers: {
            'Content-Type': 'text/event-stream',
            'Cache-Control': 'no-cache, no-transform',
            'Connection': 'keep-alive',
        },
    })
}
