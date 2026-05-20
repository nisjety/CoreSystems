/**
 * Phase A · A3 — `/settings/audit-log` page.
 *
 * Lists the most recent audit events for the current org. Data comes
 * from audit-core via `/api/audit/log` (which scopes by `org_id`
 * resolved from the verified Better Auth session — never from a
 * client-supplied header).
 *
 * Same visual direction as `/settings/usage`: Intercom canvas + white
 * card with cormorant-garamond title + inter body.
 */

'use client'

import { useEffect, useState } from 'react'

interface AuditRow {
  id: number
  ingested_at: string
  occurred_at: string
  org_id: string
  user_id?: string
  actor_role?: string
  plane: string
  event: string
  subject?: string
  resource_id?: string
  outcome: string
  details?: Record<string, unknown>
  request_id?: string
}

interface AuditResponse {
  data?: AuditRow[]
  meta?: { count?: number; limit?: number }
  error?: string | null
}

function relativeTime(iso: string): string {
  const t = Date.parse(iso)
  if (!Number.isFinite(t)) return iso
  const diffSec = Math.floor((Date.now() - t) / 1000)
  if (diffSec < 60) return `${diffSec}s siden`
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)}m siden`
  if (diffSec < 86_400) return `${Math.floor(diffSec / 3600)}t siden`
  return `${Math.floor(diffSec / 86_400)}d siden`
}

function outcomeColor(outcome: string): string {
  switch (outcome) {
    case 'ok':
      return 'bg-emerald-50 text-emerald-700 border-emerald-200'
    case 'denied':
      return 'bg-amber-50 text-amber-700 border-amber-200'
    case 'error':
      return 'bg-red-50 text-red-700 border-red-200'
    default:
      return 'bg-[#F4EFE5] text-[#2B2B2B] border-[#D8D2C6]'
  }
}

export default function AuditLogPage(): JSX.Element {
  const [rows, setRows] = useState<AuditRow[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [filterEvent, setFilterEvent] = useState('')

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    const url = new URL('/api/audit/log', window.location.origin)
    url.searchParams.set('limit', '100')
    if (filterEvent) url.searchParams.set('event', filterEvent)
    fetch(url.toString(), { cache: 'no-store' })
      .then((res) => res.json() as Promise<AuditResponse>)
      .then((body) => {
        if (cancelled) return
        if (body.error) setError(body.error)
        setRows(body.data ?? [])
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load audit log')
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [filterEvent])

  return (
    <div className="bg-[#F4EFE5] min-h-full px-8 py-10">
      <div className="mx-auto max-w-5xl space-y-6">
        <header className="flex items-end justify-between gap-4">
          <div>
            <h1
              className="text-[28px] text-[#2B2B2B]"
              style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
            >
              Revisjonslogg
            </h1>
            <p className="mt-2 font-inter text-[13px] text-[#A09890]">
              Sikkerhets- og driftshendelser fra alle plan, siste 7 dager.
            </p>
          </div>
          <input
            type="text"
            placeholder="Filtrer på hendelse"
            value={filterEvent}
            onChange={(e) => setFilterEvent(e.target.value)}
            className="rounded-md border border-[#E9EBF2] bg-white px-3 py-2 font-inter text-[12px] text-[#2B2B2B] placeholder:text-[#A09890]"
          />
        </header>

        <section className="rounded-lg border border-[#E9EBF2] bg-white">
          {loading ? (
            <div className="px-5 py-8 text-center font-inter text-[12px] text-[#A09890]">
              Laster revisjonslogg …
            </div>
          ) : error ? (
            <div className="px-5 py-8 text-center font-inter text-[12px] text-red-600">
              {error}
            </div>
          ) : rows.length === 0 ? (
            <div className="px-5 py-8 text-center font-inter text-[12px] text-[#A09890]">
              Ingen hendelser i perioden.
            </div>
          ) : (
            <ul className="divide-y divide-[#F4EFE5]">
              {rows.map((row) => (
                <li key={row.id} className="flex items-start gap-4 px-5 py-3">
                  <span
                    className={`shrink-0 rounded-full border px-2 py-0.5 font-inter text-[10px] uppercase tracking-wide ${outcomeColor(row.outcome)}`}
                  >
                    {row.outcome}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-baseline gap-2">
                      <span className="font-inter text-[12px] font-medium text-[#2B2B2B]">
                        {row.event}
                      </span>
                      <span className="font-inter text-[11px] text-[#A09890]">
                        {row.plane}
                      </span>
                    </div>
                    {(row.subject || row.resource_id) && (
                      <div className="mt-0.5 font-inter text-[11px] text-[#A09890]">
                        {row.subject}
                        {row.subject && row.resource_id ? ' · ' : ''}
                        {row.resource_id}
                      </div>
                    )}
                    {row.user_id && (
                      <div className="mt-0.5 font-inter text-[11px] text-[#A09890]">
                        bruker {row.user_id}
                        {row.actor_role ? ` · ${row.actor_role}` : ''}
                      </div>
                    )}
                  </div>
                  <span className="shrink-0 font-inter text-[11px] text-[#A09890] tabular-nums">
                    {relativeTime(row.ingested_at)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  )
}
