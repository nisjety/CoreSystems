/**
 * Phase A · A3 — `/admin/system-health` page.
 *
 * Polls `/api/admin/health` (which fans out to every plane's /healthz
 * over `inter-plane-bus`) and renders the rolled-up status. Auto-
 * refreshes every 30 seconds; manual "Refresh now" button on the
 * header.
 *
 * Visual direction: same Intercom canvas + Chatbase simplicity used on
 * /settings/usage and /settings/audit-log. The status pill colours
 * match the audit-log outcome chips so operators learn one palette.
 */

'use client'

import { useCallback, useEffect, useMemo, useState } from 'react'

type ProbeStatus = 'healthy' | 'degraded' | 'down' | 'unknown'

interface PlaneProbe {
  plane: string
  service: string
  url: string
  status: ProbeStatus
  http_status?: number
  latency_ms?: number
  error?: string
}

interface HealthSummary {
  total: number
  healthy: number
  degraded: number
  down: number
  unknown: number
  checked_at: string
}

interface HealthResponse {
  data?: PlaneProbe[]
  meta?: HealthSummary
  error?: string | null
}

function statusPill(status: ProbeStatus): string {
  switch (status) {
    case 'healthy':
      return 'bg-emerald-50 text-emerald-700 border-emerald-200'
    case 'degraded':
      return 'bg-amber-50 text-amber-700 border-amber-200'
    case 'down':
      return 'bg-red-50 text-red-700 border-red-200'
    default:
      return 'bg-[#F4EFE5] text-[#2B2B2B] border-[#D8D2C6]'
  }
}

function statusDot(status: ProbeStatus): string {
  switch (status) {
    case 'healthy':
      return 'bg-emerald-500'
    case 'degraded':
      return 'bg-amber-500'
    case 'down':
      return 'bg-red-500'
    default:
      return 'bg-[#D8D2C6]'
  }
}

const PLANE_LABELS: Record<string, string> = {
  application: 'Application Plane',
  control: 'Control Plane',
  data: 'Data Plane v2',
  ingestion: 'Ingestion Plane',
}

export default function SystemHealthPage(): JSX.Element {
  const [probes, setProbes] = useState<PlaneProbe[]>([])
  const [summary, setSummary] = useState<HealthSummary | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const res = await fetch('/api/admin/health', { cache: 'no-store' })
      const body = (await res.json()) as HealthResponse
      if (body.error) setError(body.error)
      setProbes(body.data ?? [])
      setSummary(body.meta ?? null)
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Failed to load health')
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
    const id = setInterval(() => void load(), 30_000)
    return () => clearInterval(id)
  }, [load])

  const grouped = useMemo(() => {
    const m = new Map<string, PlaneProbe[]>()
    for (const p of probes) {
      const arr = m.get(p.plane) ?? []
      arr.push(p)
      m.set(p.plane, arr)
    }
    return m
  }, [probes])

  return (
    <div className="bg-[#F4EFE5] min-h-full px-8 py-10">
      <div className="mx-auto max-w-5xl space-y-6">
        <header className="flex items-end justify-between gap-4">
          <div>
            <h1
              className="text-[28px] text-[#2B2B2B]"
              style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
            >
              Systemhelse
            </h1>
            <p className="mt-2 font-inter text-[13px] text-[#A09890]">
              Tverrplan-helsesjekk over inter-plane-bus. Oppdateres hvert 30. sekund.
            </p>
          </div>
          <button
            type="button"
            onClick={() => void load()}
            disabled={loading}
            className="rounded-md bg-[#111111] px-4 py-2 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80 disabled:opacity-40"
          >
            {loading ? 'Sjekker …' : 'Oppdater nå'}
          </button>
        </header>

        {summary && (
          <section className="grid grid-cols-2 gap-4 sm:grid-cols-4">
            <StatCard label="Totalt" value={String(summary.total)} />
            <StatCard label="Friske" value={String(summary.healthy)} tint="emerald" />
            <StatCard label="Degraderte" value={String(summary.degraded)} tint="amber" />
            <StatCard label="Nede" value={String(summary.down + summary.unknown)} tint="red" />
          </section>
        )}

        {error && (
          <div className="rounded-md border border-red-200 bg-red-50 px-4 py-3 font-inter text-[12px] text-red-700">
            {error}
          </div>
        )}

        {Array.from(grouped.entries()).map(([plane, items]) => (
          <section key={plane} className="rounded-lg border border-[#E9EBF2] bg-white">
            <header className="flex items-center justify-between border-b border-[#E9EBF2] px-5 py-3">
              <h2 className="font-inter text-[13px] font-semibold text-[#2B2B2B]">
                {PLANE_LABELS[plane] ?? plane}
              </h2>
              <span className="font-inter text-[11px] text-[#A09890]">
                {items.filter((i) => i.status === 'healthy').length} / {items.length} friske
              </span>
            </header>
            <ul className="divide-y divide-[#F4EFE5]">
              {items.map((p) => (
                <li key={p.url} className="flex items-center gap-4 px-5 py-3">
                  <span className={`h-2 w-2 shrink-0 rounded-full ${statusDot(p.status)}`} />
                  <span className="min-w-0 flex-1">
                    <span className="font-inter text-[12px] font-medium text-[#2B2B2B]">
                      {p.service}
                    </span>
                    <span className="ml-2 font-inter text-[11px] text-[#A09890]">
                      {p.url.replace(/^https?:\/\//, '')}
                    </span>
                  </span>
                  {p.latency_ms !== undefined && (
                    <span className="font-inter text-[11px] tabular-nums text-[#A09890]">
                      {p.latency_ms} ms
                    </span>
                  )}
                  <span
                    className={`shrink-0 rounded-full border px-2 py-0.5 font-inter text-[10px] uppercase tracking-wide ${statusPill(p.status)}`}
                  >
                    {p.status}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        ))}

        {summary?.checked_at && (
          <p className="text-right font-inter text-[10px] text-[#A09890]">
            Sist sjekket: {new Date(summary.checked_at).toLocaleTimeString()}
          </p>
        )}
      </div>
    </div>
  )
}

function StatCard({
  label,
  value,
  tint = 'neutral',
}: {
  label: string
  value: string
  tint?: 'neutral' | 'emerald' | 'amber' | 'red'
}) {
  const tintClass = {
    neutral: 'text-[#2B2B2B]',
    emerald: 'text-emerald-700',
    amber: 'text-amber-700',
    red: 'text-red-700',
  }[tint]
  return (
    <div className="rounded-lg border border-[#E9EBF2] bg-white px-5 py-4">
      <div className="font-inter text-[11px] uppercase tracking-wide text-[#A09890]">
        {label}
      </div>
      <div className={`mt-2 font-inter text-[22px] font-semibold tabular-nums ${tintClass}`}>
        {value}
      </div>
    </div>
  )
}
