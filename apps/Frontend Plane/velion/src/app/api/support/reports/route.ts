import { NextResponse } from 'next/server'

import { ZAMMAD_URL, zammadConfigured, zammadHeaders, notConfiguredResponse } from '../_lib/zammad'

interface ZammadTicket {
  id: number
  state_id: number
  group_id: number
  group: string
  created_at: string
  close_at?: string
  first_response_at?: string
}

interface GroupStat {
  name: string
  count: number
}

interface DayCount {
  date: string
  count: number
}

export interface ReportStats {
  open: number
  pending: number
  solved: number
  total: number
  avgResponseTimeHours: number | null
  csatScore: number | null
  topGroups: GroupStat[]
  ticketsByDay: DayCount[]
}

async function fetchTicketsByState(state: string): Promise<ZammadTicket[]> {
  const qs = new URLSearchParams({ expand: 'true', per_page: '200', state })
  const res = await fetch(`${ZAMMAD_URL}/api/v1/tickets?${qs}`, {
    headers: zammadHeaders(),
    cache: 'no-store',
  })

  if (!res.ok) {
    return []
  }

  const data: unknown = await res.json()
  return Array.isArray(data) ? (data as ZammadTicket[]) : []
}

async function fetchAllTickets(): Promise<ZammadTicket[]> {
  const qs = new URLSearchParams({ expand: 'true', per_page: '200', page: '1' })
  const res = await fetch(`${ZAMMAD_URL}/api/v1/tickets?${qs}`, {
    headers: zammadHeaders(),
    cache: 'no-store',
  })

  if (!res.ok) {
    return []
  }

  const data: unknown = await res.json()
  return Array.isArray(data) ? (data as ZammadTicket[]) : []
}

function buildTicketsByDay(tickets: ZammadTicket[]): DayCount[] {
  const now = new Date()
  const days: DayCount[] = []

  for (let i = 6; i >= 0; i--) {
    const d = new Date(now)
    d.setDate(d.getDate() - i)
    days.push({ date: d.toISOString().slice(0, 10), count: 0 })
  }

  const dayMap = new Map(days.map((d) => [d.date, d]))

  for (const ticket of tickets) {
    const dateKey = ticket.created_at?.slice(0, 10)
    if (dateKey && dayMap.has(dateKey)) {
      const entry = dayMap.get(dateKey)
      if (entry) {
        entry.count += 1
      }
    }
  }

  return days
}

function buildTopGroups(tickets: ZammadTicket[]): GroupStat[] {
  const counts = new Map<string, number>()

  for (const ticket of tickets) {
    const group = ticket.group || 'Unknown'
    counts.set(group, (counts.get(group) ?? 0) + 1)
  }

  return Array.from(counts.entries())
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count)
    .slice(0, 3)
}

function computeAvgResponseTimeHours(tickets: ZammadTicket[]): number | null {
  const responseTimes: number[] = []

  for (const ticket of tickets) {
    if (ticket.created_at && ticket.first_response_at) {
      const created = new Date(ticket.created_at).getTime()
      const responded = new Date(ticket.first_response_at).getTime()
      const diffHours = (responded - created) / (1000 * 60 * 60)

      if (diffHours >= 0) {
        responseTimes.push(diffHours)
      }
    }
  }

  if (responseTimes.length === 0) {
    return null
  }

  const total = responseTimes.reduce((sum, h) => sum + h, 0)
  return Math.round((total / responseTimes.length) * 10) / 10
}

export async function GET() {
  if (!zammadConfigured()) return notConfiguredResponse()
  try {
    const [openTickets, closedTickets, allTickets] = await Promise.all([
      fetchTicketsByState('open'),
      fetchTicketsByState('closed'),
      fetchAllTickets(),
    ])

    const pendingTickets = await fetchTicketsByState('pending reminder')

    const stats: ReportStats = {
      open: openTickets.length,
      pending: pendingTickets.length,
      solved: closedTickets.length,
      total: allTickets.length,
      avgResponseTimeHours: computeAvgResponseTimeHours(allTickets),
      csatScore: null,
      topGroups: buildTopGroups(allTickets),
      ticketsByDay: buildTicketsByDay(allTickets),
    }

    return NextResponse.json(stats)
  } catch (error) {
    const message = error instanceof Error ? error.message : 'Failed to fetch report stats'
    return NextResponse.json({ error: message }, { status: 500 })
  }
}
