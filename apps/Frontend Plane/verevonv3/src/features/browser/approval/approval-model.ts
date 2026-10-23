import { ORCHESTRATION_APPROVAL_REASON_MAX } from '@/shared/actions/action-registry'
import type { Approval } from '@/shared/api/orchestration-client'
import type { BrowserApprovalEntry } from '@/features/dashboard/home/browser-session'

/**
 * Pure view model for the shared keyboard-first approval card (E3.2-b).
 *
 * The card renders one pending decision. It can come from a browser-action
 * gate (`BrowserApprovalEntry`, the evidence drawer) or from an orchestration
 * run approval (`Approval`, the chat deck). Both map to the same model so the
 * card, its shortcuts and its decision request are one code path.
 *
 * Nothing here touches the DOM or the network. The redirect text travels in
 * the decision `reason` of a reject (see the action contract
 * `orchestration.decide_approval`); the owner decides whether it is acted on.
 */

/** Longest redirect text the card will send. Same cap as the action contract. */
export const APPROVAL_REDIRECT_MAX = ORCHESTRATION_APPROVAL_REASON_MAX

export type ApprovalCardSource = 'browser' | 'run'

export type ApprovalCardModel = {
  approvalId: string
  source: ApprovalCardSource
  /** What needs approval, as the owner described it. Empty when the owner gave none. */
  reason: string
  /** Browser action type (e.g. "click") or the run approval kind. Empty when none. */
  actionType: string
  /** Origin and path of the target page plus the selector, with any query string dropped. */
  target: string
  /** Browser risk category key (login, checkout, ...). Empty for run approvals. */
  riskCategory: string
}

/** One of the three things the person can do with a pending approval. */
export type ApprovalCardAction = 'approve' | 'reject' | 'redirect'

export type ApprovalDecisionRequest = {
  approvalId: string
  decision: 'approve' | 'reject'
  /** '' for approve and plain reject; the trimmed redirect text for a redirect. */
  reason: string
}

export type RedirectTextCheck =
  | { ok: true; text: string }
  | { ok: false; reason: 'empty' | 'too_long' }

/**
 * Origin and path of a URL with the query string and fragment dropped, so a
 * token in a query never reaches the card. Non-URL input is returned trimmed.
 */
export function approvalTargetSummary(url: string, selector: string): string {
  const parts: string[] = []
  const rawUrl = url.trim()
  if (rawUrl) {
    try {
      const parsed = new URL(rawUrl)
      parts.push(`${parsed.host}${parsed.pathname}`)
    } catch {
      parts.push(rawUrl)
    }
  }
  const rawSelector = selector.trim()
  if (rawSelector) parts.push(rawSelector)
  return parts.join(' · ')
}

/** Maps a pending browser-action gate to the card model. Null when not pending. */
export function browserApprovalToCardModel(entry: BrowserApprovalEntry): ApprovalCardModel | null {
  if (entry.status !== 'pending' || !entry.approvalId) return null
  return {
    approvalId: entry.approvalId,
    source: 'browser',
    reason: entry.reason.trim(),
    actionType: entry.actionType.trim(),
    target: approvalTargetSummary(entry.url, entry.selector),
    riskCategory: entry.riskCategory,
  }
}

/** Maps a pending orchestration-run approval to the card model. Null when decided. */
export function runApprovalToCardModel(approval: Approval): ApprovalCardModel | null {
  if (approval.status && approval.status !== 'PENDING') return null
  if (!approval.id) return null
  return {
    approvalId: approval.id,
    source: 'run',
    reason: (approval.detail ?? '').trim(),
    actionType: (approval.kind ?? '').trim(),
    target: '',
    riskCategory: '',
  }
}

/**
 * Trims redirect text and checks the cap. Empty text is not a redirect, so a
 * redirect with nothing in it cannot be sent as a reject.
 */
export function checkRedirectText(raw: string): RedirectTextCheck {
  const text = raw.trim()
  if (!text) return { ok: false, reason: 'empty' }
  if (text.length > APPROVAL_REDIRECT_MAX) return { ok: false, reason: 'too_long' }
  return { ok: true, text }
}

/**
 * Builds the decision request for one card action. Approve and reject carry no
 * reason. A redirect is a reject whose reason is the checked redirect text.
 */
export function buildDecisionRequest(
  model: ApprovalCardModel,
  action: ApprovalCardAction,
  redirectRaw = '',
): { ok: true; request: ApprovalDecisionRequest } | { ok: false; reason: 'empty' | 'too_long' } {
  if (action === 'approve') {
    return { ok: true, request: { approvalId: model.approvalId, decision: 'approve', reason: '' } }
  }
  if (action === 'reject') {
    return { ok: true, request: { approvalId: model.approvalId, decision: 'reject', reason: '' } }
  }
  const check = checkRedirectText(redirectRaw)
  if (!check.ok) return { ok: false, reason: check.reason }
  return {
    ok: true,
    request: { approvalId: model.approvalId, decision: 'reject', reason: check.text },
  }
}

/** Norwegian and English copy for a browser risk category. Null when unknown. */
export const APPROVAL_RISK_COPY: Readonly<Record<string, { no: string; en: string }>> = {
  checkout: { no: 'Utsjekk/betaling', en: 'Checkout or payment' },
  cross_domain_navigation: { no: 'Navigering til nytt domene', en: 'Navigation to another domain' },
  destructive: { no: 'Ødeleggende handling', en: 'Destructive action' },
  login: { no: 'Innlogging', en: 'Sign-in' },
  persistent_cookie_use: { no: 'Bruker vedvarende profil/informasjonskapsler', en: 'Uses a persistent profile or cookies' },
  posting_form: { no: 'Skjemainnsending', en: 'Form submission' },
}

export function approvalRiskCopy(category: string): { no: string; en: string } | null {
  if (!category) return null
  return APPROVAL_RISK_COPY[category] ?? { no: 'Risikofylt handling', en: 'Risky action' }
}
