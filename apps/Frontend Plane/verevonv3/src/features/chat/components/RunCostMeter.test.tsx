// @vitest-environment jsdom

import { render } from '@solidjs/testing-library'
import { describe, expect, it } from 'vitest'
import { RunCostMeter, type RunCeilingReading } from './RunCostMeter'

function meter(ceiling?: RunCeilingReading): HTMLElement {
  const { container } = render(() => <RunCostMeter ceiling={ceiling} />)
  return container
}

describe('RunCostMeter run ceiling (WS27-S6)', () => {
  it('renders nothing until a frame states a cap the turn can reach', () => {
    expect(meter(undefined).textContent).toBe('')
    // A share alone names no cap, so there is no bar to draw.
    const shareOnly = meter({ fraction: 0.5 })
    expect(shareOnly.querySelector('[role="progressbar"]')).toBeNull()
    expect(shareOnly.textContent).toBe('')
  })

  it('shows the share used on a progress bar beside the spend cap', () => {
    const view = meter({ ceilingUsd: 0.05, fraction: 0.82 })
    const bar = view.querySelector('[role="progressbar"]')
    expect(bar?.getAttribute('aria-valuenow')).toBe('82')
    expect(bar?.getAttribute('aria-valuemin')).toBe('0')
    expect(bar?.getAttribute('aria-valuemax')).toBe('100')
    expect(view.textContent).toContain('$0.05')
  })

  it('states the token cap beside the spend cap when the turn has both', () => {
    const view = meter({ ceilingUsd: 0.05, ceilingTokens: 20000, fraction: 0.1 })
    expect(view.textContent).toContain('$0.05')
    expect(view.textContent).toContain('tokens')
  })

  it('says the share is unknown rather than drawing it as zero', () => {
    // A cap whose spend the gateway cannot price leaves no share: an empty bar would read as "nothing used".
    const view = meter({ ceilingUsd: 0.05 })
    expect(view.querySelector('[role="progressbar"]')).toBeNull()
    expect(view.textContent).toContain('$0.05')
    expect(view.querySelector('.verevon-run-cost-meter__unknown')).not.toBeNull()
  })

  it('carries the 80% warning on the frame that first sends it', () => {
    const view = meter({ ceilingUsd: 0.05, fraction: 0.85, warning: 'run_ceiling_80' })
    const warning = view.querySelector('[data-warning]')
    expect(warning?.getAttribute('data-warning')).toBe('run_ceiling_80')
    expect(warning?.getAttribute('aria-live')).toBe('polite')
    expect(warning?.textContent?.trim()).not.toBe('')
  })

  it('keeps the bar at full width once the cap is passed, and says so', () => {
    const view = meter({ ceilingTokens: 20000, fraction: 1.4, warning: 'run_ceiling_reached' })
    expect(view.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow')).toBe('100')
    expect(view.querySelector('[data-warning]')?.getAttribute('data-warning')).toBe('run_ceiling_reached')
  })

  it('leaves the warning region empty on a frame that sends no warning', () => {
    const view = meter({ ceilingUsd: 0.05, fraction: 0.3 })
    expect(view.querySelector('[data-warning]')?.getAttribute('data-warning')).toBe('')
    expect(view.querySelector('[data-warning]')?.textContent).toBe('')
  })
})
