// @vitest-environment jsdom

import { render } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { RunCostMeter, applyUsageProgress, applyUsageTotal } from './RunCostMeter'
import type { RunMeter } from './chat-types'

function meter(value?: RunMeter): HTMLElement {
  const { container } = render(() => <RunCostMeter meter={value} />)
  return container
}

describe('RunCostMeter run ceiling (WS27-S6)', () => {
  it('renders nothing until a frame states a total or a cap', () => {
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

  it('announces a warning once: a frame that repeats it leaves the live region untouched', async () => {
    const [value, setValue] = createSignal<RunMeter | undefined>({ ceilingUsd: 0.05, fraction: 0.85, warning: 'run_ceiling_80' })
    const { container } = render(() => <RunCostMeter meter={value()} />)
    const region = container.querySelector('[data-warning]') as HTMLElement
    const changes: string[] = []
    const observer = new MutationObserver(() => changes.push(region.textContent ?? ''))
    observer.observe(region, { childList: true, characterData: true, subtree: true })

    setValue({ ceilingUsd: 0.05, fraction: 0.9, warning: 'run_ceiling_80' })
    flush()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(changes).toHaveLength(0)

    setValue({ ceilingUsd: 0.05, fraction: 1.1, warning: 'run_ceiling_reached' })
    flush()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(changes).toHaveLength(1)
    expect(region.getAttribute('data-warning')).toBe('run_ceiling_reached')
    observer.disconnect()
  })
})

describe('RunCostMeter totals (WS27-S4)', () => {
  it('states the calls, the tokens in and out, the cost and the last call phase', () => {
    const view = meter({
      calls: 3,
      phase: 'tool_round',
      inputTokens: 1200,
      outputTokens: 340,
      costUsd: 0.0031,
      costBasis: 'metered',
    })
    expect(view.textContent).toMatch(/3 kall|3 calls/)
    expect(view.textContent).toContain('340')
    expect(view.textContent).toContain('$0.0031')
    expect(view.textContent).toMatch(/Verktøyrunde|Tool round/)
  })

  it('never shows $0 for a subscription turn, and says it is included in the subscription', () => {
    // The wire's placeholder cost rides along and must still not be read as a price.
    const view = meter({ calls: 2, inputTokens: 500, outputTokens: 50, costUsd: 0, costBasis: 'subscription' })
    expect(view.textContent).toMatch(/Inkludert i abonnementet|Included in the subscription/)
    expect(view.textContent).not.toContain('$')
  })

  it('says a cost covers only the token-priced calls when the turn also had subscription calls', () => {
    // The gateway states no basis for a mixed total; its cost is the metered part alone.
    const mixed = meter({ calls: 2, inputTokens: 1500, outputTokens: 150, costUsd: 0.25 })
    expect(mixed.textContent).toContain('$0.25')
    expect(mixed.textContent).toMatch(/bare kall med tokenpris|token-priced calls only/)

    const metered = meter({ calls: 1, inputTokens: 1000, outputTokens: 100, costUsd: 0.25, costBasis: 'metered' })
    expect(metered.textContent).toContain('$0.25')
    expect(metered.textContent).not.toMatch(/tokenpris|token-priced/)
  })

  it('shows the tokens and says the price is unavailable when a call was unpriced', () => {
    const view = meter({ calls: 1, inputTokens: 900, outputTokens: 100, unpricedCalls: 1 })
    expect(view.textContent).toContain('900')
    expect(view.textContent).toMatch(/Pris ikke tilgjengelig|Price unavailable/)
    expect(view.textContent).not.toContain('$')
  })

  it('shows the cache share only when the provider read from the cache', () => {
    expect(meter({ inputTokens: 1000, outputTokens: 10, cacheReadTokens: 0 }).textContent).not.toMatch(/fra hurtigbuffer|from cache/)
    expect(meter({ inputTokens: 1000, outputTokens: 10, cacheReadTokens: 250 }).textContent).toMatch(/25 % av inndata|25% of input/)
  })

  it('shows the cost by category for a priced total, and only then', () => {
    const categories = { inputUsd: 0.1, cacheReadUsd: 0.2, cacheWriteUsd: 0.05, outputUsd: 0.15 }
    const priced = meter({ calls: 1, inputTokens: 10, outputTokens: 5, costUsd: 0.5, costBasis: 'metered', costByCategory: categories })
    expect(priced.querySelector('details.verevon-run-cost-meter__breakdown')).not.toBeNull()
    expect(priced.querySelectorAll('dd')).toHaveLength(4)

    const unpriced = meter({ calls: 1, inputTokens: 10, outputTokens: 5, unpricedCalls: 1, costByCategory: categories })
    expect(unpriced.querySelector('details')).toBeNull()
  })
})

describe('RunCostMeter reducers (WS27-S4)', () => {
  it('ignores a progress frame that counts fewer calls than the total it would replace', () => {
    const total = applyUsageProgress({ calls: 3, inputTokens: 900 }, { calls: 2, inputTokens: 500 })
    expect(total.calls).toBe(3)
    expect(total.inputTokens).toBe(900)
  })

  it('takes a progress frame that counts as many calls as the total', () => {
    const total = applyUsageProgress({ calls: 2, inputTokens: 500 }, { calls: 2, inputTokens: 700 })
    expect(total.inputTokens).toBe(700)
  })

  it('keeps a warning once it is stated, until a stronger one arrives', () => {
    const warned = applyUsageProgress(undefined, { calls: 2, warning: 'run_ceiling_80', ceilingUsd: 0.05, fraction: 0.85 })
    const later = applyUsageProgress(warned, { calls: 3, fraction: 0.9 })
    expect(later.warning).toBe('run_ceiling_80')
    expect(applyUsageProgress(later, { calls: 4, warning: 'run_ceiling_reached', fraction: 1.1 }).warning).toBe('run_ceiling_reached')
  })

  it('reads the share from the frame that states it, never from an earlier frame', () => {
    const earlier = applyUsageProgress(undefined, { calls: 1, fraction: 0.5, ceilingUsd: 0.05 })
    const later = applyUsageProgress(earlier, { calls: 2 })
    expect(later.fraction).toBeUndefined()
    expect(later.ceilingUsd).toBe(0.05)
  })

  it('never carries a subscription cost, even a wire placeholder', () => {
    const total = applyUsageProgress(undefined, { calls: 1, costBasis: 'subscription', costUsd: 0 })
    expect(total.costUsd).toBeUndefined()
    expect(total.costBasis).toBe('subscription')
  })

  it('lets the usage frame replace every progress total, and keeps the phase, the caps and the warning', () => {
    const progress = applyUsageProgress(undefined, {
      calls: 2,
      phase: 'tool_round',
      inputTokens: 900,
      costUsd: 0.001,
      costBasis: 'metered',
      ceilingUsd: 0.05,
      fraction: 0.9,
      warning: 'run_ceiling_80',
    })
    const total = applyUsageTotal(progress, {
      calls: 3,
      inputTokens: 1000,
      outputTokens: 80,
      costUsd: 0.004,
      costBasis: 'metered',
      unpricedCalls: 0,
      cacheReadTokens: 200,
      costByCategory: { inputUsd: 0.001, cacheReadUsd: 0.0002, cacheWriteUsd: 0, outputUsd: 0.0028 },
    })
    expect(total).toMatchObject({
      calls: 3,
      inputTokens: 1000,
      outputTokens: 80,
      costUsd: 0.004,
      phase: 'tool_round',
      ceilingUsd: 0.05,
      warning: 'run_ceiling_80',
    })
    expect(total.costByCategory?.outputUsd).toBe(0.0028)
  })

  it('drops the placeholder cost and breakdown of a subscription total', () => {
    const total = applyUsageTotal(undefined, { calls: 1, costUsd: 0, costBasis: 'subscription' })
    expect(total.costUsd).toBeUndefined()
    expect(total.costByCategory).toBeUndefined()
  })
})

describe('RunCostMeter organisation allowance (WS27-S10)', () => {
  const metric = 'ai.metered.input_tokens'
  const quota = { org_id: 'org_1', metric, limit: 1000, used: 850, remaining: 150, is_exceeded: false, utilization: 0.85 }

  afterEach(() => {
    vi.unstubAllEnvs()
    vi.unstubAllGlobals()
  })

  function readoutOn(): void {
    vi.stubEnv('VITE_QUOTA_READOUT', 'true')
    vi.stubEnv('VITE_QUOTA_READOUT_METRIC', metric)
  }

  function quotaFetch(body: Record<string, unknown>, status = 200) {
    const fetchMock = vi.fn<(input: string, init?: RequestInit) => Promise<Response>>(
      async () => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }),
    )
    vi.stubGlobal('fetch', fetchMock)
    return fetchMock
  }

  async function settle(): Promise<void> {
    flush()
    await new Promise((resolve) => setTimeout(resolve, 10))
    flush()
  }

  it('reads nothing and shows no line while the readout is off, even with a metric named', async () => {
    vi.stubEnv('VITE_QUOTA_READOUT', 'false')
    vi.stubEnv('VITE_QUOTA_READOUT_METRIC', metric)
    const fetchMock = quotaFetch(quota)
    const view = meter(undefined)
    await settle()
    expect(fetchMock).not.toHaveBeenCalled()
    expect(view.textContent).toBe('')
  })

  it('reads the allowance when the meter opens, and again only when the refresh key changes, never per frame', async () => {
    readoutOn()
    const fetchMock = quotaFetch(quota)
    const [value, setValue] = createSignal<RunMeter | undefined>({ calls: 1, inputTokens: 10 })
    const [key, setKey] = createSignal('thread-1:0')
    render(() => <RunCostMeter meter={value()} quotaRefreshKey={key()} />)
    await settle()
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[0]).toContain(`/api/v1/billing/quotas/${metric}`)

    setValue({ calls: 2, inputTokens: 40 })
    setValue({ calls: 3, inputTokens: 90 })
    await settle()
    expect(fetchMock).toHaveBeenCalledTimes(1)

    setKey('thread-1:1')
    await settle()
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('states the organisation share on its own and warns at 80 percent', async () => {
    readoutOn()
    quotaFetch(quota)
    const view = meter(undefined)
    await vi.waitFor(() => expect(view.textContent).toMatch(/85 ?% (brukt|used)/))
    expect(view.textContent).toMatch(/Organisasjonens kvote|Organisation allowance/)
    const warning = view.querySelector('[data-allowance-warning]')
    expect(warning?.getAttribute('data-allowance-warning')).toBe('near')
    expect(warning?.getAttribute('aria-live')).toBe('polite')
    expect(warning?.textContent?.trim()).not.toBe('')
  })

  it('says the allowance is used up once it is exceeded', async () => {
    readoutOn()
    quotaFetch({ ...quota, used: 1100, remaining: 0, is_exceeded: true, utilization: 1 })
    const view = meter(undefined)
    await vi.waitFor(() => expect(view.textContent).toMatch(/100 ?% (brukt|used)/))
    expect(view.querySelector('[data-allowance-warning]')?.getAttribute('data-allowance-warning')).toBe('exceeded')
  })

  it('stays silent below 80 percent', async () => {
    readoutOn()
    quotaFetch({ ...quota, used: 300, remaining: 700, utilization: 0.3 })
    const view = meter(undefined)
    await vi.waitFor(() => expect(view.textContent).toMatch(/30 ?% (brukt|used)/))
    const warning = view.querySelector('[data-allowance-warning]')
    expect(warning?.getAttribute('data-allowance-warning')).toBe('')
    expect(warning?.textContent).toBe('')
  })

  it('shows no line when the read is refused', async () => {
    readoutOn()
    const fetchMock = quotaFetch({ error: { code: 'forbidden', message: 'Forbidden' } }, 403)
    const view = meter(undefined)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    await settle()
    expect(view.querySelector('.verevon-run-cost-meter__allowance')).toBeNull()
    expect(view.textContent).toBe('')
  })
})
