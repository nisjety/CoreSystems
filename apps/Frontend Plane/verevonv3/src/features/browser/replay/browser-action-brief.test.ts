import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  BROWSER_ACTION_BRIEF_REASON_MAX_CHARS,
  browserActionBriefEnabled,
  browserActionGate,
  browserActionHost,
  browserActionLines,
  browserActionPill,
  browserActionRowId,
  browserActionTitle,
  emptyBrowserActionFacts,
  withBrowserApprovalRequired,
  withBrowserDecided,
  withBrowserDispatched,
  withBrowserObserved,
  type BrowserActionBriefTr,
} from './browser-action-brief'

afterEach(() => {
  vi.unstubAllEnvs()
})

// The English side of the copy, so the expectations read as the words a reader sees.
const tr: BrowserActionBriefTr = (_noText, enText) => enText

describe('browserActionBriefEnabled', () => {
  it('is off by default and only the literal true turns it on', () => {
    expect(browserActionBriefEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_ACTION_BRIEF', '1')
    expect(browserActionBriefEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_ACTION_BRIEF', 'false')
    expect(browserActionBriefEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_ACTION_BRIEF', 'true')
    expect(browserActionBriefEnabled()).toBe(true)
  })
})

describe('browserActionRowId', () => {
  it('keys a row on its plan and action, so an action id that another plan reuses opens its own row', () => {
    expect(browserActionRowId('plan_1', 'act_0001')).toBe('browse-plan_1:act_0001')
    expect(browserActionRowId('plan_2', 'act_0001')).not.toBe(browserActionRowId('plan_1', 'act_0001'))
    expect(browserActionRowId(undefined, 'act_0001')).toBe('browse-:act_0001')
  })
})

describe('browserActionHost', () => {
  it('returns the host without www, and never the path or the query', () => {
    expect(browserActionHost('https://www.shop.example.com/cart?token=abc')).toBe('shop.example.com')
    expect(browserActionHost('https://example.org')).toBe('example.org')
  })

  it('returns nothing for a missing or unusable address', () => {
    expect(browserActionHost(undefined)).toBe('')
    expect(browserActionHost('   ')).toBe('')
    expect(browserActionHost('shop.example.com/cart')).toBe('')
  })
})

describe('merging the events of one action', () => {
  it('collects the gate, the decision, the dispatch and the observation on one set of facts', () => {
    let facts = emptyBrowserActionFacts('act_0003')
    facts = withBrowserApprovalRequired(facts, { riskCategory: 'checkout' })
    facts = withBrowserDecided(facts, { decision: 'granted' })
    facts = withBrowserDispatched(facts, { actionType: 'click', url: 'https://shop.example.com/pay', reason: 'Pay now' })
    facts = withBrowserObserved(facts, {
      status: 'success',
      pageUrl: 'https://shop.example.com/done',
      pageTitle: 'Thank you',
      screenshotRef: 'art_shot_1',
    })
    expect(facts).toEqual({
      actionId: 'act_0003',
      actionType: 'click',
      url: 'https://shop.example.com/pay',
      reason: 'Pay now',
      gated: true,
      riskCategory: 'checkout',
      decision: 'granted',
      observed: true,
      outcome: 'success',
      pageUrl: 'https://shop.example.com/done',
      pageTitle: 'Thank you',
      screenshotRef: 'art_shot_1',
    })
  })

  it('keeps what an earlier event said when a later event leaves a field out', () => {
    let facts = withBrowserDispatched(emptyBrowserActionFacts('act_0001'), {
      actionType: 'goto',
      url: 'https://shop.example.com/',
    })
    facts = withBrowserObserved(facts, { status: 'success' })
    expect(facts.actionType).toBe('goto')
    expect(facts.url).toBe('https://shop.example.com/')
    expect(facts.observed).toBe(true)
    expect(facts.pageUrl).toBe('')
  })

  it('leaves the gate pending when the decision is not one a gate ends with', () => {
    let facts = withBrowserApprovalRequired(emptyBrowserActionFacts('act_0002'), { riskCategory: 'login' })
    facts = withBrowserDecided(facts, { decision: 'redirected' })
    expect(browserActionGate(facts)).toBe('pending')
  })

  it('starts a new gate pending, whatever the earlier one decided', () => {
    let facts = withBrowserApprovalRequired(emptyBrowserActionFacts('act_0002'), { riskCategory: 'login' })
    facts = withBrowserDecided(facts, { decision: 'denied' })
    facts = withBrowserApprovalRequired(facts, { riskCategory: 'login' })
    expect(browserActionGate(facts)).toBe('pending')
  })
})

describe('browserActionGate', () => {
  it('is none for an action that raised no gate, pending until a decision, then that decision', () => {
    const plain = withBrowserDispatched(emptyBrowserActionFacts('act_0001'), { actionType: 'click' })
    expect(browserActionGate(plain)).toBe('none')
    const gated = withBrowserApprovalRequired(plain, {})
    expect(browserActionGate(gated)).toBe('pending')
    expect(browserActionGate(withBrowserDecided(gated, { decision: 'timed_out' }))).toBe('timed_out')
  })
})

describe('browserActionTitle', () => {
  it('reads as the action, then the host it went to', () => {
    const facts = withBrowserDispatched(emptyBrowserActionFacts('act_0001'), {
      actionType: 'click',
      url: 'https://www.shop.example.com/cart',
    })
    expect(browserActionTitle(facts, tr)).toBe('Click · shop.example.com')
  })

  it('falls back to the page host for an observation with no dispatch, and to the plain word with no host', () => {
    const seen = withBrowserObserved(emptyBrowserActionFacts('act_0001'), { pageUrl: 'https://example.org/page' })
    expect(browserActionTitle(seen, tr)).toBe('Browser · example.org')
    expect(browserActionTitle(emptyBrowserActionFacts('act_0001'), tr)).toBe('Browser')
  })

  it('names an action type it has no words for as a browser action', () => {
    const facts = withBrowserDispatched(emptyBrowserActionFacts('act_0001'), {
      actionType: 'drag_drop',
      url: 'https://example.org',
    })
    expect(browserActionTitle(facts, tr)).toBe('Browser action · example.org')
  })
})

describe('browserActionPill', () => {
  it('shows the gate while it waits or after a refusal, and the approval once it is granted', () => {
    const waiting = withBrowserApprovalRequired(emptyBrowserActionFacts('act_0001'), { riskCategory: 'login' })
    expect(browserActionPill(waiting, tr)).toEqual({ label: 'Waiting for approval', tone: 'warn' })
    expect(browserActionPill(withBrowserDecided(waiting, { decision: 'denied' }), tr))
      .toEqual({ label: 'Denied', tone: 'error' })
    const granted = withBrowserDecided(waiting, { decision: 'granted' })
    expect(browserActionPill(granted, tr)).toEqual({ label: 'Approved', tone: 'ok' })
  })

  it('shows the observation once the action has been seen', () => {
    const granted = withBrowserDecided(
      withBrowserApprovalRequired(emptyBrowserActionFacts('act_0001'), { riskCategory: 'login' }),
      { decision: 'granted' },
    )
    expect(browserActionPill(withBrowserObserved(granted, { status: 'success' }), tr))
      .toEqual({ label: 'Succeeded', tone: 'ok' })
    const blocked = withBrowserObserved(emptyBrowserActionFacts('act_0001'), { status: 'site_blocked' })
    expect(browserActionPill(blocked, tr)).toEqual({ label: 'Blocked', tone: 'error' })
    const other = withBrowserObserved(emptyBrowserActionFacts('act_0001'), { status: 'received' })
    expect(browserActionPill(other, tr)).toEqual({ label: 'Done', tone: 'neutral' })
  })

  it('says sent for an action that has only been dispatched', () => {
    const dispatched = withBrowserDispatched(emptyBrowserActionFacts('act_0001'), { actionType: 'click' })
    expect(browserActionPill(dispatched, tr)).toEqual({ label: 'Sent', tone: 'neutral' })
  })
})

describe('browserActionLines', () => {
  it('reads the reason, then the approval, then what the page showed', () => {
    let facts = withBrowserDispatched(emptyBrowserActionFacts('act_0001'), {
      actionType: 'click',
      url: 'https://shop.example.com/login',
      reason: 'Sign in to see the order',
    })
    facts = withBrowserApprovalRequired(facts, { riskCategory: 'login' })
    facts = withBrowserDecided(facts, { decision: 'granted' })
    facts = withBrowserObserved(facts, {
      status: 'success',
      pageUrl: 'https://shop.example.com/account',
      pageTitle: 'My account',
      screenshotRef: 'art_shot_2',
    })
    expect(browserActionLines(facts, tr)).toEqual([
      'Sign in to see the order',
      'Signing in · Approved',
      'Observed: My account',
    ])
  })

  it('states a gate that names no risk, and a seen page with no screenshot and no title', () => {
    let facts = withBrowserApprovalRequired(emptyBrowserActionFacts('act_0001'), {})
    expect(browserActionLines(facts, tr)).toEqual(['Approval: Waiting for approval'])
    facts = withBrowserObserved(facts, { status: 'failed', pageUrl: 'https://www.example.org/x?y=1' })
    expect(browserActionLines(facts, tr)).toEqual([
      'Approval: Waiting for approval',
      'Observed: example.org',
      'No screenshot for this step.',
    ])
  })

  it('bounds a long reason', () => {
    const facts = withBrowserDispatched(emptyBrowserActionFacts('act_0001'), { reason: 'x'.repeat(1000) })
    const [line] = browserActionLines(facts, tr)
    expect(line?.length).toBe(BROWSER_ACTION_BRIEF_REASON_MAX_CHARS)
    expect(line?.endsWith('…')).toBe(true)
  })

  it('is empty for an action that has only been dispatched with no reason', () => {
    const facts = withBrowserDispatched(emptyBrowserActionFacts('act_0001'), { actionType: 'click' })
    expect(browserActionLines(facts, tr)).toEqual([])
  })
})
