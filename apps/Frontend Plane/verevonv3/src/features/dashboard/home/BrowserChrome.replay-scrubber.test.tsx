// @vitest-environment jsdom
import { fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { BrowserTimelineEntry } from '@/shared/api/browser-client'
import { BrowserChrome } from './BrowserChrome'
import { attachBrowserSession, browserSessionFromPreview } from './browser-session'
import type { ScrapePreview } from './knowledge-preview'

// No screenshot artifacts here: BrowserChrome's screenshot branch does not mount under jsdom
// (the same crash reproduces on the committed component), so these cases exercise the
// timeline, the scrubber and the empty-frame note only.
const page: ScrapePreview = {
  blocks: [],
  charCount: 0,
  description: '',
  markdown: '',
  source: 'extract',
  title: 'Kvittering',
  url: 'https://example.test/kvittering',
}

const timeline: BrowserTimelineEntry[] = [
  { step: 0, title: 'Forside', url: 'https://example.test/' },
  { step: 1, title: 'Kontakt', url: 'https://example.test/kontakt' },
  { step: 2, title: 'Kvittering', url: 'https://example.test/kvittering' },
]

function session() {
  return browserSessionFromPreview(
    attachBrowserSession(page, {
      observation: { run_id: 'run-1', step: 2, title: 'Kvittering', url: 'https://example.test/kvittering' },
      session: {
        capabilities: ['navigate'],
        control: { mode: 'agent_control' },
        id: 'run-1',
        profile: { scope: 'run_scoped', storage: 'isolated' },
        renderMode: 'chromium',
        status: 'live',
        tabs: [],
        timeline,
        title: 'Kvittering',
        url: 'https://example.test/kvittering',
        viewport: { width: 1280, height: 800 },
      },
    }),
  )
}

const openTimeline = (container: HTMLElement) => {
  fireEvent.click(
    container.querySelector<HTMLButtonElement>('button[aria-label="Vis eller skjul tidslinje og evidens"]') as HTMLButtonElement,
  )
  flush()
}
const scrubber = (container: HTMLElement) =>
  container.querySelector<HTMLInputElement>('.browser-replay-scrubber__slider')
const scrubberStatus = (container: HTMLElement) =>
  container.querySelector<HTMLElement>('.browser-replay-scrubber__status')?.textContent ?? ''
const timelineStep = (container: HTMLElement, step: number) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>('.knowledge-browser-timeline button'))
    .find((button) => button.textContent?.includes(`#${step}`))
const emptyFrameNote = (container: HTMLElement) =>
  container.querySelector<HTMLElement>('.knowledge-browser-empty')?.textContent ?? ''

describe('BrowserChrome replay scrubber (F-t3-050 R1)', () => {
  afterEach(() => {
    vi.unstubAllEnvs()
  })

  it('is not shown while the flag is off, and the step buttons are unchanged', () => {
    const view = render(() => <BrowserChrome session={session()} />)
    openTimeline(view.container)
    expect(view.container.querySelector('.browser-replay-scrubber')).toBeNull()
    expect(timelineStep(view.container, 1)).toBeTruthy()
  })

  it('with the flag off, a step without a screenshot shows no replay note', () => {
    const view = render(() => <BrowserChrome session={session()} />)
    openTimeline(view.container)
    fireEvent.click(timelineStep(view.container, 1) as HTMLButtonElement)
    flush()
    expect(emptyFrameNote(view.container)).not.toContain('Ingen skjermbilde for dette steget.')
  })

  it('with the flag on, the scrubber covers the recorded steps and starts on the live page', () => {
    vi.stubEnv('VITE_BROWSER_REPLAY_SCRUBBER', 'true')
    const view = render(() => <BrowserChrome session={session()} />)
    openTimeline(view.container)
    expect(scrubber(view.container)?.max).toBe('2')
    expect(scrubber(view.container)?.value).toBe('2')
    expect(scrubberStatus(view.container)).toContain('#2')
  })

  it('with the flag on, a slider pick selects that recorded step in the chrome', () => {
    vi.stubEnv('VITE_BROWSER_REPLAY_SCRUBBER', 'true')
    const view = render(() => <BrowserChrome session={session()} />)
    openTimeline(view.container)

    fireEvent.input(scrubber(view.container) as HTMLInputElement, { target: { value: '1' } })
    flush()
    expect(scrubberStatus(view.container)).toContain('#1')
    expect(timelineStep(view.container, 1)?.className).toContain('knowledge-browser-timeline__step--active')
  })

  it('with the flag on, a scrubbed step without a screenshot says so instead of showing another frame', () => {
    vi.stubEnv('VITE_BROWSER_REPLAY_SCRUBBER', 'true')
    const view = render(() => <BrowserChrome session={session()} />)
    openTimeline(view.container)

    fireEvent.input(scrubber(view.container) as HTMLInputElement, { target: { value: '1' } })
    flush()
    expect(view.container.querySelector('img')).toBeNull()
    expect(emptyFrameNote(view.container)).toContain('Ingen skjermbilde for dette steget.')
  })
})
