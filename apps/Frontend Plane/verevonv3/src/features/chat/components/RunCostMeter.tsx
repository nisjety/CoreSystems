/**
 * The run's ceiling on the chat Work panel (WS27-S6): how far the turn has come toward its spend or
 * token cap, and the warning on the frame that first crossed 80% of it or reached it.
 *
 * It shows only what a `usage_progress` frame states. A share the gateway could not compute is shown
 * as unknown, never as zero, and nothing here estimates a cost.
 */

import { Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { localeIntl } from '@/shared/i18n/locales'

/** The closed tokens a progress frame names a ceiling warning by. */
export type RunCeilingWarning = 'run_ceiling_80' | 'run_ceiling_reached'

/**
 * A turn's ceiling as its progress frames state it. Every key is optional, because a frame carries only
 * what it has: a cap the turn does not have, or a share it cannot compute, is left out.
 */
export type RunCeilingReading = {
  /** The metered spend cap, in US dollars. */
  ceilingUsd?: number
  /** The metered token cap: input and output tokens of the metered calls. */
  ceilingTokens?: number
  /** The share of the ceiling used. Above 1 once the cap is passed. */
  fraction?: number
  /** The warning this frame is the first to carry. */
  warning?: RunCeilingWarning
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

export function RunCostMeter(props: { ceiling?: RunCeilingReading }) {
  const i18n = useI18n()

  const hasCap = () => {
    const ceiling = props.ceiling
    return ceiling !== undefined && (ceiling.ceilingUsd !== undefined || ceiling.ceilingTokens !== undefined)
  }

  const percent = () => {
    const fraction = props.ceiling?.fraction
    return fraction === undefined ? undefined : percentOf(fraction)
  }

  const capLabel = () => {
    const ceiling = props.ceiling
    const parts: string[] = []
    if (ceiling?.ceilingUsd !== undefined) parts.push(formatCapUsd(ceiling.ceilingUsd))
    if (ceiling?.ceilingTokens !== undefined) {
      parts.push(`${ceiling.ceilingTokens.toLocaleString(localeIntl(i18n.locale()))} tokens`)
    }
    return parts.join(' · ')
  }

  const warningText = () => {
    switch (props.ceiling?.warning) {
      case 'run_ceiling_80':
        return i18n.tr('Denne kjøringen har brukt over 80 % av grensen.', 'This run has used over 80% of its limit.')
      case 'run_ceiling_reached':
        return i18n.tr('Denne kjøringen har nådd grensen.', 'This run has reached its limit.')
      default:
        return ''
    }
  }

  return (
    <Show when={hasCap()}>
      <div class="verevon-run-cost-meter">
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
                background: fillToken(props.ceiling?.fraction),
                'border-radius': '3px',
              }}
            />
          </div>
        </Show>
        {/* Persistent so a screen reader hears a warning when it arrives, and silent otherwise. */}
        <p class="verevon-run-cost-meter__warning" aria-live="polite" data-warning={props.ceiling?.warning ?? ''}>
          {warningText()}
        </p>
      </div>
    </Show>
  )
}
