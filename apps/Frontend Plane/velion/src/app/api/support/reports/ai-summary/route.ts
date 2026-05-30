import { NextRequest, NextResponse } from 'next/server'
import type { ReportStats } from '../route'
import { invokeReasoning } from '@/lib/model-plane/reasoning'

export async function POST(request: NextRequest) {
  let stats: ReportStats | null = null

  try {
    const body: unknown = await request.json()
    if (body && typeof body === 'object') {
      stats = body as ReportStats
    }
  } catch {
    return NextResponse.json({ error: 'Invalid request body' }, { status: 400 })
  }

  if (!stats) {
    return NextResponse.json({ error: 'Stats payload required' }, { status: 400 })
  }

  const statsContext = [
    `Open tickets: ${stats.open}`,
    `Pending tickets: ${stats.pending}`,
    `Solved/closed tickets: ${stats.solved}`,
    `Total tickets: ${stats.total}`,
    stats.avgResponseTimeHours !== null
      ? `Average first response time: ${stats.avgResponseTimeHours} hours`
      : 'Average first response time: not available',
    stats.csatScore !== null
      ? `CSAT score: ${stats.csatScore}`
      : 'CSAT score: not available',
    stats.topGroups.length > 0
      ? `Top groups by volume: ${stats.topGroups.map((g) => `${g.name} (${g.count})`).join(', ')}`
      : 'Top groups: no data',
    stats.ticketsByDay.length > 0
      ? `Tickets by day (last 7 days): ${stats.ticketsByDay.map((d) => `${d.date}: ${d.count}`).join(', ')}`
      : 'Daily ticket trend: no data',
  ].join('\n')

  try {
    const aiRes = await invokeReasoning(
      {
        query: statsContext,
        strategy: 'fast',
        context: {
          system_prompt:
            'You are a support analytics expert. Given these ticket statistics, provide 3-5 actionable insights for the support team in bullet point format.',
        },
      },
      {
        signal: AbortSignal.timeout(60_000),
        // U2-5: forward session cookie so the gateway gets a real JWT.
        cookieHeader: request.headers.get('cookie') ?? '',
      },
    )

    if (!aiRes.ok) {
      return NextResponse.json({ error: 'AI summary generation failed' }, { status: 502 })
    }

    const { answer } = (await aiRes.json()) as { answer: string }
    return NextResponse.json({ summary: answer })
  } catch (error) {
    const message = error instanceof Error ? error.message : 'AI summary request failed'
    return NextResponse.json({ error: message }, { status: 502 })
  }
}
