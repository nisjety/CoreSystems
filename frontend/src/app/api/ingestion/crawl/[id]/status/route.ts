import { NextRequest, NextResponse } from 'next/server'

const QUARRY_URL = process.env.QUARRY_API_URL || 'http://localhost:9090'

export async function GET(
  _request: NextRequest,
  context: { params: Promise<{ id: string }> }
) {
  const { id } = await context.params

  try {
    const response = await fetch(`${QUARRY_URL}/v1/jobs/${id}`, {
      headers: {
        'Accept': 'application/json',
        'X-API-Key': process.env.QUARRY_API_KEY || 'dev-test-key-12345',
      },
      next: { revalidate: 0 },
      signal: AbortSignal.timeout(5000),
    })

    if (!response.ok) {
      return NextResponse.json({ error: 'Job not found' }, { status: response.status })
    }

    const data = await response.json()

    // Map Quarry's job status to the frontend phase model
    const phaseRaw = data.result?.phase || data.job?.status || 'idle'

    let mappedPhase = 'discovering'
    if (phaseRaw === 'completed' || phaseRaw === 'ready') mappedPhase = 'done'
    else if (phaseRaw === 'failed') mappedPhase = 'error'
    else if (phaseRaw === 'extracting' || phaseRaw === 'building' || phaseRaw === 'mapping') mappedPhase = phaseRaw

    return NextResponse.json({
      phase: mappedPhase,
      pagesFound: data.result?.pagesFound || data.result?.pageCount || 0,
      quarryJob: data.job
    })
  } catch {
    return NextResponse.json({ error: 'Quarry not available' }, { status: 503 })
  }
}
