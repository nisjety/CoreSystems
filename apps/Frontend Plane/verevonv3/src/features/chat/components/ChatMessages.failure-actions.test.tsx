// @vitest-environment jsdom

/**
 * What a failed turn lets the reader do, by the reason the server gave.
 *
 *  - A ChatGPT sign-in that must be renewed links to the subscription panel.
 *  - A usage limit keeps "Prøv igjen" disabled, with its reset time, until
 *    the reset; it is then enabled, and still never pressed by itself.
 *  - A request ChatGPT rejected offers no retry at all.
 *  - The Work panel names the reason instead of an outage.
 */

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { AnswerRegion, ErrorNotice } from './ChatMessages'
import { formatResetTime } from './chat-normalizers'
import { StepsPanel } from './ChatPanels'

const NOW = new Date('2026-09-25T12:00:00Z')
const IN_30_MIN = new Date(NOW.getTime() + 30 * 60_000).toISOString()

const button = (container: HTMLElement) => container.querySelector<HTMLButtonElement>('.verevon-chat-error-notice button')

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW)
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('ErrorNotice actions', () => {
  it('links a sign-in that must be renewed to the ChatGPT subscription panel, beside the retry', () => {
    const onRetry = vi.fn()
    const { container } = render(() => (
      <AnswerRegion
        answer={{ state: 'failed', message: 'ChatGPT-tilkoblingen må fornyes.', action: 'reconnect' }}
        onRegenerate={onRetry}
      />
    ))
    flush()
    const link = container.querySelector<HTMLAnchorElement>('a.verevon-chat-error-notice__reconnect')
    // The reader's own Profile page, which every member can open. Settings →
    // Integrations is admin-only and would send a member to the dashboard.
    expect(link?.getAttribute('href')).toBe('/account#chatgpt-subscription')
    expect(link?.textContent).toBe('Koble til ChatGPT på nytt')
    expect(button(container)?.disabled).toBe(false)
    expect(button(container)?.textContent).toBe('Prøv igjen')
    fireEvent.click(button(container)!)
    expect(onRetry).toHaveBeenCalledTimes(1)
  })

  it('keeps a usage-limit retry disabled until the reset, then enables it without pressing it', async () => {
    const onRetry = vi.fn()
    const { container } = render(() => (
      <ErrorNotice message="Bruksgrensen er nådd." action="wait" retryAt={IN_30_MIN} onRetry={onRetry} />
    ))
    flush()
    expect(button(container)?.disabled).toBe(true)
    expect(button(container)?.textContent).toMatch(/^Kan prøves igjen (i dag|i morgen) kl\. \d{2}:\d{2}$/)
    fireEvent.click(button(container)!)
    expect(onRetry).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(29 * 60_000)
    flush()
    expect(button(container)?.disabled).toBe(true)

    await vi.advanceTimersByTimeAsync(60_000)
    flush()
    expect(button(container)?.disabled).toBe(false)
    expect(button(container)?.textContent).toBe('Prøv igjen')
    // Enabled is all the reset does: nothing was sent by itself.
    expect(onRetry).not.toHaveBeenCalled()
    fireEvent.click(button(container)!)
    expect(onRetry).toHaveBeenCalledTimes(1)
  })

  it('names the reset to the next whole minute, opens at the exact second, and says "i dag" once midnight has passed', async () => {
    // Local clock times: the notice speaks the viewer's own zone.
    const evening = new Date(2026, 8, 25, 23, 0, 0)
    const reset = new Date(2026, 8, 26, 1, 21, 40)
    vi.setSystemTime(evening)
    const onRetry = vi.fn()
    const written = `Den tilbakestilles ${formatResetTime(reset.toISOString(), 'no', evening.getTime())}.`
    expect(written).toBe('Den tilbakestilles i morgen kl. 01:22.')
    const { container } = render(() => (
      <ErrorNotice message={written} action="wait" retryAt={reset.toISOString()} onRetry={onRetry} />
    ))
    flush()
    const text = () => container.querySelector('.verevon-chat-error-notice p')?.textContent
    expect(text()).toContain('Den tilbakestilles i morgen kl. 01:22.')
    expect(button(container)?.textContent).toBe('Kan prøves igjen i morgen kl. 01:22')
    expect(vi.getTimerCount()).toBe(1)

    // Past midnight the same reset is today, in the message and on the control.
    await vi.advanceTimersByTimeAsync(60 * 60_000 + 1_000)
    flush()
    expect(text()).toContain('Den tilbakestilles i dag kl. 01:22.')
    expect(button(container)?.textContent).toBe('Kan prøves igjen i dag kl. 01:22')
    expect(button(container)?.disabled).toBe(true)

    // Enabled at 01:21:40, the true reset, not at the rounded minute shown.
    await vi.advanceTimersByTimeAsync(reset.getTime() - Date.now() - 1_000)
    flush()
    expect(button(container)?.disabled).toBe(true)
    await vi.advanceTimersByTimeAsync(1_000)
    flush()
    expect(button(container)?.disabled).toBe(false)
    expect(text()).toContain('Den tilbakestilles i dag kl. 01:22.')
    expect(onRetry).not.toHaveBeenCalled()
  })

  it('never enables a wait early when the reset is beyond the longest timer delay', async () => {
    const { container } = render(() => (
      <ErrorNotice message="Vent." action="wait" retryAt={new Date(NOW.getTime() + 30 * 86_400_000).toISOString()} onRetry={() => {}} />
    ))
    flush()
    await vi.advanceTimersByTimeAsync(2 ** 31)
    flush()
    expect(button(container)?.disabled).toBe(true)
    await vi.advanceTimersByTimeAsync(30 * 86_400_000 - 2 ** 31)
    flush()
    expect(button(container)?.disabled).toBe(false)
  })

  it('clears its timer when the notice goes away', () => {
    const { unmount } = render(() => (
      <ErrorNotice message="Vent." action="wait" retryAt={IN_30_MIN} onRetry={() => {}} />
    ))
    flush()
    expect(vi.getTimerCount()).toBe(1)
    unmount()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('treats a wait whose reset has passed, or has no instant, as an ordinary retry', () => {
    const passed = render(() => (
      <ErrorNotice message="x" action="wait" retryAt={new Date(NOW.getTime() - 1_000).toISOString()} onRetry={() => {}} />
    ))
    flush()
    expect(button(passed.container)?.disabled).toBe(false)
    const missing = render(() => <ErrorNotice message="x" action="wait" onRetry={() => {}} />)
    flush()
    expect(button(missing.container)?.disabled).toBe(false)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('offers no retry when sending the same turn again cannot help', () => {
    const { container } = render(() => (
      <AnswerRegion answer={{ state: 'failed', message: 'ChatGPT avviste forespørselen.', action: 'none' }} onRegenerate={() => {}} />
    ))
    flush()
    expect(container.querySelector('.verevon-chat-error-notice')?.textContent).toContain('ChatGPT avviste forespørselen.')
    expect(button(container)).toBeNull()
    expect(container.querySelector('a')).toBeNull()
  })

  it('offers a plain retry when no action is given', () => {
    const { container } = render(() => <ErrorNotice message="x" onRetry={() => {}} />)
    flush()
    expect(button(container)?.textContent).toBe('Prøv igjen')
    expect(container.querySelector('a')).toBeNull()
  })

  it('speaks the reader\'s language', () => {
    window.localStorage.setItem(localeStorageKey, 'en')
    const { container } = render(() => (
      <I18nProvider>
        <ErrorNotice message="x" action="reconnect" onRetry={() => {}} />
        <ErrorNotice message="y" action="wait" retryAt={IN_30_MIN} onRetry={() => {}} />
      </I18nProvider>
    ))
    flush()
    const [reconnect, wait] = [...container.querySelectorAll<HTMLElement>('.verevon-chat-error-notice')]
    expect(reconnect?.querySelector('a')?.textContent).toBe('Reconnect ChatGPT')
    expect(reconnect?.querySelector('button')?.textContent).toBe('Try again')
    expect(wait?.querySelector('button')?.textContent).toMatch(/^Available again (today|tomorrow) at \d{1,2}:\d{2}\s[AP]M$/)
  })
})

describe('StepsPanel after a ChatGPT-subscription refusal', () => {
  it('names the reason the service step gives, not an outage', () => {
    const { container } = render(() => (
      <StepsPanel
        turnStatus="error"
        steps={[
          { id: 'a1:service-unavailable', title: 'ChatGPT-planen har nådd bruksgrensen', detail: 'Den tilbakestilles i morgen kl. 03:21.', status: 'stopped', createdAt: '2026-09-25T12:00:00Z', turnId: 'a1' },
        ]}
        onStopTask={() => {}}
      />
    ))
    flush()
    expect(container.querySelector('.verevon-chat-steps-header p')?.textContent)
      .toBe('ChatGPT-planen har nådd bruksgrensen. Ingenting ble publisert.')
  })
})
