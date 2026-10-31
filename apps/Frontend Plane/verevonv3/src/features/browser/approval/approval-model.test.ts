import { describe, expect, it } from 'vitest'
import type { BrowserApprovalEntry } from '@/features/dashboard/home/browser-session'
import type { Approval } from '@/shared/api/orchestration-client'
import {
  APPROVAL_REDIRECT_MAX,
  approvalRiskCopy,
  approvalTargetSummary,
  browserApprovalToCardModel,
  buildDecisionRequest,
  checkRedirectText,
  runApprovalToCardModel,
  type ApprovalCardModel,
} from './approval-model'

const model: ApprovalCardModel = {
  approvalId: 'appr_1',
  source: 'run',
  reason: 'Send the form',
  actionType: 'posting_form',
  target: '',
  riskCategory: '',
}

function browserEntry(overrides: Partial<BrowserApprovalEntry> = {}): BrowserApprovalEntry {
  return {
    key: 'run_1:act_1',
    approvalId: 'appr_b1',
    runId: 'run_1',
    planId: 'plan_1',
    actionId: 'act_1',
    actionType: 'click',
    url: 'https://shop.example.com/checkout?token=secret#frag',
    selector: '#pay',
    reason: '  Pays for the order  ',
    riskCategory: 'checkout',
    status: 'pending',
    decidedBy: null,
    ...overrides,
  } as BrowserApprovalEntry
}

describe('approvalTargetSummary', () => {
  it('keeps host and path and drops the query string and fragment', () => {
    expect(approvalTargetSummary('https://shop.example.com/checkout?token=secret#frag', '#pay')).toBe(
      'shop.example.com/checkout · #pay',
    )
  })

  it('returns non-URL input trimmed and skips empty parts', () => {
    expect(approvalTargetSummary('  not a url  ', '')).toBe('not a url')
    expect(approvalTargetSummary('', '')).toBe('')
  })
})

describe('browserApprovalToCardModel', () => {
  it('maps a pending browser gate to a card model with a trimmed reason', () => {
    expect(browserApprovalToCardModel(browserEntry())).toEqual({
      approvalId: 'appr_b1',
      source: 'browser',
      reason: 'Pays for the order',
      actionType: 'click',
      target: 'shop.example.com/checkout · #pay',
      riskCategory: 'checkout',
    })
  })

  it('returns null for a decided gate and for a gate without an approval id', () => {
    expect(browserApprovalToCardModel(browserEntry({ status: 'granted' }))).toBeNull()
    expect(browserApprovalToCardModel(browserEntry({ approvalId: '' }))).toBeNull()
  })
})

describe('runApprovalToCardModel', () => {
  it('maps a requested run approval and keeps its kind and detail', () => {
    const approval: Approval = { id: 'appr_2', status: 'PENDING', kind: 'tool', detail: ' Run the tool ' }
    expect(runApprovalToCardModel(approval)).toEqual({
      approvalId: 'appr_2',
      source: 'run',
      reason: 'Run the tool',
      actionType: 'tool',
      target: '',
      riskCategory: '',
    })
  })

  it('returns null once the approval is decided, and when it has no id', () => {
    expect(runApprovalToCardModel({ id: 'appr_3', status: 'GRANTED' })).toBeNull()
    expect(runApprovalToCardModel({ id: '', status: 'PENDING' })).toBeNull()
  })

  it('treats a missing status as pending', () => {
    expect(runApprovalToCardModel({ id: 'appr_4' })?.approvalId).toBe('appr_4')
  })
})

describe('checkRedirectText', () => {
  it('trims the text and refuses empty or whitespace-only input', () => {
    expect(checkRedirectText('  use the other form  ')).toEqual({ ok: true, text: 'use the other form' })
    expect(checkRedirectText('   ')).toEqual({ ok: false, reason: 'empty' })
  })

  it('accepts exactly the cap and refuses one character more', () => {
    expect(checkRedirectText('a'.repeat(APPROVAL_REDIRECT_MAX))).toMatchObject({ ok: true })
    expect(checkRedirectText('a'.repeat(APPROVAL_REDIRECT_MAX + 1))).toEqual({ ok: false, reason: 'too_long' })
  })
})

describe('buildDecisionRequest', () => {
  it('builds approve and plain reject with an empty reason', () => {
    expect(buildDecisionRequest(model, 'approve', 'ignored text')).toEqual({
      ok: true,
      request: { approvalId: 'appr_1', decision: 'approve', reason: '' },
    })
    expect(buildDecisionRequest(model, 'reject')).toEqual({
      ok: true,
      request: { approvalId: 'appr_1', decision: 'reject', reason: '' },
    })
  })

  it('builds a redirect as a reject whose reason is the trimmed text', () => {
    expect(buildDecisionRequest(model, 'redirect', '  open the invoice tab  ')).toEqual({
      ok: true,
      request: { approvalId: 'appr_1', decision: 'reject', reason: 'open the invoice tab' },
    })
  })

  it('refuses a redirect with no text or with text over the cap', () => {
    expect(buildDecisionRequest(model, 'redirect', '')).toEqual({ ok: false, reason: 'empty' })
    expect(buildDecisionRequest(model, 'redirect', 'x'.repeat(APPROVAL_REDIRECT_MAX + 1))).toEqual({
      ok: false,
      reason: 'too_long',
    })
  })
})

describe('approvalRiskCopy', () => {
  it('returns bilingual copy for a known category and a generic label for an unknown one', () => {
    expect(approvalRiskCopy('login')).toEqual({ no: 'Innlogging', en: 'Sign-in' })
    expect(approvalRiskCopy('something_new')).toEqual({ no: 'Risikofylt handling', en: 'Risky action' })
  })

  it('returns null when there is no category (run approvals)', () => {
    expect(approvalRiskCopy('')).toBeNull()
  })
})
