// @vitest-environment jsdom
import { fireEvent, render } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import type { BrowserChallenge } from '@/shared/api/browser-client'
import { BLOCK_REASON_COPY, ERROR_COPY } from '@/shared/i18n/errors'
import { BrowserChrome } from './BrowserChrome'
import { attachBrowserSession, browserSessionFromPreview } from './browser-session'
import type { ScrapePreview } from './knowledge-preview'

const page: ScrapePreview = {
  blocks: [],
  charCount: 0,
  description: '',
  markdown: '',
  source: 'extract',
  title: 'Example',
  url: 'https://example.test/',
}

const strong: BrowserChallenge = {
  block_reason: 'waf_interstitial',
  confidence: 0.9,
  kind: 'waf',
  recovery: 'available',
  requires_escalation: true,
  strength: 'strong',
}

function session(options: { challenge?: BrowserChallenge; mode?: 'agent_control' | 'human_takeover'; renderMode?: 'chromium' | 'readability_fallback' }) {
  return browserSessionFromPreview(
    attachBrowserSession(page, {
      observation: { challenge: options.challenge, run_id: 'run-1', step: 1, title: 'Example', url: 'https://example.test/' },
      session: {
        capabilities: ['navigate'],
        control: { mode: options.mode ?? 'agent_control' },
        id: 'run-1',
        profile: { scope: 'run_scoped', storage: 'isolated' },
        renderMode: options.renderMode ?? 'chromium',
        status: 'live',
        tabs: [],
        title: 'Example',
        url: 'https://example.test/',
        viewport: { width: 1280, height: 800 },
      },
    }),
  )
}

const notice = (container: HTMLElement) => container.querySelector<HTMLElement>('.knowledge-browser-blocked')
const button = (container: HTMLElement, label: RegExp) =>
  [...container.querySelectorAll<HTMLButtonElement>('.knowledge-browser-blocked__button')].find((candidate) => label.test(candidate.textContent ?? ''))

describe('BrowserBlockedNotice in the browser chrome', () => {
  it('renders for a strong signal, with the sentence for its reason', () => {
    const view = render(() => <BrowserChrome session={session({ challenge: strong })} />)
    const shown = notice(view.container)
    expect(shown).not.toBeNull()
    expect(shown!.textContent).toContain(BLOCK_REASON_COPY.waf_interstitial!.no)
    expect(shown!.getAttribute('data-reason')).toBe('waf_interstitial')
  })

  it('uses the general sentence for a strong signal whose reason is missing', () => {
    const view = render(() => <BrowserChrome session={session({ challenge: { ...strong, block_reason: null } })} />)
    expect(notice(view.container)!.textContent).toContain(ERROR_COPY.browser_site_blocked!.no)
  })

  // The DoD: a login or consent page renders none.
  it.each([
    ['login', 'non_signal'],
    ['consent', 'non_signal'],
    ['paywall', 'non_signal'],
    ['rate_limit', 'non_signal'],
    ['captcha', 'weak'],
  ])('renders nothing for a %s page (%s)', (kind, strength) => {
    const view = render(() => <BrowserChrome session={session({ challenge: { kind, requires_escalation: false, strength } })} />)
    expect(notice(view.container)).toBeNull()
  })

  it('renders nothing without a challenge, and nothing outside a live browser', () => {
    const plain = render(() => <BrowserChrome session={session({})} />)
    expect(notice(plain.container)).toBeNull()
    const fallback = render(() => <BrowserChrome session={session({ challenge: strong, renderMode: 'readability_fallback' })} />)
    expect(notice(fallback.container)).toBeNull()
  })

  it('"Ta over" hands the browser to the person through the same control as the toolbar chip', () => {
    const onBrowserControlMode = vi.fn()
    const view = render(() => <BrowserChrome onBrowserControlMode={onBrowserControlMode} session={session({ challenge: strong })} />)

    fireEvent.click(button(view.container, /Ta over/)!)

    expect(onBrowserControlMode).toHaveBeenCalledTimes(1)
    expect(onBrowserControlMode).toHaveBeenCalledWith('human_takeover')
  })

  it('"Prøv en annen kilde" asks the owner to leave the site', () => {
    const onBrowserTryOtherSource = vi.fn()
    const view = render(() => (
      <BrowserChrome onBrowserControlMode={vi.fn()} onBrowserTryOtherSource={onBrowserTryOtherSource} session={session({ challenge: strong })} />
    ))

    fireEvent.click(button(view.container, /annen kilde/)!)

    expect(onBrowserTryOtherSource).toHaveBeenCalledTimes(1)
  })

  it('does not offer "Ta over" once the person already holds the browser, but still offers to leave', () => {
    const view = render(() => (
      <BrowserChrome
        onBrowserControlMode={vi.fn()}
        onBrowserTryOtherSource={vi.fn()}
        session={session({ challenge: strong, mode: 'human_takeover' })}
      />
    ))
    expect(notice(view.container)).not.toBeNull()
    expect(button(view.container, /Ta over/)).toBeUndefined()
    expect(button(view.container, /annen kilde/)).toBeDefined()
  })

  it('disables both buttons while the browser is busy, and without a handler for leaving', () => {
    const busy = render(() => (
      <BrowserChrome
        browserBusy
        onBrowserControlMode={vi.fn()}
        onBrowserTryOtherSource={vi.fn()}
        session={session({ challenge: strong })}
      />
    ))
    expect(button(busy.container, /Ta over/)!.disabled).toBe(true)
    expect(button(busy.container, /annen kilde/)!.disabled).toBe(true)

    const unwired = render(() => <BrowserChrome onBrowserControlMode={vi.fn()} session={session({ challenge: strong })} />)
    expect(button(unwired.container, /annen kilde/)!.disabled).toBe(true)
    expect(button(unwired.container, /Ta over/)!.disabled).toBe(false)
  })

  it('never shows the signal text', () => {
    const view = render(() => (
      <BrowserChrome
        session={session({ challenge: { ...strong, evidence: ['cf-chl-bypass', 'secret.example/login'] } as BrowserChallenge })}
      />
    ))
    expect(notice(view.container)!.textContent).not.toMatch(/cf-chl|secret\.example/)
  })
})
