import { describe, expect, it } from 'vitest'
import { getActionDescriptor } from '@/shared/actions/action-registry'
import { buildActionCatalogManifest } from '@/shared/actions/catalog-manifest'
import type { BrowserChallenge } from '@/shared/api/browser-client'
import { BLOCK_REASON_COPY, ERROR_COPY } from '@/shared/i18n/errors'
import { blockedSentence, blockedStateFor, leftWithRunControlsDisabled, runControlsDisabledNotice } from './browser-blocked'
import { attachBrowserSession, browserSessionFromPreview } from './browser-session'
import type { ScrapePreview } from './knowledge-preview'

const no = (noText: string) => noText
const en = (_noText: string, enText: string) => enText

// Quarry's ChallengeSignal as it is serialized (strength, block_reason and
// recovery are always present on a current Quarry; `evidence` carries marker names).
const strong = (fields: Partial<BrowserChallenge> = {}): BrowserChallenge => ({
  block_reason: 'captcha_widget',
  confidence: 0.92,
  kind: 'captcha',
  recovery: 'available',
  requires_escalation: true,
  strength: 'strong',
  ...fields,
})

describe('blockedStateFor', () => {
  it('reads a strong signal as a blocked page, with the reason Quarry gave', () => {
    for (const reason of Object.keys(BLOCK_REASON_COPY)) {
      expect(blockedStateFor(strong({ block_reason: reason }))).toEqual({ reason })
    }
  })

  it('still blocks on a strong signal whose reason is missing or not one it knows', () => {
    expect(blockedStateFor(strong({ block_reason: null }))).toEqual({ reason: null })
    expect(blockedStateFor(strong({ block_reason: undefined }))).toEqual({ reason: null })
    expect(blockedStateFor(strong({ block_reason: 'a_reason_added_later' }))).toEqual({ reason: null })
    // Object.prototype members are not reasons.
    for (const inherited of ['constructor', 'toString', '__proto__', 'hasOwnProperty']) {
      expect(blockedStateFor(strong({ block_reason: inherited }))).toEqual({ reason: null })
    }
  })

  // The slice's DoD: a login or consent page renders no notice. Every ordinary access
  // state is a non-signal (or at most a weak hint), whatever its kind says.
  it.each([
    ['login', 'non_signal'],
    ['consent', 'non_signal'],
    ['paywall', 'non_signal'],
    ['rate_limit', 'non_signal'],
    ['access_denied', 'non_signal'],
    ['geo_blocked', 'non_signal'],
    ['unknown', 'non_signal'],
    ['captcha', 'weak'],
    ['waf', 'weak'],
    ['login', 'weak'],
  ])('shows nothing for a %s signal that is %s', (kind, strength) => {
    expect(blockedStateFor({ kind, strength, requires_escalation: false })).toBeNull()
    // Even with a block reason attached by mistake, strength decides.
    expect(blockedStateFor({ kind, strength, block_reason: 'captcha_widget' })).toBeNull()
  })

  // The other half of "strength alone decides": whatever Quarry calls the kind, a strong
  // verdict is a blocked page. A regression that special-cased one kind (say, hiding a
  // strong login or consent signal) would pass every row above and fail here.
  it.each(['captcha', 'waf', 'login', 'consent', 'paywall', 'rate_limit', 'access_denied', 'geo_blocked', 'unknown'])(
    'shows the notice for a strong %s signal',
    (kind) => {
      expect(blockedStateFor(strong({ kind }))).toEqual({ reason: 'captcha_widget' })
      expect(blockedStateFor(strong({ kind, block_reason: null }))).toEqual({ reason: null })
    },
  )

  it('shows nothing for a signal with no strength, or one it does not know', () => {
    expect(blockedStateFor({ kind: 'captcha', requires_escalation: true })).toBeNull()
    expect(blockedStateFor(strong({ strength: undefined }))).toBeNull()
    expect(blockedStateFor(strong({ strength: 'STRONG' }))).toBeNull()
    expect(blockedStateFor(strong({ strength: 'very_strong' }))).toBeNull()
  })

  it('shows nothing when there is no challenge at all', () => {
    expect(blockedStateFor(null)).toBeNull()
    expect(blockedStateFor(undefined)).toBeNull()
    expect(blockedStateFor('strong' as unknown as BrowserChallenge)).toBeNull()
  })
})

describe('blockedSentence', () => {
  it('uses the copy of the reason, in both languages', () => {
    for (const [reason, copy] of Object.entries(BLOCK_REASON_COPY)) {
      expect(blockedSentence({ reason }, no)).toBe(copy.no)
      expect(blockedSentence({ reason }, en)).toBe(copy.en)
    }
  })

  it('falls back to the general site-blocked sentence without a reason', () => {
    expect(blockedSentence({ reason: null }, no)).toBe(ERROR_COPY.browser_site_blocked!.no)
    expect(blockedSentence({ reason: null }, en)).toBe(ERROR_COPY.browser_site_blocked!.en)
  })

  it('never shows the signal text: the sentence comes from the closed reason alone', () => {
    const signal = { ...strong(), evidence: ['cf-chl-bypass', 'secret.example/login'] } as BrowserChallenge
    const sentence = blockedSentence(blockedStateFor(signal)!, no)
    expect(sentence).not.toMatch(/cf-chl|secret\.example/)
    expect(sentence).not.toMatch(/\b[a-z]+(?:_[a-z]+)+\b/)
  })
})

describe('the challenge on an observation reaches the browser chrome', () => {
  const page: ScrapePreview = {
    blocks: [],
    charCount: 0,
    description: '',
    markdown: '',
    source: 'extract',
    title: 'Example',
    url: 'https://example.test/',
  }
  const withObservation = (challenge: BrowserChallenge | undefined) =>
    browserSessionFromPreview(
      attachBrowserSession(page, {
        observation: { challenge, run_id: 'run-1', step: 1, title: 'Example', url: 'https://example.test/' },
        session: {
          capabilities: ['navigate'],
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

  it('keeps the challenge on the view model the chrome reads', () => {
    expect(blockedStateFor(withObservation(strong()).observation?.challenge)).toEqual({ reason: 'captcha_widget' })
    expect(blockedStateFor(withObservation(strong({ kind: 'login', strength: 'non_signal' })).observation?.challenge)).toBeNull()
    expect(blockedStateFor(withObservation(undefined).observation?.challenge)).toBeNull()
  })
})

describe('the action contract browser.blocked.try_other_source', () => {
  const action = getActionDescriptor('browser.blocked.try_other_source')

  it('is registered before any UI uses it, as a low-risk action that needs no approval', () => {
    expect(action).toBeDefined()
    expect(action).toMatchObject({ ownerPlane: 'model', requiresApproval: false, risk: 'low' })
    expect(action?.description).toMatch(/blocked automated browsing/i)
  })

  it('takes a session and optionally the run on it, and nothing about the page', () => {
    const schema = action!.inputSchema
    expect(schema.safeParse({ sessionId: 'run-1' }).success).toBe(true)
    expect(schema.safeParse({ sessionId: 'run-1', runId: 'run-9' }).success).toBe(true)
    expect(schema.safeParse({}).success).toBe(false)
    expect(schema.safeParse({ sessionId: '' }).success).toBe(false)
    expect(schema.safeParse({ sessionId: 'run-1', runId: '' }).success).toBe(false)
    // Unknown keys are dropped by the parse, so a caller cannot smuggle page text through it.
    const parsed = schema.safeParse({ sessionId: 'run-1', url: 'https://secret.example/', text: 'page text' })
    expect(parsed.success && parsed.data).toEqual({ sessionId: 'run-1' })
  })

  it('is for the person: a model may not run it', () => {
    const entry = buildActionCatalogManifest().actions.find((candidate) => candidate.id === 'browser.blocked.try_other_source')
    expect(entry?.allowedActorTypes).toEqual(['human'])
  })
})

describe('leaving a site whose run could not be stopped (run controls off)', () => {
  it('reads runControlsDisabled only when it is a literal true on the action result', () => {
    expect(leftWithRunControlsDisabled({ result: { closed: true, runStopped: false, runControlsDisabled: true } })).toBe(true)
    expect(leftWithRunControlsDisabled({ result: { closed: true, runStopped: true } })).toBe(false)
    expect(leftWithRunControlsDisabled({ result: { closed: true, runStopped: false } })).toBe(false)
    expect(leftWithRunControlsDisabled({ result: { runControlsDisabled: 'true' } })).toBe(false)
    expect(leftWithRunControlsDisabled({ result: { runControlsDisabled: 1 } })).toBe(false)
    expect(leftWithRunControlsDisabled({ result: null })).toBe(false)
    expect(leftWithRunControlsDisabled({ result: 'closed' })).toBe(false)
    expect(leftWithRunControlsDisabled({})).toBe(false)
    expect(leftWithRunControlsDisabled(null)).toBe(false)
    expect(leftWithRunControlsDisabled(undefined)).toBe(false)
  })

  it('says in both languages that the session is closed and the run was not stopped, and names no page', () => {
    for (const text of [runControlsDisabledNotice(no), runControlsDisabledNotice(en)]) {
      expect(text).toMatch(/lukket|closed/)
      expect(text).toMatch(/stoppes|stopped/)
      expect(text).not.toMatch(/https?:|\.com|\.no\b/)
    }
    expect(runControlsDisabledNotice(no)).not.toBe(runControlsDisabledNotice(en))
  })
})
