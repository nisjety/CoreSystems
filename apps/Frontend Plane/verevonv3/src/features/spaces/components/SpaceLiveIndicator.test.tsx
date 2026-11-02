import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it } from 'vitest'

import type { SpaceStreamState } from '../lib/space-stream'
import { SpaceLiveIndicator } from './SpaceLiveIndicator'

afterEach(() => cleanup())

// No I18nProvider wraps these renders, so the copy is the Norwegian default, as in the page tests.
function renderIndicator(initial: SpaceStreamState) {
  const [state, setState] = createSignal<SpaceStreamState>(initial)
  const view = render(() => <SpaceLiveIndicator state={state} />)
  const set = (next: SpaceStreamState) => {
    setState(next)
    flush()
  }
  return { ...view, set }
}

const indicator = () => document.querySelector('.verevon-space-live')
const region = () => screen.getByRole('status')

describe('SpaceLiveIndicator (WS02-S5)', () => {
  it('says Live, quietly, for a live stream', () => {
    renderIndicator('live')
    expect(indicator()?.textContent).toBe('Live')
    expect(indicator()?.getAttribute('data-state')).toBe('live')
    // The dot is decoration, not content.
    expect(indicator()?.querySelector('.verevon-space-live__dot')?.getAttribute('aria-hidden')).toBe('true')
  })

  it('says plainly that the room refreshes on a timer when degraded', () => {
    renderIndicator('degraded')
    expect(indicator()?.textContent).toBe('Oppdateres hvert par sekunder')
    expect(indicator()?.getAttribute('data-state')).toBe('degraded')
  })

  it('renders nothing for a state that is not a claim about the connection', () => {
    for (const state of ['idle', 'connecting', 'unavailable'] as const) {
      const view = renderIndicator(state)
      expect(indicator(), state).toBeNull()
      view.unmount()
    }
  })

  it('always draws a polite, atomic region, empty in the steady state', () => {
    renderIndicator('live')
    expect(region().getAttribute('aria-live')).toBe('polite')
    expect(region().getAttribute('aria-atomic')).toBe('true')
    expect(region().textContent).toBe('')
  })

  it('does not announce a start: idle to live and connecting to live are silent', async () => {
    const view = renderIndicator('idle')
    view.set('connecting')
    view.set('live')
    expect(region().textContent).toBe('')
    expect(indicator()?.textContent).toBe('Live')
  })

  it('announces live to degraded', async () => {
    const view = renderIndicator('idle')
    view.set('live')
    view.set('degraded')
    await waitFor(() => expect(region().textContent).toMatch(/Live-oppdateringer er satt på pause/))
  })

  it('announces degraded to live', async () => {
    const view = renderIndicator('idle')
    view.set('live')
    view.set('degraded')
    view.set('live')
    await waitFor(() => expect(region().textContent).toBe('Live-oppdateringer er tilbake.'))
  })

  it('announces nothing for any other transition, and clears an earlier announcement', async () => {
    const view = renderIndicator('idle')
    view.set('live')
    view.set('degraded')
    await waitFor(() => expect(region().textContent).not.toBe(''))
    // degraded -> connecting is not live <-> degraded: the announcement is cleared.
    view.set('connecting')
    await waitFor(() => expect(region().textContent).toBe(''))
    view.set('unavailable')
    view.set('idle')
    expect(region().textContent).toBe('')
  })

  it('does not announce degraded when it was never live (a stream that never connected)', async () => {
    const view = renderIndicator('connecting')
    view.set('degraded')
    expect(region().textContent).toBe('')
    expect(indicator()?.getAttribute('data-state')).toBe('degraded')
  })
})
