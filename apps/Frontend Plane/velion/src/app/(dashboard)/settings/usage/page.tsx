/**
 * Phase A · A3 — `/settings/usage` page.
 *
 * Shows per-(plane, op) rollups for the current org over the last 30
 * days. Data comes from audit-core via `/api/usage/summary`.
 *
 * Visual direction follows the Intercom canvas + Chatbase simplicity
 * combo agreed in `docs/phase-a-implementation-plan.md`:
 *
 *   - Cream canvas (`bg-[#F4EFE5]`) matches the dashboard veil.
 *   - Cards on `bg-white` with the standard `border-[#E9EBF2]` hairline.
 *   - Editorial serif (Cormorant Garamond, already a font variable) on
 *     the page title; everything else stays in Inter.
 *   - Single accent: charcoal `#111111` on the primary CTA.
 */

'use client'

import { useEffect, useMemo, useState } from 'react'

interface UsageSummaryRow {
  plane: string
  op: string
  events: number
  tokens_in: number
  tokens_out: number
  bytes_in: number
  bytes_out: number
  cost_cents: number
}

interface UsageSummaryResponse {
  data?: UsageSummaryRow[]
  meta?: { count?: number }
  error?: string | null
}

function formatCost(cents: number): string {
  if (!Number.isFinite(cents) || cents === 0) return '—'
  return `$${(cents / 100).toFixed(2)}`
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
  return n.toLocaleString()
}

function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n === 0) return '—'
  const units = ['B', 'KB', 'MB', 'GB']
  let value = n
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`
}

export default function UsagePage(): JSX.Element {
  const [rows, setRows] = useState<UsageSummaryRow[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    fetch('/api/usage/summary', { cache: 'no-store' })
      .then((res) => res.json() as Promise<UsageSummaryResponse>)
      .then((body) => {
        if (cancelled) return
        if (body.error) setError(body.error)
        setRows(body.data ?? [])
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load usage')
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const totals = useMemo(() => {
    return rows.reduce(
      (acc, r) => ({
        events: acc.events + r.events,
        tokens_in: acc.tokens_in + r.tokens_in,
        tokens_out: acc.tokens_out + r.tokens_out,
        cost_cents: acc.cost_cents + r.cost_cents,
      }),
      { events: 0, tokens_in: 0, tokens_out: 0, cost_cents: 0 },
    )
  }, [rows])

  return (
    <div className="bg-[#F4EFE5] min-h-full px-8 py-10">
      <div className="mx-auto max-w-5xl space-y-8">
        <header className="space-y-2">
          <h1
            className="text-[28px] text-[#2B2B2B]"
            style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
          >
            Bruk
          </h1>
          <p className="font-inter text-[13px] text-[#A09890]">
            Tokens, forespørsler og kostnader for organisasjonen din — siste 30 dager.
          </p>
        </header>

        <section className="grid grid-cols-1 gap-4 sm:grid-cols-4">
          <StatCard label="Hendelser" value={formatNumber(totals.events)} />
          <StatCard label="Tokens inn" value={formatNumber(totals.tokens_in)} />
          <StatCard label="Tokens ut" value={formatNumber(totals.tokens_out)} />
          <StatCard label="Kostnad" value={formatCost(totals.cost_cents)} />
        </section>

        <section className="rounded-lg border border-[#E9EBF2] bg-white">
          <header className="border-b border-[#E9EBF2] px-5 py-4">
            <h2 className="font-inter text-[13px] font-semibold text-[#2B2B2B]">
              Per tjeneste
            </h2>
          </header>
          {loading ? (
            <div className="px-5 py-8 text-center font-inter text-[12px] text-[#A09890]">
              Laster bruksdata …
            </div>
          ) : error ? (
            <div className="px-5 py-8 text-center font-inter text-[12px] text-red-600">
              {error}
            </div>
          ) : rows.length === 0 ? (
            <div className="px-5 py-8 text-center font-inter text-[12px] text-[#A09890]">
              Ingen registrert bruk i perioden ennå.
            </div>
          ) : (
            <table className="w-full font-inter text-[12px]">
              <thead className="text-left text-[11px] uppercase tracking-wide text-[#A09890]">
                <tr>
                  <th className="px-5 py-3">Plan</th>
                  <th className="px-5 py-3">Operasjon</th>
                  <th className="px-5 py-3 text-right">Hendelser</th>
                  <th className="px-5 py-3 text-right">Tokens</th>
                  <th className="px-5 py-3 text-right">Data</th>
                  <th className="px-5 py-3 text-right">Kostnad</th>
                </tr>
              </thead>
              <tbody className="text-[#2B2B2B]">
                {rows.map((r) => (
                  <tr key={`${r.plane}:${r.op}`} className="border-t border-[#F4EFE5]">
                    <td className="px-5 py-3">{r.plane}</td>
                    <td className="px-5 py-3 text-[#A09890]">{r.op}</td>
                    <td className="px-5 py-3 text-right tabular-nums">
                      {formatNumber(r.events)}
                    </td>
                    <td className="px-5 py-3 text-right tabular-nums">
                      {formatNumber(r.tokens_in + r.tokens_out)}
                    </td>
                    <td className="px-5 py-3 text-right tabular-nums">
                      {formatBytes(r.bytes_in + r.bytes_out)}
                    </td>
                    <td className="px-5 py-3 text-right tabular-nums">
                      {formatCost(r.cost_cents)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>
      </div>
    </div>
  )
}

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-[#E9EBF2] bg-white px-5 py-4">
      <div className="font-inter text-[11px] uppercase tracking-wide text-[#A09890]">
        {label}
      </div>
      <div className="mt-2 font-inter text-[22px] font-semibold text-[#2B2B2B] tabular-nums">
        {value}
      </div>
    </div>
  )
}
