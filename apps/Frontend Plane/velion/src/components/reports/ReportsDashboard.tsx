'use client'

import React, { useCallback, useEffect, useState } from 'react'
import { ArrowDown, ArrowUp, Minus, Sparkles, Users } from 'lucide-react'

// ── Types ─────────────────────────────────────────────────────────────────────

interface GroupStat {
  name: string
  count: number
}

interface DayCount {
  date: string
  count: number
}

interface ReportStats {
  open: number
  pending: number
  solved: number
  total: number
  avgResponseTimeHours: number | null
  csatScore: number | null
  topGroups: GroupStat[]
  ticketsByDay: DayCount[]
}

interface MetricItem {
  label: string
  value: string | number
  unit?: string
  trend?: {
    value: number
    direction: 'up' | 'down' | 'stable'
  }
}

// ── Sub-components ────────────────────────────────────────────────────────────

function ReportMetricCard({ metric }: { metric: MetricItem }) {
  const trend = metric.trend
  const isUp = trend?.direction === 'up'
  const isDown = trend?.direction === 'down'

  const trendColor = isUp ? 'text-green-600' : isDown ? 'text-red-600' : 'text-gray-600'
  const trendBgColor = isUp ? 'bg-green-50' : isDown ? 'bg-red-50' : 'bg-gray-50'
  const TrendIcon = isUp ? ArrowUp : isDown ? ArrowDown : Minus

  return (
    <div className="rounded-[22px] border border-[#E6E8EF] bg-white p-6 hover:shadow-md transition-shadow">
      <div className="flex items-start justify-between">
        <div>
          <p className="text-sm font-medium text-[#707480] mb-2">{metric.label}</p>
          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-bold text-[#2F3138]">{metric.value}</span>
            {metric.unit && (
              <span className="text-lg text-[#707480]">{metric.unit}</span>
            )}
          </div>
        </div>
        {trend && (
          <div className={`flex items-center gap-1 rounded-lg px-2 py-1 ${trendBgColor}`}>
            <TrendIcon className={`w-4 h-4 ${trendColor}`} />
            <span className={`text-xs font-semibold ${trendColor}`}>{trend.value}%</span>
          </div>
        )}
      </div>
    </div>
  )
}

interface BarChartProps {
  data: DayCount[]
}

function TicketBarChart({ data }: BarChartProps) {
  const maxCount = Math.max(...data.map((d) => d.count), 1)

  return (
    <div className="rounded-[22px] border border-[#E6E8EF] bg-white p-6">
      <h3 className="text-sm font-semibold text-[#2F3138] mb-4">Tickets by day (last 7 days)</h3>
      <div className="flex items-end gap-2 h-32">
        {data.map((day) => {
          const heightPct = Math.round((day.count / maxCount) * 100)
          const label = day.date.slice(5)

          return (
            <div key={day.date} className="flex flex-col items-center flex-1 gap-1">
              <span className="text-[10px] text-[#707480] font-medium">{day.count}</span>
              <div className="w-full flex flex-col justify-end" style={{ height: '88px' }}>
                <div
                  className="w-full rounded-t-md bg-[#DD7A1F] transition-all duration-300"
                  style={{ height: `${heightPct}%`, minHeight: day.count > 0 ? '4px' : '0' }}
                  title={`${day.date}: ${day.count} tickets`}
                />
              </div>
              <span className="text-[10px] text-[#707480]">{label}</span>
            </div>
          )
        })}
      </div>
    </div>
  )
}

interface TopGroupsTableProps {
  groups: GroupStat[]
}

function TopGroupsTable({ groups }: TopGroupsTableProps) {
  const maxCount = Math.max(...groups.map((g) => g.count), 1)

  return (
    <div className="rounded-[22px] border border-[#E6E8EF] bg-white p-6">
      <div className="flex items-center gap-2 mb-4">
        <Users className="w-4 h-4 text-[#707480]" />
        <h3 className="text-sm font-semibold text-[#2F3138]">Top groups by volume</h3>
      </div>
      {groups.length === 0 ? (
        <p className="text-sm text-[#707480]">No group data available.</p>
      ) : (
        <div className="space-y-3">
          {groups.map((group, index) => {
            const widthPct = Math.round((group.count / maxCount) * 100)
            const rank = index + 1

            return (
              <div key={group.name} className="flex items-center gap-3">
                <span className="text-xs font-semibold text-[#707480] w-4 shrink-0">
                  {rank}
                </span>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center justify-between mb-1">
                    <span className="text-sm font-medium text-[#2F3138] truncate">
                      {group.name}
                    </span>
                    <span className="text-sm font-semibold text-[#2F3138] ml-2 shrink-0">
                      {group.count}
                    </span>
                  </div>
                  <div className="h-1.5 rounded-full bg-[#F0F1F5] overflow-hidden">
                    <div
                      className="h-full rounded-full bg-[#DD7A1F] transition-all duration-500"
                      style={{ width: `${widthPct}%` }}
                    />
                  </div>
                </div>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

interface AiInsightsSectionProps {
  stats: ReportStats | null
}

function AiInsightsSection({ stats }: AiInsightsSectionProps) {
  const [summary, setSummary] = useState<string | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleGenerate = useCallback(async () => {
    if (!stats) return

    setIsLoading(true)
    setError(null)
    setSummary(null)

    try {
      const res = await fetch('/api/support/reports/ai-summary', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(stats),
      })

      if (!res.ok) {
        const body = (await res.json()) as { error?: string }
        throw new Error(body.error ?? `Request failed with status ${res.status}`)
      }

      const data = (await res.json()) as { summary: string }
      setSummary(data.summary)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to generate summary')
    } finally {
      setIsLoading(false)
    }
  }, [stats])

  return (
    <div className="rounded-[22px] border border-[#E6E8EF] bg-white p-6">
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <Sparkles className="w-4 h-4 text-[#DD7A1F]" />
          <h3 className="text-sm font-semibold text-[#2F3138]">AI Insights</h3>
        </div>
        <button
          type="button"
          onClick={handleGenerate}
          disabled={isLoading || !stats}
          className="inline-flex items-center gap-2 rounded-xl border border-[#E6E8EF] bg-[#FAFBFC] px-4 py-2 text-sm font-medium text-[#2F3138] transition-colors hover:bg-[#F0F1F5] disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Sparkles className="w-3.5 h-3.5 text-[#DD7A1F]" />
          {isLoading ? 'Generating...' : 'Generate weekly summary'}
        </button>
      </div>

      {error && (
        <div className="rounded-xl border border-red-100 bg-red-50 px-4 py-3 text-sm text-red-700">
          {error}
        </div>
      )}

      {summary ? (
        <div className="rounded-xl border border-[#F2E8D8] bg-[#FFFBF5] px-4 py-4">
          <p className="text-sm leading-relaxed text-[#2F3138] whitespace-pre-wrap">{summary}</p>
        </div>
      ) : !error && (
        <p className="text-sm text-[#707480]">
          Click "Generate weekly summary" to get AI-powered insights from your current ticket data.
        </p>
      )}
    </div>
  )
}

// ── Main component ────────────────────────────────────────────────────────────

function buildMetrics(stats: ReportStats): MetricItem[] {
  return [
    {
      label: 'Open tickets',
      value: stats.open,
    },
    {
      label: 'Solved today',
      value: stats.solved,
    },
    {
      label: 'Total this week',
      value: stats.total,
    },
    {
      label: 'CSAT score',
      value: stats.csatScore !== null ? `${stats.csatScore}%` : '—',
      unit: stats.csatScore !== null ? undefined : undefined,
    },
  ]
}

export function ReportsDashboard() {
  const [stats, setStats] = useState<ReportStats | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    async function loadStats() {
      setIsLoading(true)
      setError(null)

      try {
        const res = await fetch('/api/support/reports')

        if (!res.ok) {
          const body = (await res.json()) as { error?: string }
          throw new Error(body.error ?? `Request failed with status ${res.status}`)
        }

        const data = (await res.json()) as ReportStats

        if (!cancelled) {
          setStats(data)
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load report data')
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false)
        }
      }
    }

    loadStats()
    return () => {
      cancelled = true
    }
  }, [])

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64 text-[#707480] text-sm">
        Loading reports...
      </div>
    )
  }

  if (error) {
    return (
      <div className="rounded-[22px] border border-red-100 bg-red-50 p-6 text-sm text-red-700">
        Failed to load reports: {error}
      </div>
    )
  }

  if (!stats) {
    return null
  }

  const metrics = buildMetrics(stats)

  return (
    <div className="space-y-6 p-6">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold text-[#2F3138]">Support Reports</h1>
        <p className="text-sm text-[#707480] mt-1">
          Aggregated ticket metrics and team performance overview.
        </p>
      </div>

      {/* Metric cards */}
      <div className="grid grid-cols-2 gap-4 xl:grid-cols-4">
        {metrics.map((metric) => (
          <ReportMetricCard key={metric.label} metric={metric} />
        ))}
      </div>

      {/* Chart + Groups side by side on wider screens */}
      <div className="grid gap-4 xl:grid-cols-2">
        <TicketBarChart data={stats.ticketsByDay} />
        <TopGroupsTable groups={stats.topGroups} />
      </div>

      {/* Avg response time info strip */}
      {stats.avgResponseTimeHours !== null && (
        <div className="rounded-[22px] border border-[#E6E8EF] bg-white px-6 py-4 flex items-center gap-3">
          <span className="text-sm text-[#707480]">Avg. first response time</span>
          <span className="text-base font-semibold text-[#2F3138]">
            {stats.avgResponseTimeHours}h
          </span>
        </div>
      )}

      {/* AI Insights */}
      <AiInsightsSection stats={stats} />
    </div>
  )
}
