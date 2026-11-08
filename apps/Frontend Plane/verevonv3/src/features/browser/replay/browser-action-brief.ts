/**
 * Per-action brief for the agent run console (F-t3-050, R2). One brief per browser action,
 * built from the events the run already streams: the dispatch (action type, target and the
 * model's reason), the approval gate and its decision, and the observation (page, status and
 * screenshot reference). It only reads those events. It fetches, stores and persists nothing,
 * so the session's ZDR posture is unchanged. The switch is `VITE_BROWSER_ACTION_BRIEF`, which
 * is off unless its value is the literal `true`.
 */

import {
  browserApprovalStatusLabel,
  browserRiskLabel,
  type BrowserApprovalStatus,
} from '@/shared/approvals/approval-copy'

/** The translator the brief reads its copy through: `i18n.tr`, or a stand-in in tests. */
export type BrowserActionBriefTr = (noText: string, enText: string) => string

/** The longest model reason the brief shows. */
export const BROWSER_ACTION_BRIEF_REASON_MAX_CHARS = 280
/** The longest page title the brief shows. */
export const BROWSER_ACTION_BRIEF_TITLE_MAX_CHARS = 160

/** The decisions an approval can end with. A pending gate has none yet. */
export type BrowserActionDecision = Exclude<BrowserApprovalStatus, 'pending'>

/** What the run's events have said about one browser action so far. Every field starts empty. */
export type BrowserActionFacts = {
  actionId: string
  actionType: string
  /** The action's target, as the loop dispatched it. Only its host is ever shown. */
  url: string
  /** The model's rationale for the action; empty for the deterministic fallback. */
  reason: string
  /** True once an approval gate was raised for this action. */
  gated: boolean
  riskCategory: string
  /** The gate's decision; empty while it is pending or when the action was not gated. */
  decision: BrowserActionDecision | ''
  observed: boolean
  /** The observation's status as the wire sent it: success, failed, timeout, blocked or site_blocked. */
  outcome: string
  pageUrl: string
  pageTitle: string
  /** A reference to the observation's screenshot, never its bytes. */
  screenshotRef: string
}

export type BrowserActionPill = {
  label: string
  tone: 'ok' | 'warn' | 'error' | 'neutral'
}

/** Build-time switch. Only the literal `true` turns the per-action brief on. */
export function browserActionBriefEnabled(): boolean {
  return import.meta.env.VITE_BROWSER_ACTION_BRIEF === 'true'
}

/** The timeline row a browser action fills in. The plan is part of the key, because action ids restart at each plan. */
export function browserActionRowId(planId: string | undefined, actionId: string): string {
  return `browse-${planId ?? ''}:${actionId}`
}

export function emptyBrowserActionFacts(actionId: string): BrowserActionFacts {
  return {
    actionId,
    actionType: '',
    url: '',
    reason: '',
    gated: false,
    riskCategory: '',
    decision: '',
    observed: false,
    outcome: '',
    pageUrl: '',
    pageTitle: '',
    screenshotRef: '',
  }
}

/** The dispatch of an action. A field the event leaves out keeps what the facts already hold. */
export function withBrowserDispatched(
  facts: BrowserActionFacts,
  event: { actionType?: string; url?: string; reason?: string },
): BrowserActionFacts {
  return {
    ...facts,
    actionType: event.actionType || facts.actionType,
    url: event.url || facts.url,
    reason: event.reason || facts.reason,
  }
}

/** An approval gate raised for the action. A new gate starts pending, whatever an earlier one decided. */
export function withBrowserApprovalRequired(
  facts: BrowserActionFacts,
  event: { riskCategory?: string },
): BrowserActionFacts {
  return {
    ...facts,
    gated: true,
    riskCategory: event.riskCategory || facts.riskCategory,
    decision: '',
  }
}

/** The decision a gate ended with. Any other value leaves the facts as they are. */
export function withBrowserDecided(
  facts: BrowserActionFacts,
  event: { decision?: string },
): BrowserActionFacts {
  const decision = decisionOf(event.decision)
  return decision ? { ...facts, decision } : facts
}

/** The observation the loop took after the action. */
export function withBrowserObserved(
  facts: BrowserActionFacts,
  event: { status?: string; pageUrl?: string; pageTitle?: string; screenshotRef?: string },
): BrowserActionFacts {
  return {
    ...facts,
    observed: true,
    outcome: event.status || facts.outcome,
    pageUrl: event.pageUrl || facts.pageUrl,
    pageTitle: event.pageTitle || facts.pageTitle,
    screenshotRef: event.screenshotRef || facts.screenshotRef,
  }
}

function decisionOf(value: string | undefined): BrowserActionDecision | '' {
  return value === 'granted' || value === 'denied' || value === 'timed_out' ? value : ''
}

/** Where the action's approval stands: no gate, a gate still waiting, or the decision it ended with. */
export function browserActionGate(facts: BrowserActionFacts): 'none' | 'pending' | BrowserActionDecision {
  if (!facts.gated) return 'none'
  return facts.decision || 'pending'
}

/** The host of a URL without its `www.` prefix; empty when there is no usable URL. Paths and queries never reach the brief. */
export function browserActionHost(url: string | undefined): string {
  const value = url?.trim() ?? ''
  if (!value) return ''
  try {
    return new URL(value).hostname.replace(/^www\./, '')
  } catch {
    return ''
  }
}

function bounded(value: string, max: number): string {
  const text = value.replace(/\s+/g, ' ').trim()
  return text.length > max ? `${text.slice(0, max - 1)}…` : text
}

const VERB_LABELS: Record<string, (tr: BrowserActionBriefTr) => string> = {
  back: (tr) => tr('Gå tilbake', 'Go back'),
  click: (tr) => tr('Klikk', 'Click'),
  extract: (tr) => tr('Hent innhold', 'Extract content'),
  goto: (tr) => tr('Åpne side', 'Open page'),
  observe: (tr) => tr('Undersøk siden', 'Inspect the page'),
  press: (tr) => tr('Trykk tast', 'Press a key'),
  scroll: (tr) => tr('Rull', 'Scroll'),
  select: (tr) => tr('Velg', 'Select'),
  type: (tr) => tr('Skriv', 'Type'),
  wait: (tr) => tr('Vent', 'Wait'),
  wait_for: (tr) => tr('Vent på', 'Wait for'),
}

function verbOf(actionType: string, tr: BrowserActionBriefTr): string {
  const key = actionType.trim().toLowerCase()
  const label = Object.hasOwn(VERB_LABELS, key) ? VERB_LABELS[key] : undefined
  return label ? label(tr) : tr('Nettleserhandling', 'Browser action')
}

/** The row's title: what the action did and where, as `Verb · host`. */
export function browserActionTitle(facts: BrowserActionFacts, tr: BrowserActionBriefTr): string {
  const verb = facts.actionType ? verbOf(facts.actionType, tr) : tr('Nettleser', 'Browser')
  const host = browserActionHost(facts.url) || browserActionHost(facts.pageUrl)
  return host ? `${verb} · ${host}` : verb
}

function outcomePill(outcome: string, tr: BrowserActionBriefTr): BrowserActionPill {
  switch (outcome) {
    case 'success':
      return { label: tr('Vellykket', 'Succeeded'), tone: 'ok' }
    case 'failed':
      return { label: tr('Mislyktes', 'Failed'), tone: 'error' }
    case 'timeout':
      return { label: tr('Tidsavbrudd', 'Timed out'), tone: 'warn' }
    case 'blocked':
    case 'site_blocked':
      return { label: tr('Blokkert', 'Blocked'), tone: 'error' }
    default:
      return { label: tr('Ferdig', 'Done'), tone: 'neutral' }
  }
}

/** The pill on the row: the latest thing the action reached. The gate comes before the observation. */
export function browserActionPill(facts: BrowserActionFacts, tr: BrowserActionBriefTr): BrowserActionPill {
  const gate = browserActionGate(facts)
  if (gate === 'pending') return { label: browserApprovalStatusLabel('pending', { tr }), tone: 'warn' }
  if (gate === 'denied') return { label: browserApprovalStatusLabel('denied', { tr }), tone: 'error' }
  if (gate === 'timed_out') return { label: browserApprovalStatusLabel('timed_out', { tr }), tone: 'warn' }
  if (facts.observed) return outcomePill(facts.outcome, tr)
  if (gate === 'granted') return { label: browserApprovalStatusLabel('granted', { tr }), tone: 'ok' }
  return { label: tr('Sendt', 'Sent'), tone: 'neutral' }
}

/**
 * The lines under the title, in reading order: the model's reason, the approval, the page the
 * observation saw, and a plain note when a seen page has no screenshot.
 */
export function browserActionLines(facts: BrowserActionFacts, tr: BrowserActionBriefTr): string[] {
  const lines: string[] = []
  const reason = bounded(facts.reason, BROWSER_ACTION_BRIEF_REASON_MAX_CHARS)
  if (reason) lines.push(reason)
  const gate = browserActionGate(facts)
  if (gate !== 'none') {
    const state = browserApprovalStatusLabel(gate, { tr })
    lines.push(facts.riskCategory
      ? `${browserRiskLabel(facts.riskCategory, { tr })} · ${state}`
      : `${tr('Godkjenning', 'Approval')}: ${state}`)
  }
  if (facts.observed) {
    const page = bounded(facts.pageTitle, BROWSER_ACTION_BRIEF_TITLE_MAX_CHARS) || browserActionHost(facts.pageUrl)
    if (page) lines.push(`${tr('Observert', 'Observed')}: ${page}`)
    if (!facts.screenshotRef) lines.push(tr('Ingen skjermbilde for dette steget.', 'No screenshot for this step.'))
  }
  return lines
}
