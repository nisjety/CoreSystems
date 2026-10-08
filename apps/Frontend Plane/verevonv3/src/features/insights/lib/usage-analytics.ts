import type { UsageAnalytics, UsageBucket } from '@/shared/api/cost-client'

export type UsageGroup = 'model' | 'day' | 'basis'
export type UsageFilter = 'all' | UsageBucket['basis']
export type UsageTotal = { label: string; requests: number; input: number; output: number; apiCost: number; metered: number; subscription: number; unknown: number; unmeasured: number }
const empty = (label = ''): UsageTotal => ({ label, requests: 0, input: 0, output: 0, apiCost: 0, metered: 0, subscription: 0, unknown: 0, unmeasured: 0 })

export function usageRows(buckets: UsageBucket[], group: UsageGroup, filter: UsageFilter = 'all'): UsageTotal[] {
  const rows = new Map<string, UsageTotal>()
  for (const b of buckets) {
    if (filter !== 'all' && filter !== b.basis) continue
    const label = b[group]
    const row = rows.get(label) ?? empty(label)
    row.requests += b.requests; row.input += b.input_tokens; row.output += b.output_tokens
    row.apiCost += b.metered_cost_usd; row[b.basis] += b.requests; row.unmeasured += b.unmeasured_requests
    rows.set(label, row)
  }
  return [...rows.values()].sort((a, b) => group === 'day' ? a.label.localeCompare(b.label) : b.requests - a.requests || a.label.localeCompare(b.label))
}

export function usageTotals(buckets: UsageBucket[], filter: UsageFilter = 'all'): UsageTotal {
  return usageRows(buckets, 'basis', filter).reduce((total, row) => ({
    label: '', requests: total.requests + row.requests, input: total.input + row.input, output: total.output + row.output,
    apiCost: total.apiCost + row.apiCost, metered: total.metered + row.metered, subscription: total.subscription + row.subscription,
    unknown: total.unknown + row.unknown, unmeasured: total.unmeasured + row.unmeasured,
  }), empty())
}

export function usageMoney(amount: number): string {
  return amount > 0 && amount < 0.01 ? '< $0.01' : `$${amount.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
}

// Quoted fields alone don't prevent spreadsheet formula execution.
function csvCell(value: string | number | null): string {
  let text = String(value ?? '')
  if (typeof value === 'string' && /^[\s]*[=+\-@]/.test(text)) text = `'${text}`
  return `"${text.replaceAll('"', '""')}"`
}
export function usageCsv(report: UsageAnalytics, filter: UsageFilter = 'all'): string {
  const rows: Array<Array<string | number | null>> = [
    ['Scope', 'Period start', 'Period end', 'Day (UTC)', 'Model', 'Provider', 'Billing type', 'Requests', 'Input tokens', 'Output tokens', 'API estimate (USD)', 'Requests without token counts'],
    ...report.buckets.filter(b => filter === 'all' || b.basis === filter).map(b => [
      report.scope, report.from, report.to, b.day, b.model, b.provider, b.basis, b.requests, b.input_tokens, b.output_tokens,
      b.basis === 'metered' ? b.metered_cost_usd : null, b.unmeasured_requests,
    ]),
  ]
  return '\uFEFF' + rows.map(row => row.map(csvCell).join(',')).join('\r\n')
}
