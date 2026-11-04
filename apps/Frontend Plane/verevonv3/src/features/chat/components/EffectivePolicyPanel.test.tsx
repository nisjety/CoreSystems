import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/api-error'
import { en } from '@/shared/i18n/en'
import { no } from '@/shared/i18n/no'
import { EffectivePolicyPanel, parseEffectivePolicy, policyEffectiveViewEnabled } from './EffectivePolicyPanel'

const { requestJson, i18nState } = vi.hoisted(() => ({
  requestJson: vi.fn(),
  i18nState: { locale: 'en' as 'en' | 'no' },
}))
vi.mock('@/shared/api/http', () => ({ requestJson }))
vi.mock('@/shared/i18n', () => ({
  useI18n: () => ({
    locale: () => i18nState.locale,
    tr: (noText: string, enText: string) => (i18nState.locale === 'no' ? noText : enText),
  }),
}))

const threadPath = '/api/v1/policy/effective?thread_id=thr_1'

afterEach(() => {
  cleanup()
  requestJson.mockReset()
  i18nState.locale = 'en'
  vi.unstubAllEnvs()
})

/** Lets a settled read reach the DOM. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

/** Opens the disclosure once it has appeared. A click is also what Enter and Space send a native button. */
async function openPanel(name = 'Effective policy') {
  const toggle = await screen.findByRole('button', { name })
  fireEvent.click(toggle)
  return toggle
}

describe('EffectivePolicyPanel', () => {
  it('is off unless the build turns the view on, and sends no request', async () => {
    expect(policyEffectiveViewEnabled()).toBe(false)
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await settle()

    expect(requestJson).not.toHaveBeenCalled()
    expect(screen.queryByRole('button')).toBeNull()
  })

  it('reads the switch strictly: only the literal true turns it on', () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'TRUE')
    expect(policyEffectiveViewEnabled()).toBe(false)

    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    expect(policyEffectiveViewEnabled()).toBe(true)
  })

  it('hides itself when the server answers 404 (its switch is off)', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockRejectedValue(new ApiError('Not found', 404, null))
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)

    await waitFor(() => expect(requestJson).toHaveBeenCalledWith(threadPath))
    await settle()
    expect(screen.queryByRole('button')).toBeNull()
  })

  it('asks for the thread or the Space it is given, by its id', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockResolvedValue({ items: [] })
    render(() => <EffectivePolicyPanel scope={{ kind: 'space', spaceId: 'spc_1' }} />)

    await openPanel()
    expect(requestJson).toHaveBeenCalledWith('/api/v1/policy/effective?space_id=spc_1')
  })

  it('names the layer that set each value, for every origin the server can send', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockResolvedValue({
      items: [
        { key: 'approval_posture', value: 'ask_first', origin: 'platform' },
        { key: 'zdr', value: true, origin: 'org', revision: 4 },
        { key: 'routing', value: 'auto', origin: 'space' },
        { key: 'budget', value: 500, origin: 'user' },
        { key: 'web_access', value: 'read_only', origin: 'request' },
      ],
    })
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await openPanel()

    expect(await screen.findByText('Set by: Platform default')).toBeTruthy()
    expect(screen.getByText('Set by: Organisation')).toBeTruthy()
    expect(screen.getByText('Set by: Space')).toBeTruthy()
    expect(screen.getByText('Set by: You')).toBeTruthy()
    expect(screen.getByText('Set by: This request')).toBeTruthy()
    expect(screen.getByText('Revision 4')).toBeTruthy()
    expect(screen.getByText('On')).toBeTruthy()
    expect(screen.getByText('500')).toBeTruthy()
    expect(screen.getByText('read_only')).toBeTruthy()
  })

  it('never names a layer the server did not send', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockResolvedValue({
      items: [{ key: 'routing', value: 'auto', origin: 'mystery' }, { key: 'budget', value: 10 }],
    })
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await openPanel()

    expect(await screen.findAllByText('Source not reported')).toHaveLength(2)
    expect(screen.queryByText(/^Set by:/)).toBeNull()
  })

  it('marks a value a higher layer fixes, in words as well as the icon', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockResolvedValue({
      items: [{ key: 'zdr', value: true, origin: 'org', locked: true }],
    })
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await openPanel()

    expect(await screen.findByText('Fixed by a higher layer')).toBeTruthy()
  })

  it('shows an unavailable setting without its value or its origin', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockResolvedValue({
      items: [{ key: 'web_access', unavailable: true, value: 'read_only', origin: 'org' }],
    })
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await openPanel()

    expect(await screen.findByText('Could not be read right now')).toBeTruthy()
    expect(screen.queryByText('read_only')).toBeNull()
    expect(screen.queryByText('Set by: Organisation')).toBeNull()
  })

  it('shows a member an admin-only detail as a count, and keeps the origin', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockResolvedValue({
      items: [{ key: 'browser_site_mode', value: null, origin: 'org', redacted: true, count: 12 }],
    })
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await openPanel()

    expect(await screen.findByText('12 entries, visible to admins only')).toBeTruthy()
    expect(screen.getByText('Set by: Organisation')).toBeTruthy()
    expect(screen.queryByText('Not set')).toBeNull()
  })

  it('opens and closes from the keyboard, and the region it controls exists only while open', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockResolvedValue({ items: [{ key: 'zdr', value: true, origin: 'org' }] })
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)

    const toggle = await screen.findByRole('button', { name: 'Effective policy' })
    expect(toggle.tagName).toBe('BUTTON')
    expect(toggle.getAttribute('aria-expanded')).toBe('false')
    expect(toggle.getAttribute('aria-controls')).toBeNull()

    fireEvent.click(toggle)
    const controlled = toggle.getAttribute('aria-controls') ?? ''
    expect(toggle.getAttribute('aria-expanded')).toBe('true')
    expect(document.getElementById(controlled)).not.toBeNull()

    fireEvent.click(toggle)
    expect(toggle.getAttribute('aria-expanded')).toBe('false')
    expect(document.getElementById(controlled)).toBeNull()
  })

  it('says so in place when the policy cannot be loaded', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockRejectedValue(new ApiError('boom', 500, null))
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await openPanel()

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe('The effective policy could not be loaded.')
  })

  it('treats a response of the wrong shape as a failure, not as an empty policy', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockResolvedValue({ items: 'nope' })
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await openPanel()

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.queryByText('No settings apply here.')).toBeNull()
  })

  it('shows the Norwegian copy in the Norwegian locale', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    i18nState.locale = 'no'
    requestJson.mockResolvedValue({
      items: [{ key: 'zdr', value: true, origin: 'org' }],
    })
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await openPanel('Gjeldende policy')

    expect(await screen.findByText('Satt av: Organisasjonen')).toBeTruthy()
    expect(screen.getByText('Null datalagring (ZDR)')).toBeTruthy()
    expect(screen.getByText('På')).toBeTruthy()
  })

  it('keeps the two locales on the same keys, so a switch never shows a gap', () => {
    expect(Object.keys(no.effectivePolicy).sort()).toEqual(Object.keys(en.effectivePolicy).sort())
    expect(Object.keys(no.effectivePolicy.keys).sort()).toEqual(Object.keys(en.effectivePolicy.keys).sort())
    expect(Object.keys(no.effectivePolicy.origin).sort()).toEqual(Object.keys(en.effectivePolicy.origin).sort())
  })

  it('drops rows it cannot read, and makes an unreadable value unavailable instead of guessing', () => {
    expect(
      parseEffectivePolicy({
        items: [{ value: true }, 'x', null, { key: '   ' }, { key: 'zdr', value: true, origin: 'org' }],
      }),
    ).toEqual([
      { key: 'zdr', value: true, origin: 'org', locked: false, redacted: false, unavailable: false },
    ])

    expect(parseEffectivePolicy({ items: [{ key: 'routing', value: { mode: 'auto' }, origin: 'org' }] })).toEqual([
      { key: 'routing', value: null, origin: null, locked: false, redacted: false, unavailable: true },
    ])
  })

  it('reads a revision and a count only when they are whole numbers', () => {
    expect(
      parseEffectivePolicy({ items: [{ key: 'budget', value: 1, revision: 1.5, count: -2, origin: 'org' }] }),
    ).toEqual([{ key: 'budget', value: 1, origin: 'org', locked: false, redacted: false, unavailable: false }])
  })

  it('throws on a response without an items list', () => {
    expect(() => parseEffectivePolicy({ items: 'nope' })).toThrow()
    expect(() => parseEffectivePolicy(null)).toThrow()
  })

  it('keeps one column at phone width and lets long values wrap', () => {
    const css = readFileSync(fileURLToPath(new URL('./effective-policy-panel.css', import.meta.url)), 'utf8')

    expect(css).toContain('overflow-wrap: anywhere')
    expect(css).toContain('min-width: 0')
    expect(css).toContain('@media (min-width: 640px)')
    expect(css).not.toMatch(/(?<!min-)width:\s*\d{3,}px/)
  })
})
