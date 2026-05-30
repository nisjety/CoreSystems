import { NextRequest, NextResponse } from 'next/server'

// G32: target Quarry-v2 control service. Status now lives at GET /v1/jobs/{id}
// (envelope-wrapped) rather than the v1 GET /v1/crawl/{id}.
const QUARRY_URL = process.env.QUARRY_API_URL || 'http://quarry-control:8081'

export const runtime = 'nodejs'
export const dynamic = 'force-dynamic'

type QuarryJob = {
  id: string
  kind: string
  status: string
  params?: Record<string, unknown>
  created_at?: number
}

type QuarryEnvelope<T> = {
  data?: T | null
  meta?: Record<string, unknown> | null
  error?: { code?: string; message?: string } | null
}

// Quarry-v2 IDs are ULID-prefixed, e.g. `job_01KRA03SHZ2M10HH6B5Q6BGCHW`.
const JOB_ID_PATTERN = /^job_[0-9A-HJ-NP-TV-Z]{26}$/

export async function GET(
  _request: NextRequest,
  { params }: { params: Promise<{ jobId: string }> },
) {
  const { jobId } = await params

  if (!jobId || typeof jobId !== 'string' || !JOB_ID_PATTERN.test(jobId)) {
    return NextResponse.json({ error: 'Invalid jobId' }, { status: 400 })
  }

  let upstream: Response
  try {
    upstream = await fetch(`${QUARRY_URL}/v1/jobs/${jobId}`, {
      headers: { Accept: 'application/json' },
    })
  } catch {
    return NextResponse.json({ error: 'Upstream unreachable' }, { status: 502 })
  }

  const envelope = (await upstream.json().catch(() => null)) as QuarryEnvelope<QuarryJob> | null

  if (!upstream.ok || !envelope?.data) {
    return NextResponse.json(
      { error: envelope?.error?.message ?? 'Upstream error' },
      { status: upstream.status || 502 },
    )
  }

  const job = envelope.data

  // Pages-fetched is not on the job record in v2; if needed, callers should
  // poll GET /v1/jobs/{id}/events. We return 0 here to keep the shape stable
  // and let the SSE channel be the source of truth for progress.
  return NextResponse.json({
    status: job.status ?? 'unknown',
    pages: 0,
    raw: job,
  })
}
