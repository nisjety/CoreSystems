/**
 * The run-cost meter of a turn or an agent run (WS27-S4), with the run ceiling its frames carry (WS27-S6).
 *
 * It shows what the `usage_progress` frames and the turn's `usage` frame state, and nothing more. A price
 * the gateway could not state reads as "price unavailable", a subscription call reads as included in the
 * subscription and never as $0, and a share the gateway could not compute reads as unknown. Nothing here
 * estimates a cost.
 */

import { createMemo, For, Show } from 'solid-js'
import type { ChatUsageEvent, ChatUsagePhase, ChatUsageProgressEvent } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'
import { localeIntl } from '@/shared/i18n/locales'
import { formatUsd } from './chat-media-markdown'
import type { RunMeter } from './chat-types'

/**
 * The total after one `usage_progress` frame. A frame that counts fewer calls than the total is late or
 * replayed, so the total stays as it was. A warning is stated once, on the frame that first crosses it,
 * and stays shown until a stronger one arrives. The share is the frame's own: a frame that states none
 * leaves it unknown rather than at the last share.
 */
export function applyUsageProgress(current: RunMeter | undefined, frame: ChatUsageProgressEvent): RunMeter {
  if (current?.calls !== undefined && frame.calls !== undefined && frame.calls < current.calls) {
    return { ...current }
  }
  return {
    calls: frame.calls,
    phase: frame.phase,
    inputTokens: frame.inputTokens,
    outputTokens: frame.outputTokens,
    cacheReadTokens: frame.cacheReadTokens,
    cacheWriteTokens: frame.cacheWriteTokens,
    costUsd: frame.costBasis === 'subscription' ? undefined : frame.costUsd,
    costBasis: frame.costBasis,
    unpricedCalls: frame.unpricedCalls,
    ceilingUsd: frame.ceilingUsd ?? current?.ceilingUsd,
    ceilingTokens: frame.ceilingTokens ?? current?.ceilingTokens,
    fraction: frame.fraction,
    warning: frame.warning ?? current?.warning,
  }
}

/**
 * The total the turn's `usage` frame states. The gateway's final total is the authority, so it replaces
 * every progress total. The frame carries neither the last call's phase, nor the caps, nor the share, so
 * those are kept from the progress frames.
 */
export function applyUsageTotal(current: RunMeter | undefined, usage: ChatUsageEvent): RunMeter {
  const subscription = usage.costBasis === 'subscription'
  return {
    calls: usage.calls ?? current?.calls,
    phase: current?.phase,
    inputTokens: usage.inputTokens,
    outputTokens: usage.outputTokens,
    cacheReadTokens: usage.cacheReadTokens,
    cacheWriteTokens: usage.cacheWriteTokens,
    costUsd: subscription ? undefined : usage.costUsd,
    costBasis: usage.costBasis,
    unpricedCalls: usage.unpricedCalls,
    costByCategory: subscription ? undefined : usage.costByCategory,
    ceilingUsd: current?.ceilingUsd,
    ceilingTokens: current?.ceilingTokens,
    fraction: current?.fraction,
    warning: current?.warning,
  }
}

/** A spend cap at the precision it needs: a cap of a few cents must not round to a misleading figure. */
function formatCapUsd(usd: number): string {
  return `$${usd < 0.01 ? usd.toFixed(4) : usd.toFixed(2)}`
}

/** The share as a whole percentage, clamped to the bar's range. */
function percentOf(fraction: number): number {
  return Math.min(100, Math.max(0, Math.round(fraction * 100)))
}

/** The bar's fill: the accent, the strong accent from 80%, and the danger token once the cap is reached. */
function fillToken(fraction: number | undefined): string {
  if (fraction === undefined) return 'var(--verevon-accent)'
  if (fraction >= 1) return 'var(--danger)'
  if (fraction >= 0.8) return 'var(--verevon-accent-strong)'
  return 'var(--verevon-accent)'
}

/** The warning chip's look. Applied only while a warning is set, so an empty warning region takes no space. */
const WARNING_CHIP_STYLE = {
  margin: '0',
  'align-self': 'flex-start',
  padding: '2px 8px',
  'border-radius': '999px',
  background: 'var(--verevon-surface-soft)',
  color: 'var(--verevon-text)',
  'font-weight': '600',
}

function CostPart(props: { label: string; usd: number }) {
  return (
    <>
      <dt>{props.label}</dt>
      <dd style={{ margin: '0' }}>{formatUsd(props.usd)}</dd>
    </>
  )
}

export function RunCostMeter(props: { meter?: RunMeter }) {
  const i18n = useI18n()
  const count = (value: number) => value.toLocaleString(localeIntl(i18n.locale()))

  const hasTotals = () => {
    const meter = props.meter
    return meter !== undefined && (
      meter.calls !== undefined
      || meter.inputTokens !== undefined
      || meter.outputTokens !== undefined
      || meter.costBasis !== undefined
      || meter.costUsd !== undefined
    )
  }
  const hasCap = () => props.meter?.ceilingUsd !== undefined || props.meter?.ceilingTokens !== undefined

  const phaseLabel = (phase: ChatUsagePhase): string => {
    switch (phase) {
      case 'answer': return i18n.tr('Svar', 'Answer')
      case 'tool_round': return i18n.tr('Verktøyrunde', 'Tool round')
      case 'agent_round': return i18n.tr('Agentrunde', 'Agent round')
      case 'overhead': return i18n.tr('Kontrollkall', 'Check call')
    }
  }

  // A subscription is checked before the cost, so its wire placeholder can never be read as a price.
  const costLabel = (meter: RunMeter): string => {
    if (meter.costBasis === 'subscription') return i18n.tr('Inkludert i abonnementet', 'Included in the subscription')
    if (meter.costUsd !== undefined && (meter.unpricedCalls ?? 0) === 0) {
      const cost = formatUsd(meter.costUsd)
      // The gateway states no basis for a total that mixes metered and subscription calls.
      // Its cost is then the metered calls alone, and the label says so.
      return meter.costBasis === undefined
        ? i18n.tr(`${cost}, bare kall med tokenpris`, `${cost}, token-priced calls only`)
        : cost
    }
    return i18n.tr('Pris ikke tilgjengelig', 'Price unavailable')
  }

  const cacheLabel = (cacheRead: number, input: number | undefined): string => {
    if (input !== undefined && input > 0) {
      const share = Math.min(100, Math.round((cacheRead / input) * 100))
      return i18n.tr(`${share} % av inndata fra hurtigbuffer`, `${share}% of input from cache`)
    }
    return i18n.tr(`${count(cacheRead)} fra hurtigbuffer`, `${count(cacheRead)} from cache`)
  }

  const headline = createMemo((): string[] => {
    const meter = props.meter
    if (!meter || !hasTotals()) return []
    const parts: string[] = []
    if (meter.calls !== undefined && meter.calls > 0) {
      parts.push(i18n.tr(`${meter.calls} kall`, meter.calls === 1 ? '1 call' : `${meter.calls} calls`))
    }
    if (meter.inputTokens !== undefined || meter.outputTokens !== undefined) {
      const input = count(meter.inputTokens ?? 0)
      const output = count(meter.outputTokens ?? 0)
      parts.push(i18n.tr(`${input} inn · ${output} ut`, `${input} in · ${output} out`))
    }
    // Shown only when the provider read from the cache, so a turn without one carries no cache figure at all.
    if ((meter.cacheReadTokens ?? 0) > 0) parts.push(cacheLabel(meter.cacheReadTokens ?? 0, meter.inputTokens))
    parts.push(costLabel(meter))
    if (meter.phase !== undefined) {
      parts.push(i18n.tr(`Siste kall: ${phaseLabel(meter.phase)}`, `Last call: ${phaseLabel(meter.phase)}`))
    }
    return parts
  })

  // Only for a priced total, so no breakdown sits beside "price unavailable" or a subscription.
  const breakdown = () => {
    const meter = props.meter
    return meter?.costByCategory !== undefined && meter.costUsd !== undefined && meter.costBasis !== 'subscription'
      ? meter.costByCategory
      : undefined
  }

  const percent = () => {
    const fraction = props.meter?.fraction
    return fraction === undefined ? undefined : percentOf(fraction)
  }

  const capLabel = () => {
    const meter = props.meter
    const parts: string[] = []
    if (meter?.ceilingUsd !== undefined) parts.push(formatCapUsd(meter.ceilingUsd))
    if (meter?.ceilingTokens !== undefined) parts.push(`${count(meter.ceilingTokens)} tokens`)
    return parts.join(' · ')
  }

  // The text is a memo, so a frame that repeats the warning writes nothing and a screen reader hears it once.
  const warningText = createMemo(() => {
    switch (props.meter?.warning) {
      case 'run_ceiling_80':
        return i18n.tr('Denne kjøringen har brukt over 80 % av grensen.', 'This run has used over 80% of its limit.')
      case 'run_ceiling_reached':
        return i18n.tr('Denne kjøringen har nådd grensen.', 'This run has reached its limit.')
      default:
        return ''
    }
  })

  return (
    <Show when={hasTotals() || hasCap()}>
      <div
        class="verevon-run-cost-meter"
        role="group"
        aria-label={i18n.tr('Kjøringens kostnad og bruk', 'Run cost and usage')}
        style={{
          display: 'flex',
          'flex-direction': 'column',
          gap: '6px',
          margin: '0 0 10px',
          'font-size': '12px',
          color: 'var(--verevon-text-muted)',
        }}
      >
        <Show when={hasTotals()}>
          <p class="verevon-run-cost-meter__line" style={{ margin: '0' }}>
            <For each={headline()}>
              {(part, index) => (
                <>
                  <Show when={index() > 0}><span aria-hidden="true"> · </span></Show>
                  <span>{part}</span>
                </>
              )}
            </For>
          </p>
          <Show when={breakdown()}>
            {(parts) => (
              <details class="verevon-run-cost-meter__breakdown">
                <summary>{i18n.tr('Kostnad etter kategori', 'Cost by category')}</summary>
                <dl style={{ display: 'grid', 'grid-template-columns': 'auto auto', gap: '2px 12px', margin: '6px 0 0' }}>
                  <CostPart label={i18n.tr('Inndata', 'Input')} usd={parts().inputUsd} />
                  <CostPart label={i18n.tr('Hurtigbuffer lest', 'Cache read')} usd={parts().cacheReadUsd} />
                  <CostPart label={i18n.tr('Hurtigbuffer skrevet', 'Cache write')} usd={parts().cacheWriteUsd} />
                  <CostPart label={i18n.tr('Utdata', 'Output')} usd={parts().outputUsd} />
                </dl>
              </details>
            )}
          </Show>
        </Show>
        <Show when={hasCap()}>
          <span class="verevon-run-cost-meter__cap">
            {i18n.tr('Grense for kjøringen', 'Run limit')}: {capLabel()}
          </span>
          <Show
            when={percent() !== undefined}
            fallback={<span class="verevon-run-cost-meter__unknown">{i18n.tr('Andelen er ukjent', 'Share unknown')}</span>}
          >
            <div
              class="verevon-run-cost-meter__bar"
              role="progressbar"
              aria-label={i18n.tr('Brukt av grensen for kjøringen', 'Used of the run limit')}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={percent()}
              style={{ width: '100%', height: '6px', background: 'var(--verevon-border)', 'border-radius': '3px' }}
            >
              <span
                style={{
                  display: 'block',
                  height: '100%',
                  width: `${percent() ?? 0}%`,
                  background: fillToken(props.meter?.fraction),
                  'border-radius': '3px',
                }}
              />
            </div>
          </Show>
          {/* Persistent so a screen reader hears a warning when it arrives, and silent otherwise. */}
          <p
            class="verevon-run-cost-meter__warning"
            aria-live="polite"
            data-warning={props.meter?.warning ?? ''}
            style={props.meter?.warning ? WARNING_CHIP_STYLE : { margin: '0' }}
          >
            {warningText()}
          </p>
        </Show>
      </div>
    </Show>
  )
}
