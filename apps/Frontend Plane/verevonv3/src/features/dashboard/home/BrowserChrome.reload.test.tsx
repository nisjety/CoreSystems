// @vitest-environment jsdom
import { fireEvent, render } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import { BrowserChrome } from './BrowserChrome'
import { attachBrowserSession, browserSessionFromPreview } from './browser-session'
import type { ScrapePreview } from './knowledge-preview'
import type { BrowserNavigationFailure } from '@/features/browser/session/browser-navigation-error'

const page: ScrapePreview = {
  blocks: [],
  charCount: 0,
  description: '',
  markdown: '',
  source: 'extract',
  title: 'Example',
  url: 'https://example.test/',
}

function session(capabilities: string[]) {
  return browserSessionFromPreview(
    attachBrowserSession(page, {
      observation: { run_id: 'run-1', step: 1, title: 'Example', url: 'https://example.test/' },
      session: {
        capabilities,
        control: { mode: 'agent_control' },
        id: 'run-1',
        profile: { scope: 'run_scoped', storage: 'isolated' },
        renderMode: 'chromium',
        status: 'live',
        tabs: [],
        title: 'Example',
        url: 'https://example.test/',
        viewport: { width: 1280, height: 800 },
      },
    }),
  )
}

const failure: BrowserNavigationFailure = {
  message: 'Adressen finnes ikke.',
  reason: 'unreachable',
  url: 'https://this-host-does-not-resolve.invalid/',
}

const softReload = (container: HTMLElement) =>
  container.querySelector<HTMLButtonElement>('button[aria-label="Last nettlesersiden på nytt"]')
const hardReload = (container: HTMLElement) =>
  container.querySelector<HTMLButtonElement>('button[aria-label="Last nettlesersiden på nytt uten hurtigbuffer"]')
const chrome = (container: HTMLElement) => container.querySelector<HTMLElement>('.knowledge-browser-chrome')

describe('BrowserChrome reload and navigation error (WS9.B)', () => {
  it('shows the error card with the failed URL and retries that URL on Prøv igjen', () => {
    const onRetry = vi.fn()
    const view = render(() => (
      <BrowserChrome
        session={session(['navigate'])}
        browserNavigationError={failure}
        onBrowserNavigationRetry={onRetry}
      />
    ))
    const card = view.container.querySelector<HTMLElement>('.browser-navigation-error')
    expect(card?.textContent).toContain('Adressen finnes ikke.')
    expect(card?.textContent).toContain(failure.url)
    const retry = Array.from(card?.querySelectorAll('button') ?? []).find((b) => b.textContent?.includes('Prøv igjen'))
    fireEvent.click(retry as HTMLButtonElement)
    expect(onRetry).toHaveBeenCalledWith(failure.url)
  })

  it('a soft reload after a failed navigation retries the failed URL, not the session URL', () => {
    const onRetry = vi.fn()
    const onAction = vi.fn()
    const view = render(() => (
      <BrowserChrome
        session={session(['navigate'])}
        browserNavigationError={failure}
        onBrowserNavigationRetry={onRetry}
        onBrowserAction={onAction}
      />
    ))
    fireEvent.click(softReload(view.container) as HTMLButtonElement)
    expect(onRetry).toHaveBeenCalledWith(failure.url)
    expect(onAction).not.toHaveBeenCalled()
  })

  it('a soft reload without a failure navigates to the session URL (GET, not a reload action)', () => {
    const onAction = vi.fn()
    const view = render(() => <BrowserChrome session={session(['navigate'])} onBrowserAction={onAction} />)
    fireEvent.click(softReload(view.container) as HTMLButtonElement)
    expect(onAction).toHaveBeenCalledWith({ type: 'navigate', url: 'https://example.test/' })
  })

  it('offers the hard reload only when the server advertises hard_reload', () => {
    const without = render(() => <BrowserChrome session={session(['navigate'])} />)
    expect(hardReload(without.container)).toBeNull()
    without.unmount()
    const withCapability = render(() => <BrowserChrome session={session(['navigate', 'hard_reload'])} />)
    expect(hardReload(withCapability.container)).not.toBeNull()
  })

  it('a hard reload sends reload with ignore_cache true', () => {
    const onAction = vi.fn()
    const view = render(() => (
      <BrowserChrome session={session(['navigate', 'hard_reload'])} onBrowserAction={onAction} />
    ))
    fireEvent.click(hardReload(view.container) as HTMLButtonElement)
    expect(onAction).toHaveBeenCalledWith({ type: 'reload', ignore_cache: true })
  })

  it('F5 inside the chrome performs a soft reload and prevents the browser default', () => {
    const onAction = vi.fn()
    const view = render(() => <BrowserChrome session={session(['navigate'])} onBrowserAction={onAction} />)
    const event = new KeyboardEvent('keydown', { key: 'F5', bubbles: true, cancelable: true })
    chrome(view.container)?.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(true)
    expect(onAction).toHaveBeenCalledWith({ type: 'navigate', url: 'https://example.test/' })
  })

  it('mouse buttons 4 and 5 on the page frame go back and forward', () => {
    const onAction = vi.fn()
    const view = render(() => <BrowserChrome session={session(['navigate', 'back', 'forward'])} onBrowserAction={onAction} />)
    const frame = view.container.querySelector<HTMLElement>('.knowledge-browser-chrome__page')
    frame?.dispatchEvent(new MouseEvent('auxclick', { button: 3, bubbles: true, cancelable: true }))
    expect(onAction).toHaveBeenLastCalledWith({ type: 'back' })
    frame?.dispatchEvent(new MouseEvent('auxclick', { button: 4, bubbles: true, cancelable: true }))
    expect(onAction).toHaveBeenLastCalledWith({ type: 'forward' })
  })
})
