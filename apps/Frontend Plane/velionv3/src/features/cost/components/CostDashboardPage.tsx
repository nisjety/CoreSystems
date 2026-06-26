import { createResource, For, Show, type JSX } from 'solid-js'
import { A } from '@solidjs/router'
import { ArrowLeft, Coins, Receipt, Activity, Tags } from 'lucide-solid'
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
    <div class="velion-cost">
      <style>{COST_CSS}</style>
      <header class="velion-cost__topbar">
        <A href="/agents" class="velion-cost__back" aria-label="Back to agents">
          <ArrowLeft size={16} />
        </A>
        <div>
          <p class="velion-cost__eyebrow">
            <Coins size={13} strokeWidth={2.1} /> Cost &amp; usage
          </p>
          <h1 class="velion-cost__title">Cost dashboard</h1>
          <p class="velion-cost__sub">
            Real per-run spend from the model ledger. Every figure is priced from the
            catalogue cost-core records each inference against.
          </p>
        </div>
      </header>

      <section class="velion-cost__stats" aria-label="Spend summary">
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

      <section class="velion-cost__panel" aria-label="Recent inferences">
        <div class="velion-cost__panel-head">
          <span class="velion-cost__panel-title"><Receipt size={14} strokeWidth={2.1} /> Recent inferences</span>
        </div>
        <Show
          when={(entries() ?? []).length > 0}
          fallback={
            <p class="velion-cost__empty">
              <Show when={!entries.loading} fallback="Loading…">
                No billed inferences yet — run a task in chat or the Agent Run Console and spend appears here.
              </Show>
            </p>
          }
        >
          <div class="velion-cost__table-wrap">
            <table class="velion-cost__table">
              <thead>
                <tr>
                  <th>When</th>
                  <th>Model</th>
                  <th class="velion-cost__num">Input</th>
                  <th class="velion-cost__num">Output</th>
                  <th class="velion-cost__num">Cost</th>
                </tr>
              </thead>
              <tbody>
                <For each={entries() ?? []}>
                  {(row) => (
                    <tr>
                      <td>{fmtClock(row.createdAt)}</td>
                      <td class="velion-cost__model">{row.model || '—'}</td>
                      <td class="velion-cost__num">{fmtInt(row.inputTokens)}</td>
                      <td class="velion-cost__num">{fmtInt(row.outputTokens)}</td>
                      <td class="velion-cost__num velion-cost__cost">{fmtUsd(row.costUsd)}</td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </Show>
      </section>

      <section class="velion-cost__panel" aria-label="Model rate card">
        <div class="velion-cost__panel-head">
          <span class="velion-cost__panel-title"><Tags size={14} strokeWidth={2.1} /> Rate card</span>
          <span class="velion-cost__panel-note">USD per 1M tokens</span>
        </div>
        <Show
          when={(pricing() ?? []).length > 0}
          fallback={<p class="velion-cost__empty">Loading rate card…</p>}
        >
          <div class="velion-cost__table-wrap">
            <table class="velion-cost__table">
              <thead>
                <tr>
                  <th>Model</th>
                  <th class="velion-cost__num">Input / 1M</th>
                  <th class="velion-cost__num">Output / 1M</th>
                </tr>
              </thead>
              <tbody>
                <For each={pricing() ?? []}>
                  {(rate) => (
                    <tr>
                      <td class="velion-cost__model">{rate.model}</td>
                      <td class="velion-cost__num">${rate.inputPerMillion.toFixed(2)}</td>
                      <td class="velion-cost__num">${rate.outputPerMillion.toFixed(2)}</td>
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
    <div class="velion-cost__stat">
      <span class="velion-cost__stat-label">
        {props.icon} {props.label}
      </span>
      <strong class="velion-cost__stat-value" classList={{ 'velion-cost__stat-value--loading': props.loading }}>
        {props.value}
      </strong>
    </div>
  )
}

const COST_CSS = `
.velion-cost { max-width: 1040px; margin: 0 auto; padding: 24px 28px 64px; display: flex; flex-direction: column; gap: 24px; }
.velion-cost__topbar { display: flex; align-items: flex-start; gap: 14px; }
.velion-cost__back { display: inline-flex; align-items: center; justify-content: center; width: 34px; height: 34px; border-radius: 10px; border: 1px solid var(--border, #e7e2da); color: var(--muted-foreground, #6b6660); text-decoration: none; flex: none; }
.velion-cost__back:hover { background: var(--muted, #f3efe9); }
.velion-cost__eyebrow { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; letter-spacing: .02em; color: var(--muted-foreground, #8a847c); margin: 0 0 2px; }
.velion-cost__title { font-size: 22px; font-weight: 650; color: var(--foreground, #20201d); margin: 0; }
.velion-cost__sub { font-size: 13px; color: var(--muted-foreground, #6b6660); margin: 6px 0 0; max-width: 60ch; }
.velion-cost__stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 14px; }
.velion-cost__stat { border: 1px solid var(--border, #e7e2da); border-radius: 14px; padding: 16px 18px; background: var(--card, #fffdfa); display: flex; flex-direction: column; gap: 8px; }
.velion-cost__stat-label { display: inline-flex; align-items: center; gap: 7px; font-size: 12px; font-weight: 600; color: var(--muted-foreground, #8a847c); }
.velion-cost__stat-value { font-size: 26px; font-weight: 680; color: var(--foreground, #20201d); font-variant-numeric: tabular-nums; }
.velion-cost__stat-value--loading { opacity: .4; }
.velion-cost__panel { border: 1px solid var(--border, #e7e2da); border-radius: 14px; background: var(--card, #fffdfa); overflow: hidden; }
.velion-cost__panel-head { display: flex; align-items: center; justify-content: space-between; padding: 14px 18px; border-bottom: 1px solid var(--border, #efeae3); }
.velion-cost__panel-title { display: inline-flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 650; color: var(--foreground, #20201d); }
.velion-cost__panel-note { font-size: 11px; color: var(--muted-foreground, #8a847c); }
.velion-cost__empty { padding: 22px 18px; font-size: 13px; color: var(--muted-foreground, #6b6660); margin: 0; }
.velion-cost__table-wrap { overflow-x: auto; }
.velion-cost__table { width: 100%; border-collapse: collapse; font-size: 13px; }
.velion-cost__table th { text-align: left; padding: 10px 18px; font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: .03em; color: var(--muted-foreground, #8a847c); border-bottom: 1px solid var(--border, #efeae3); }
.velion-cost__table td { padding: 11px 18px; border-bottom: 1px solid var(--border, #f1ece5); color: var(--foreground, #2c2a27); white-space: nowrap; }
.velion-cost__table tbody tr:last-child td { border-bottom: none; }
.velion-cost__num { text-align: right; font-variant-numeric: tabular-nums; }
.velion-cost__model { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; }
.velion-cost__cost { font-weight: 650; }
`
