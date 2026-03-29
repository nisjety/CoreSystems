import { NextRequest, NextResponse } from 'next/server'

const QUARRY_URL = process.env.QUARRY_API_URL || 'http://localhost:8090'

export async function POST(request: NextRequest) {
  try {
    const body = await request.json()
    const response = await fetch(`${QUARRY_URL}/v1/crawl`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Accept': 'application/json',
        'X-API-Key': process.env.QUARRY_API_KEY || 'dev-test-key-12345',
      },
      body: JSON.stringify({ ...body, mode: 'scheduled', maxDepth: 2 }),
      signal: AbortSignal.timeout(60000), // Increased from 10s to 60s
    })

    if (!response.ok) {
      return NextResponse.json(
        { error: `Quarry returned ${response.status}` },
        { status: response.status }
      )
    }

    const data = await response.json()
    return NextResponse.json({
      ...data,
      jobId: data.job?.id || data.id || null
    })
  } catch (err) {
    console.error('Next.js Crawl API Route Error:', err)
    return NextResponse.json({ error: 'Quarry not available or timed out' }, { status: 503 })
  }
}

// GET is not supported by the Quarry backend for the /v1/jobs list.
// Removed broken implementation to prevent 404/500 errors.

