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
        { key: 'approval.posture', value: 'ask_first', origin: 'platform' },
        { key: 'zdr', value: true, origin: 'org', revision: 4 },
        { key: 'web.mode', value: 'live', origin: 'space' },
        { key: 'budget.max_rounds', value: 500, origin: 'user' },
        { key: 'mode_cap', value: 'read_only', origin: 'request' },
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
      items: [{ key: 'web.mode', value: 'live', origin: 'mystery' }, { key: 'budget.max_rounds', value: 10 }],
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
      items: [{ key: 'mode_cap', unavailable: true, value: 'read_only', origin: 'org' }],
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
      items: [{ key: 'browser_sites.rules', value: null, origin: 'org', redacted: true, count: 12 }],
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

    expect(
      parseEffectivePolicy({ items: [{ key: 'model_access.max_effort', value: { mode: 'auto' }, origin: 'org' }] }),
    ).toEqual([
      { key: 'model_access.max_effort', value: null, origin: null, locked: false, redacted: false, unavailable: true },
    ])
  })

  it('reads a revision and a count only when they are whole numbers', () => {
    expect(
      parseEffectivePolicy({
        items: [{ key: 'budget.max_rounds', value: 1, revision: 1.5, count: -2, origin: 'org' }],
      }),
    ).toEqual([
      { key: 'budget.max_rounds', value: 1, origin: 'org', locked: false, redacted: false, unavailable: false },
    ])
  })

  it('throws on a response without an items list', () => {
    expect(() => parseEffectivePolicy({ items: 'nope' })).toThrow()
    expect(() => parseEffectivePolicy(null)).toThrow()
  })

  it('reads an admin list as its entries and a withheld list as its count, and no list in any other shape', () => {
    expect(
      parseEffectivePolicy({
        items: [
          { key: 'model_access.allow', value: null, origin: 'org', values: ['model-a', 'model-b'] },
          { key: 'model_access.deny', value: null, origin: 'org', redacted: true, count: 1 },
        ],
      }),
    ).toEqual([
      {
        key: 'model_access.allow',
        value: null,
        origin: 'org',
        locked: false,
        redacted: false,
        values: ['model-a', 'model-b'],
        unavailable: false,
      },
      {
        key: 'model_access.deny',
        value: null,
        origin: 'org',
        locked: false,
        redacted: true,
        count: 1,
        unavailable: false,
      },
    ])

    // A list that is not text, blank, or over the bounds makes its row unavailable rather than a list.
    const tooLong = Array.from({ length: 201 }, (_, n) => `model-${n}`)
    for (const values of [['model-a', 3], [' '], ['x'.repeat(201)], tooLong, 'model-a']) {
      expect(parseEffectivePolicy({ items: [{ key: 'model_access.allow', values, origin: 'org' }] })).toEqual([
        { key: 'model_access.allow', value: null, origin: null, locked: false, redacted: false, unavailable: true },
      ])
    }

    // A withheld row carries neither a value nor a list.
    for (const extra of [{ value: 'model-a' }, { values: ['model-a'] }]) {
      const [row] = parseEffectivePolicy({
        items: [{ key: 'model_access.allow', origin: 'org', redacted: true, count: 1, ...extra }],
      })
      expect(row?.unavailable).toBe(true)
      expect(row?.values).toBeUndefined()
    }
  })

  it('shows a member the count of a list and an admin the list itself, and "Not set" for an empty one', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockResolvedValue({
      items: [
        { key: 'model_access.allow', value: null, origin: 'org', redacted: true, count: 2 },
        {
          key: 'browser_sites.rules',
          value: null,
          origin: 'org',
          values: ['example.com (read)', 'shop.example.com (act)'],
        },
        { key: 'model_access.deny', value: null, origin: 'platform', values: [] },
      ],
    })
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await openPanel()

    expect(await screen.findByText('2 entries, visible to admins only')).toBeTruthy()
    expect(screen.getByText('example.com (read), shop.example.com (act)')).toBeTruthy()
    expect(screen.getByText('Not set')).toBeTruthy()
  })

  it('labels a dotted key from the copy and humanises a key the copy does not name', async () => {
    vi.stubEnv('VITE_POLICY_EFFECTIVE_VIEW_ENABLED', 'true')
    requestJson.mockResolvedValue({
      items: [
        { key: 'approval.posture', value: 'auto', origin: 'platform' },
        { key: 'routing.fallback_model', value: 'auto', origin: 'org' },
      ],
    })
    render(() => <EffectivePolicyPanel scope={{ kind: 'thread', threadId: 'thr_1' }} />)
    await openPanel()

    expect(await screen.findByText('Approval posture')).toBeTruthy()
    expect(screen.getByText('Routing fallback model')).toBeTruthy()
    expect(screen.queryByText('routing.fallback_model')).toBeNull()
  })

  it('names every key the servers send, in both locales', () => {
    // The keys model-gateway's view lists, the reserved and enforced settings it lists at their default, the BFF's
    // model-access and browser-site items, and the stand-ins an unanswered owner leaves in place.
    const sent = [
      'approval.posture',
      'budget.max_rounds',
      'budget.max_subagents',
      'mode_cap',
      'rung_cap',
      'web.mode',
      'web.paid_providers',
      'research.max_fan_out_items',
      'ext.skills',
      'ext.plugins',
      'user_hooks',
      'reference_uploads',
      'mcp_apps.network_origins',
      'zdr',
      'model_access.allow_mode',
      'model_access.min_privacy_tier',
      'model_access.max_effort',
      'model_access.allow',
      'model_access.deny',
      'browser_sites.mode',
      'browser_sites.rules',
      'model_plane',
      'model_access',
      'browser_sites',
    ]
    for (const key of sent) {
      expect(Object.hasOwn(en.effectivePolicy.keys, key), `en ${key}`).toBe(true)
      expect(Object.hasOwn(no.effectivePolicy.keys, key), `no ${key}`).toBe(true)
    }
  })

  it('keeps one column at phone width and lets long values wrap', () => {
    const css = readFileSync(fileURLToPath(new URL('./effective-policy-panel.css', import.meta.url)), 'utf8')

    expect(css).toContain('overflow-wrap: anywhere')
    expect(css).toContain('min-width: 0')
    expect(css).toContain('@media (min-width: 640px)')
    expect(css).not.toMatch(/(?<!min-)width:\s*\d{3,}px/)
  })
})
