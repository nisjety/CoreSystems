import { For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
import { ArrowLeft, Coins, Receipt, Activity, Tags } from '@/shared/icons'
import {
  getCostSummary,
  listCostEntries,
  getCostPricing,
} from '@/shared/api/cost-client'

// Org-wide cost & usage dashboard (Phase 7 B5). Renders real ledger spend from
// cost-core via the gateway `/api/v1/cost/*` proxy: a rolled-up summary, the
// most recent priced inferences, and the model rate card. Honest-empty: a fresh
// org shows zeros and an empty table — never a fabricated figure.

function fmtUsd(value: number): string {
  if (value === 0) return '$0.00'
  if (value < 0.01) return `$${value.toFixed(6)}`
  return `$${value.toFixed(4)}`
}

function fmtInt(value: number): string {
  return new Intl.NumberFormat(undefined).format(value)
}

function fmtClock(at: string): string {
  if (!at) return ''
  const d = new Date(at)
  return Number.isNaN(d.getTime())
    ? ''
    : d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

export default function CostDashboardPage() {
  const [summary] = createResource(getCostSummary)
  const [entries] = createResource(() => listCostEntries(50))
  const [pricing] = createResource(getCostPricing)

  return (
    <div class="verevon-cost">
      <style>{COST_CSS}</style>
      <header class="verevon-cost__topbar">
        <a href="/agents" link class="verevon-cost__back" aria-label="Back to agents">
          <ArrowLeft size={16} />
        </a>
        <div>
          <p class="verevon-cost__eyebrow">
            <Coins size={13} strokeWidth={2.1} /> Cost &amp; usage
          </p>
          <h1 class="verevon-cost__title">Cost dashboard</h1>
          <p class="verevon-cost__sub">
            Real per-run spend from the model ledger. Every figure is priced from the
            catalogue cost-core records each inference against.
          </p>
        </div>
      </header>

      <section class="verevon-cost__stats" aria-label="Spend summary">
        <Stat
          icon={<Coins size={15} strokeWidth={2.1} />}
          label="Total spend"
          value={fmtUsd(summary()?.totalCostUsd ?? 0)}
          loading={summary.loading}
        />
        <Stat
          icon={<Receipt size={15} strokeWidth={2.1} />}
          label="Billed inferences"
          value={fmtInt(summary()?.entryCount ?? 0)}
          loading={summary.loading}
        />
        <Stat
          icon={<Activity size={15} strokeWidth={2.1} />}
          label="Input tokens"
          value={fmtInt(summary()?.totalInputTokens ?? 0)}
          loading={summary.loading}
        />
        <Stat
          icon={<Activity size={15} strokeWidth={2.1} />}
          label="Output tokens"
          value={fmtInt(summary()?.totalOutputTokens ?? 0)}
          loading={summary.loading}
        />
      </section>

      <section class="verevon-cost__panel" aria-label="Recent inferences">
        <div class="verevon-cost__panel-head">
          <span class="verevon-cost__panel-title"><Receipt size={14} strokeWidth={2.1} /> Recent inferences</span>
        </div>
        <Show
          when={(entries() ?? []).length > 0}
          fallback={
            <p class="verevon-cost__empty">
              <Show when={!entries.loading} fallback="Loading…">
                No billed inferences yet — run a task in chat or the Agent Run Console and spend appears here.
              </Show>
            </p>
          }
        >
          <div class="verevon-cost__table-wrap">
            <table class="verevon-cost__table">
              <thead>
                <tr>
                  <th>When</th>
                  <th>Model</th>
                  <th class="verevon-cost__num">Input</th>
                  <th class="verevon-cost__num">Output</th>
                  <th class="verevon-cost__num">Cost</th>
                </tr>
              </thead>
              <tbody>
                <For each={entries() ?? []}>
                  {(row) => (
                    <tr>
                      <td>{fmtClock(row.createdAt)}</td>
                      <td class="verevon-cost__model">{row.model || '—'}</td>
                      <td class="verevon-cost__num">{fmtInt(row.inputTokens)}</td>
                      <td class="verevon-cost__num">{fmtInt(row.outputTokens)}</td>
                      <td class="verevon-cost__num verevon-cost__cost">{fmtUsd(row.costUsd)}</td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </Show>
      </section>

      <section class="verevon-cost__panel" aria-label="Model rate card">
        <div class="verevon-cost__panel-head">
          <span class="verevon-cost__panel-title"><Tags size={14} strokeWidth={2.1} /> Rate card</span>
          <span class="verevon-cost__panel-note">USD per 1M tokens</span>
        </div>
        <Show
          when={(pricing() ?? []).length > 0}
          fallback={<p class="verevon-cost__empty">Loading rate card…</p>}
        >
          <div class="verevon-cost__table-wrap">
            <table class="verevon-cost__table">
              <thead>
                <tr>
                  <th>Model</th>
                  <th class="verevon-cost__num">Input / 1M</th>
                  <th class="verevon-cost__num">Output / 1M</th>
                </tr>
              </thead>
              <tbody>
                <For each={pricing() ?? []}>
                  {(rate) => (
                    <tr>
                      <td class="verevon-cost__model">{rate.model}</td>
                      <td class="verevon-cost__num">${rate.inputPerMillion.toFixed(2)}</td>
                      <td class="verevon-cost__num">${rate.outputPerMillion.toFixed(2)}</td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </Show>
      </section>
    </div>
  )
}

function Stat(props: { icon: JSX.Element; label: string; value: string; loading: boolean }) {
  return (
    <div class="verevon-cost__stat">
      <span class="verevon-cost__stat-label">
        {props.icon} {props.label}
      </span>
      <strong class={['verevon-cost__stat-value', { 'verevon-cost__stat-value--loading': props.loading }]}>
        {props.value}
      </strong>
    </div>
  )
}

const COST_CSS = `
.verevon-cost { max-width: 1040px; margin: 0 auto; padding: 24px 28px 64px; display: flex; flex-direction: column; gap: 24px; }
.verevon-cost__topbar { display: flex; align-items: flex-start; gap: 14px; }
.verevon-cost__back { display: inline-flex; align-items: center; justify-content: center; width: 34px; height: 34px; border-radius: 10px; border: 1px solid var(--border, #e7e2da); color: var(--muted-foreground, #6b6660); text-decoration: none; flex: none; }
.verevon-cost__back:hover { background: var(--muted, #f3efe9); }
.verevon-cost__eyebrow { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; letter-spacing: .02em; color: var(--muted-foreground, #8a847c); margin: 0 0 2px; }
.verevon-cost__title { font-size: 22px; font-weight: 650; color: var(--foreground, #20201d); margin: 0; }
.verevon-cost__sub { font-size: 13px; color: var(--muted-foreground, #6b6660); margin: 6px 0 0; max-width: 60ch; }
.verevon-cost__stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 14px; }
.verevon-cost__stat { border: 1px solid var(--border, #e7e2da); border-radius: 14px; padding: 16px 18px; background: var(--card, #fffdfa); display: flex; flex-direction: column; gap: 8px; }
.verevon-cost__stat-label { display: inline-flex; align-items: center; gap: 7px; font-size: 12px; font-weight: 600; color: var(--muted-foreground, #8a847c); }
.verevon-cost__stat-value { font-size: 26px; font-weight: 680; color: var(--foreground, #20201d); font-variant-numeric: tabular-nums; }
.verevon-cost__stat-value--loading { opacity: .4; }
.verevon-cost__panel { border: 1px solid var(--border, #e7e2da); border-radius: 14px; background: var(--card, #fffdfa); overflow: hidden; }
.verevon-cost__panel-head { display: flex; align-items: center; justify-content: space-between; padding: 14px 18px; border-bottom: 1px solid var(--border, #efeae3); }
.verevon-cost__panel-title { display: inline-flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 650; color: var(--foreground, #20201d); }
.verevon-cost__panel-note { font-size: 11px; color: var(--muted-foreground, #8a847c); }
.verevon-cost__empty { padding: 22px 18px; font-size: 13px; color: var(--muted-foreground, #6b6660); margin: 0; }
.verevon-cost__table-wrap { overflow-x: auto; }
.verevon-cost__table { width: 100%; border-collapse: collapse; font-size: 13px; }
.verevon-cost__table th { text-align: left; padding: 10px 18px; font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: .03em; color: var(--muted-foreground, #8a847c); border-bottom: 1px solid var(--border, #efeae3); }
.verevon-cost__table td { padding: 11px 18px; border-bottom: 1px solid var(--border, #f1ece5); color: var(--foreground, #2c2a27); white-space: nowrap; }
.verevon-cost__table tbody tr:last-child td { border-bottom: none; }
.verevon-cost__num { text-align: right; font-variant-numeric: tabular-nums; }
.verevon-cost__model { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; }
.verevon-cost__cost { font-weight: 650; }
`
