import { cleanup, render } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it } from 'vitest'
import type { Translator } from '@/shared/activity/narration'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import type { ChatToolCall, LifecycleNotice } from './chat-types'
import { StepStrip, isStepStripEnabled, stepProgress } from './StepStrip'

const NO: Translator = { tr: (noText) => noText }
const EN: Translator = { tr: (_noText, enText) => enText }

const searched: ChatToolCall = {
  id: 'call-1',
  name: 'web_search',
  args: { query: 'Aquatiq' },
  status: 'ok',
  output: JSON.stringify({ count: 3 }),
}
const reading: ChatToolCall = { id: 'call-2', name: 'web_fetch', args: { url: 'https://example.com/report' }, status: 'running' }

function setLocale(locale: 'no' | 'en') {
  window.localStorage.setItem(localeStorageKey, locale)
}

afterEach(() => {
  cleanup()
  flush()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('isStepStripEnabled', () => {
  it('enables only on the literal true', () => {
    expect(isStepStripEnabled('true')).toBe(true)
    expect(isStepStripEnabled('True')).toBe(false)
    expect(isStepStripEnabled('1')).toBe(false)
    expect(isStepStripEnabled(' true')).toBe(false)
    expect(isStepStripEnabled(undefined)).toBe(false)
  })
})

describe('stepProgress', () => {
  it('counts the settled calls and the one out, against the plan', () => {
    const progress = stepProgress([searched, reading], 5, undefined, EN)
    expect(progress).toEqual({ current: 2, total: 5, sentence: 'Reading example.com' })
  })

  it('leaves the total out when the turn has no plan', () => {
    expect(stepProgress([searched, reading], undefined, undefined, EN)?.total).toBeUndefined()
  })

  it('never shows a total the calls have already passed', () => {
    const progress = stepProgress([searched, { ...searched, id: 'call-3' }], 1, undefined, EN)
    expect(progress?.current).toBe(2)
    expect(progress?.total).toBeUndefined()
  })

  it('counts a failed call as a settled step', () => {
    const failed: ChatToolCall = { id: 'call-0', name: 'web_search', args: { query: 'x' }, status: 'failed', error: 'timeout' }
    expect(stepProgress([failed, reading], undefined, undefined, EN)?.current).toBe(2)
  })

  it('shows the last call that settled once nothing is out', () => {
    expect(stepProgress([searched], 3, undefined, EN)).toEqual({
      current: 1,
      total: 3,
      sentence: "Searched for 'Aquatiq' - 3 results",
    })
  })

  it('is absent before the first call', () => {
    expect(stepProgress([], 5, undefined, EN)).toBeUndefined()
  })

  it('reads the Norwegian sentence under no', () => {
    expect(stepProgress([reading], 2, undefined, NO)).toEqual({ current: 1, total: 2, sentence: 'Leser example.com' })
  })
})

describe('StepStrip', () => {
  it('reads Step N of M with the plan, and the sentence for the step on', () => {
    setLocale('en')
    const view = render(() => (
      <I18nProvider>
        <StepStrip calls={[searched, reading]} planSteps={5} />
      </I18nProvider>
    ))
    flush()
    expect(view.container.querySelector('.verevon-chat-step-strip')?.textContent).toBe('Step 2 of 5 - Reading example.com')
  })

  it('reads Step N without a total when the turn has no plan', () => {
    setLocale('en')
    const view = render(() => (
      <I18nProvider>
        <StepStrip calls={[searched, reading]} />
      </I18nProvider>
    ))
    flush()
    expect(view.container.querySelector('.verevon-chat-step-strip')?.textContent).toBe('Step 2 - Reading example.com')
  })

  it('stays empty before the first call, with its live region in place', () => {
    setLocale('en')
    const view = render(() => (
      <I18nProvider>
        <StepStrip calls={[]} planSteps={5} />
      </I18nProvider>
    ))
    flush()
    const region = view.container.querySelector('.verevon-chat-step-strip')
    expect(region?.getAttribute('aria-live')).toBe('polite')
    expect(region?.textContent).toBe('')
  })

  it('updates the same line in place as a step settles, and says the retry in place of the call', () => {
    setLocale('en')
    const [calls, setCalls] = createSignal<ChatToolCall[]>([reading])
    const [notices, setNotices] = createSignal<LifecycleNotice[]>([])
    const view = render(() => (
      <I18nProvider>
        <StepStrip calls={calls()} notices={notices()} />
      </I18nProvider>
    ))
    flush()
    const region = view.container.querySelector('.verevon-chat-step-strip')
    expect(region?.textContent).toBe('Step 1 - Reading example.com')

    setNotices([{ kind: 'retry', attempt: 2, maxAttempts: 3, reason: 'timeout', callId: 'call-2' }])
    flush()
    expect(region?.textContent).toBe('Step 1 - Retrying reading example.com (attempt 2 of 3)')

    setCalls([{ ...reading, status: 'ok', output: JSON.stringify({ truncated: true }) }])
    flush()
    expect(view.container.querySelector('.verevon-chat-step-strip')).toBe(region)
    expect(region?.textContent).toBe('Step 1 - Read example.com - truncated')
  })

  it('takes no focus from the control that has it', () => {
    setLocale('en')
    const button = document.createElement('button')
    document.body.appendChild(button)
    button.focus()
    try {
      render(() => (
        <I18nProvider>
          <StepStrip calls={[reading]} />
        </I18nProvider>
      ))
      flush()
      expect(document.activeElement).toBe(button)
    } finally {
      button.remove()
    }
  })
})
