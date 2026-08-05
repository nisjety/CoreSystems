import { createResource, Show, type JSX } from 'solid-js'
import { A } from '@solidjs/router'
import { ArrowLeft, Gauge, TrendingUp, TrendingDown, Minus, CheckCircle2, XCircle, Activity } from 'lucide-solid'
import { getQualityRollup } from '@/shared/api/eval-client'

// Ops/Quality dashboard (Phase 7 B6). Renders accuracy (run success rate) and
// drift (recent-vs-prior trend) computed from session-core's RunService run
// history, plus the per-run confidence the chat stream now emits. Honest-empty:
// no runs → an explicit empty state, never a fabricated quality score.

function pct(value: number | null): string {
  return value == null ? '—' : `${(value * 100).toFixed(0)}%`
}

function fmtInt(value: number): string {
  return new Intl.NumberFormat(undefined).format(Math.round(value))
}

export default function OpsQualityPage() {
  const [rollup] = createResource(getQualityRollup)

  const hasRuns = () => (rollup()?.totalRuns ?? 0) > 0

  return (
    <div class="verevon-quality">
      <style>{QUALITY_CSS}</style>
      <header class="verevon-quality__topbar">
        <A href="/agents" class="verevon-quality__back" aria-label="Back to agents">
          <ArrowLeft size={16} />
        </A>
        <div>
          <p class="verevon-quality__eyebrow">
            <Gauge size={13} strokeWidth={2.1} /> Ops &amp; quality
          </p>
          <h1 class="verevon-quality__title">Quality &amp; drift</h1>
          <p class="verevon-quality__sub">
            Accuracy is the run success rate from the durable run history; drift compares the
            most recent runs to the prior window. Per-run confidence streams live on each run.
          </p>
        </div>
      </header>

      <Show
        when={hasRuns()}
        fallback={
          <div class="verevon-quality__empty">
            <Show when={!rollup.loading} fallback="Loading run history…">
              No runs recorded yet — run agent tasks and quality, accuracy and drift appear here.
            </Show>
          </div>
        }
      >
        <section class="verevon-quality__stats" aria-label="Quality summary">
          <Stat
            icon={<Gauge size={15} strokeWidth={2.1} />}
            label="Accuracy (success rate)"
            value={pct(rollup()?.accuracy ?? null)}
            hint={`${fmtInt(rollup()?.byStatus.completed ?? 0)} of ${fmtInt(rollup()?.terminalRuns ?? 0)} terminal runs`}
          />
          <Stat
            icon={<Activity size={15} strokeWidth={2.1} />}
            label="Runs analysed"
            value={fmtInt(rollup()?.totalRuns ?? 0)}
            hint={`${fmtInt(rollup()?.threadsSampled ?? 0)} threads sampled`}
          />
          <DriftStat delta={rollup()?.drift.accuracyDelta ?? null} />
          <Stat
            icon={<Activity size={15} strokeWidth={2.1} />}
            label="Avg tokens / run"
            value={`${fmtInt(rollup()?.avgInputTokens ?? 0)} in · ${fmtInt(rollup()?.avgOutputTokens ?? 0)} out`}
          />
        </section>

        <section class="verevon-quality__panel" aria-label="Run outcomes">
          <div class="verevon-quality__panel-head">
            <span class="verevon-quality__panel-title"><CheckCircle2 size={14} strokeWidth={2.1} /> Run outcomes</span>
          </div>
          <div class="verevon-quality__bars">
            <Bar label="Completed" tone="ok" count={rollup()?.byStatus.completed ?? 0} total={rollup()?.totalRuns ?? 0} />
            <Bar label="Failed" tone="error" count={rollup()?.byStatus.failed ?? 0} total={rollup()?.totalRuns ?? 0} />
            <Bar label="Cancelled" tone="warn" count={rollup()?.byStatus.cancelled ?? 0} total={rollup()?.totalRuns ?? 0} />
            <Bar label="In progress" tone="neutral" count={rollup()?.byStatus.running ?? 0} total={rollup()?.totalRuns ?? 0} />
          </div>
        </section>

        <section class="verevon-quality__panel" aria-label="Accuracy drift">
          <div class="verevon-quality__panel-head">
            <span class="verevon-quality__panel-title"><TrendingUp size={14} strokeWidth={2.1} /> Accuracy drift</span>
            <span class="verevon-quality__panel-note">{rollup()?.drift.window}</span>
          </div>
          <Show
            when={rollup()?.drift.recentAccuracy != null}
            fallback={<p class="verevon-quality__empty">Not enough terminal runs yet to compute a drift trend (need ≥ 4).</p>}
          >
            <div class="verevon-quality__drift">
              <div class="verevon-quality__drift-col">
                <span class="verevon-quality__drift-label">Prior window</span>
                <strong class="verevon-quality__drift-value">{pct(rollup()?.drift.priorAccuracy ?? null)}</strong>
              </div>
              <div class="verevon-quality__drift-arrow" aria-hidden="true">→</div>
              <div class="verevon-quality__drift-col">
                <span class="verevon-quality__drift-label">Recent window</span>
                <strong class="verevon-quality__drift-value">{pct(rollup()?.drift.recentAccuracy ?? null)}</strong>
              </div>
            </div>
          </Show>
        </section>
      </Show>
    </div>
  )
}

function Stat(props: { icon: JSX.Element; label: string; value: string; hint?: string }) {
  return (
    <div class="verevon-quality__stat">
      <span class="verevon-quality__stat-label">{props.icon} {props.label}</span>
      <strong class="verevon-quality__stat-value">{props.value}</strong>
      <Show when={props.hint}>
        <span class="verevon-quality__stat-hint">{props.hint}</span>
      </Show>
    </div>
  )
}

function DriftStat(props: { delta: number | null }) {
  const tone = () => {
    const d = props.delta
    if (d == null) return 'neutral'
    if (d > 0.001) return 'ok'
    if (d < -0.001) return 'error'
    return 'neutral'
  }
  const icon = () => {
    const t = tone()
    if (t === 'ok') return <TrendingUp size={15} strokeWidth={2.1} />
    if (t === 'error') return <TrendingDown size={15} strokeWidth={2.1} />
    return <Minus size={15} strokeWidth={2.1} />
  }
  const value = () => {
    const d = props.delta
    if (d == null) return '—'
    const sign = d > 0 ? '+' : ''
    return `${sign}${(d * 100).toFixed(0)} pts`
  }
  return (
    <div class="verevon-quality__stat" data-tone={tone()}>
      <span class="verevon-quality__stat-label">{icon()} Accuracy trend</span>
      <strong class="verevon-quality__stat-value">{value()}</strong>
      <span class="verevon-quality__stat-hint">recent vs prior window</span>
    </div>
  )
}

function Bar(props: { label: string; tone: string; count: number; total: number }) {
  const widthPct = () => (props.total > 0 ? Math.round((props.count / props.total) * 100) : 0)
  return (
    <div class="verevon-quality__bar-row">
      <span class="verevon-quality__bar-label">
        <Show when={props.tone === 'error'}><XCircle size={12} strokeWidth={2.2} /></Show>
        {props.label}
      </span>
      <div class="verevon-quality__bar-track">
        <div class={`verevon-quality__bar-fill verevon-quality__bar-fill--${props.tone}`} style={{ width: `${widthPct()}%` }} />
      </div>
      <span class="verevon-quality__bar-count">{fmtInt(props.count)}</span>
    </div>
  )
}

const QUALITY_CSS = `
.verevon-quality { max-width: 1040px; margin: 0 auto; padding: 24px 28px 64px; display: flex; flex-direction: column; gap: 24px; }
.verevon-quality__topbar { display: flex; align-items: flex-start; gap: 14px; }
.verevon-quality__back { display: inline-flex; align-items: center; justify-content: center; width: 34px; height: 34px; border-radius: 10px; border: 1px solid var(--border, #e7e2da); color: var(--muted-foreground, #6b6660); text-decoration: none; flex: none; }
.verevon-quality__back:hover { background: var(--muted, #f3efe9); }
.verevon-quality__eyebrow { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: var(--muted-foreground, #8a847c); margin: 0 0 2px; }
.verevon-quality__title { font-size: 22px; font-weight: 650; color: var(--foreground, #20201d); margin: 0; }
.verevon-quality__sub { font-size: 13px; color: var(--muted-foreground, #6b6660); margin: 6px 0 0; max-width: 64ch; }
.verevon-quality__empty { padding: 22px 18px; font-size: 13px; color: var(--muted-foreground, #6b6660); border: 1px dashed var(--border, #e0dad1); border-radius: 14px; }
.verevon-quality__stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 14px; }
.verevon-quality__stat { border: 1px solid var(--border, #e7e2da); border-radius: 14px; padding: 16px 18px; background: var(--card, #fffdfa); display: flex; flex-direction: column; gap: 6px; }
.verevon-quality__stat-label { display: inline-flex; align-items: center; gap: 7px; font-size: 12px; font-weight: 600; color: var(--muted-foreground, #8a847c); }
.verevon-quality__stat-value { font-size: 26px; font-weight: 680; color: var(--foreground, #20201d); font-variant-numeric: tabular-nums; }
.verevon-quality__stat[data-tone="ok"] .verevon-quality__stat-value { color: #1f8a4c; }
.verevon-quality__stat[data-tone="error"] .verevon-quality__stat-value { color: #c0392b; }
.verevon-quality__stat-hint { font-size: 11px; color: var(--muted-foreground, #8a847c); }
.verevon-quality__panel { border: 1px solid var(--border, #e7e2da); border-radius: 14px; background: var(--card, #fffdfa); overflow: hidden; }
.verevon-quality__panel-head { display: flex; align-items: center; justify-content: space-between; padding: 14px 18px; border-bottom: 1px solid var(--border, #efeae3); }
.verevon-quality__panel-title { display: inline-flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 650; color: var(--foreground, #20201d); }
.verevon-quality__panel-note { font-size: 11px; color: var(--muted-foreground, #8a847c); }
.verevon-quality__bars { padding: 16px 18px; display: flex; flex-direction: column; gap: 12px; }
.verevon-quality__bar-row { display: grid; grid-template-columns: 110px 1fr 48px; align-items: center; gap: 12px; }
.verevon-quality__bar-label { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--foreground, #2c2a27); }
.verevon-quality__bar-track { height: 8px; border-radius: 99px; background: var(--muted, #efeae3); overflow: hidden; }
.verevon-quality__bar-fill { height: 100%; border-radius: 99px; min-width: 2px; }
.verevon-quality__bar-fill--ok { background: #2fa45e; }
.verevon-quality__bar-fill--error { background: #d9534f; }
.verevon-quality__bar-fill--warn { background: #d99a2b; }
.verevon-quality__bar-fill--neutral { background: #9b95b8; }
.verevon-quality__bar-count { text-align: right; font-size: 13px; font-variant-numeric: tabular-nums; color: var(--foreground, #2c2a27); }
.verevon-quality__drift { padding: 20px 18px; display: flex; align-items: center; gap: 20px; }
.verevon-quality__drift-col { display: flex; flex-direction: column; gap: 4px; }
.verevon-quality__drift-label { font-size: 11px; color: var(--muted-foreground, #8a847c); }
.verevon-quality__drift-value { font-size: 24px; font-weight: 680; font-variant-numeric: tabular-nums; color: var(--foreground, #20201d); }
.verevon-quality__drift-arrow { font-size: 20px; color: var(--muted-foreground, #b3aca2); }
`
