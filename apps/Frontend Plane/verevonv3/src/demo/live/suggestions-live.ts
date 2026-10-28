/**
 * The three cases as the visitor meets them: the start-screen card on /chat, a
 * card on the dashboard, the fallback's «#demo:<id>» links, and the «Utfør»
 * choice that a Do case needs. A click only changes the real composer and
 * presses its real send button, so the product answers through the live engine
 * exactly as it would for a typed question.
 *
 * «Utfør» exists only in the chat composer: the dashboard composer has no plan-mode
 * control. So a Do card on the dashboard opens the chat page through the demo's own
 * link policy, with the question in the composer and «Utfør» chosen; the visitor
 * sends from there. A Do case started from a link or the start screen is sent from
 * the chat page the same way.
 *
 * The Work canvas that a Do or an Ask turn opens is left to the product, except that a Do turn's run panel is moved to its
 * Resultat tab when the approval appears (showResultAtGate). On a
 * phone demo.css keeps that canvas as a pane below the conversation, so the
 * approval gate stays in view (PLAN_2026-10-11, "phone hazard"). After a send,
 * on a phone, a short timed window keeps the transcript at its end for the
 * approval that arrives, and again when the Work pane opens or closes in that
 * window (followApproval), because the product does not scroll to a Do turn's
 * gate and the pane covers the lower part of the transcript. Nothing here clicks
 * a product control in answer to a DOM change: an observer that folded the canvas
 * appeared to ping-pong with the product's own auto-open and froze the page, so
 * that fold was removed.
 */
import { buildSuggestionBlock, COMPOSER_TEXTAREA, fillComposer, type SceneSuggestionSet } from '../suggestions'
import type { Scenario } from './scenario-types'

const START_HEADING = '.verevon-chat-empty__heading'
const HOME_GRID = '.home-overview-grid'
const CHAT_PAGE = '.verevon-chat-page'
const MODE_SEGMENT = '.dashboard-composer-intent-segment'
const SEND_BUTTON = 'button[aria-label="Send melding"]'
const TRANSCRIPT = '.verevon-chat-message-list'
const APPROVAL = '.verevon-chat-approval'
const RUN_PANEL = '.verevon-chat-run-panel'
const PHONE_QUERY = '(max-width: 760px)'
const LINK_PREFIX = '#demo:'
const CARD_LABEL = 'Dette har skjedd'
/** How long a step is retried while the composer, the chat page or the send button is still mounting. */
const START_ATTEMPTS = 50
const START_RETRY_MS = 100
/**
 * After a send, on a phone, the transcript is followed for its approval: a Do turn's gate arrives about five seconds
 * after the send, and the product does not scroll to it. The window is fixed, and it only ever scrolls the transcript
 * to its end, at most MAX_FOLLOWS times for one send.
 */
const GATE_WATCH_MS = 20_000
const GATE_POLL_MS = 250
/** Scrolls for one send: one for the approval, and one each time the Work pane opens or closes in the window. */
const MAX_FOLLOWS = 3
/** The Work pane as the phone shows it: the canvas, or the run panel that hosts it. */
const WORK_PANE = '.verevon-chat-workspace-canvas, .verevon-chat-run-panel'
const RESULT_TAB_LABEL = 'Resultat'

export type LiveSuggestions = {
  /** Starts a case as if the visitor had picked its card and sent it. */
  start(scenario: Scenario): void
  /** Removes the listeners and the observer. The placed cards stay with the page. */
  dispose(): void
}

/**
 * Chooses «Utfør» (plan mode) in the chat composer, when it is not chosen already. The chat composer shows
 * Spør and Utfør as a segment. Both controls report the choice in aria-pressed.
 */
function selectDoMode(): void {
  const segment = Array.from(document.querySelectorAll<HTMLButtonElement>(`${MODE_SEGMENT} button`))
  const doButton = segment.find((button) => button.textContent?.trim() === 'Utfør')
  if (doButton && doButton.getAttribute('aria-pressed') !== 'true') doButton.click()
}

/** The chat page is mounted with its composer and its mode segment, where plan mode can be chosen. */
function chatReady(): boolean {
  return Boolean(document.querySelector(CHAT_PAGE) && document.querySelector(MODE_SEGMENT) && document.querySelector(COMPOSER_TEXTAREA))
}

/** Opens the chat page the way a visitor's link does: an in-page anchor, which the demo's link policy routes. */
function openChatPage(): void {
  const anchor = document.createElement('a')
  anchor.setAttribute('href', '/chat')
  document.body.append(anchor)
  anchor.click()
  anchor.remove()
}

/** Fills the composer with the case's question and chooses its mode, without sending. False while the composer is not there. */
function prepareCase(scenario: Scenario): boolean {
  if (!fillComposer(scenario.suggestion.text)) return false
  if (scenario.mode === 'do') selectDoMode()
  return true
}

/** Prepares the case and presses send. False while the composer or the send button is not there or not enabled yet. */
function sendCase(scenario: Scenario): boolean {
  if (!prepareCase(scenario)) return false
  const send = document.querySelector<HTMLButtonElement>(SEND_BUTTON)
  // The product enables send a moment after the text lands, so a disabled button means "try again shortly".
  if (!send || send.disabled || send.getAttribute('aria-disabled') === 'true') return false
  send.click()
  return true
}

/** The newest approval card: older turns keep theirs in the transcript. */
function latestApproval(): Element | null {
  const all = document.querySelectorAll(APPROVAL)
  return all.length > 0 ? (all[all.length - 1] ?? null) : null
}

/** Whether the Work pane is on the screen: a pane that is not open has no boxes, so it reports none. */
function isPaneShown(): boolean {
  return Array.from(document.querySelectorAll<HTMLElement>(WORK_PANE)).some((pane) => pane.getClientRects().length > 0)
}

export function installLiveSuggestions(options: { scenarios: readonly Scenario[] }): LiveSuggestions {
  const { scenarios } = options
  const set: SceneSuggestionSet = {
    label: CARD_LABEL,
    items: scenarios.map((scenario) => ({
      text: scenario.suggestion.text,
      source: scenario.suggestion.source,
      time: scenario.suggestion.time,
      icon: scenario.suggestion.icon,
    })),
  }
  let disposed = false
  let watchTimer: number | undefined
  let resultTimer: number | undefined
  let followed: Element | null = null
  let resultShownFor: Element | null = null

  /**
   * On a phone, after a send, keeps the transcript at its end for the approval of that send: it scrolls when the
   * approval appears, and again when the Work pane opens or closes in the window, because the pane covers the lower
   * part of the transcript and the layout moves under it. Each send has its own budget of MAX_FOLLOWS scrolls and its
   * own deadline. Only a timer starts and drives this, never a DOM change, and it clicks nothing.
   */
  const followApproval = () => {
    if (typeof window.matchMedia !== 'function' || !window.matchMedia(PHONE_QUERY).matches) return
    window.clearTimeout(watchTimer)
    const deadline = Date.now() + GATE_WATCH_MS
    let followsLeft = MAX_FOLLOWS
    let paneWas = isPaneShown()
    const poll = () => {
      if (disposed) return
      const approval = latestApproval()
      const transcript = document.querySelector<HTMLElement>(TRANSCRIPT)
      const paneNow = isPaneShown()
      const paneChanged = paneNow !== paneWas
      paneWas = paneNow
      if (approval && transcript && followsLeft > 0 && (approval !== followed || paneChanged)) {
        followed = approval
        followsLeft -= 1
        transcript.scrollTop = transcript.scrollHeight
      }
      if (Date.now() < deadline) watchTimer = window.setTimeout(poll, GATE_POLL_MS)
    }
    poll()
  }

  /**
   * A Do turn opens the run panel on its Arbeid tab, where the three sub-tasks run. The quote they produce is the point
   * of the case, so when the approval for it appears the Resultat tab is shown, once for that approval. Only a timer started
   * by a send drives this: one click at most per approval, never a reaction to a DOM change, so it cannot ping-pong with the
   * product. An Ask turn has no run panel tab to switch, so nothing happens.
   */
  const showResultAtGate = () => {
    window.clearTimeout(resultTimer)
    const deadline = Date.now() + GATE_WATCH_MS
    const poll = () => {
      if (disposed) return
      const approval = latestApproval()
      if (approval && approval !== resultShownFor) {
        resultShownFor = approval
        const tabs = Array.from(document.querySelectorAll<HTMLElement>(`${RUN_PANEL} [role="tab"]`))
        const tab = tabs.find((candidate) => candidate.textContent?.trim().startsWith(RESULT_TAB_LABEL))
        const shown = tab?.getAttribute('aria-pressed') === 'true' || tab?.getAttribute('aria-selected') === 'true'
        if (tab && !shown) tab.click()
        return
      }
      if (Date.now() < deadline) resultTimer = window.setTimeout(poll, GATE_POLL_MS)
    }
    poll()
  }

  /** Runs `action` until it reports done, retrying for a bounded time while the product mounts what it needs. */
  const retry = (action: () => boolean) => {
    let attempts = 0
    const attempt = () => {
      if (disposed || action()) return
      attempts += 1
      if (attempts < START_ATTEMPTS) window.setTimeout(attempt, START_RETRY_MS)
    }
    attempt()
  }

  /** Starts a case as if the visitor had picked its card and sent it. A Do case is sent from the chat page. */
  const startCase = (scenario: Scenario) => {
    if (scenario.mode === 'do') {
      if (!chatReady()) openChatPage()
      retry(() => chatReady() && sendCase(scenario))
      return
    }
    retry(() => sendCase(scenario))
  }

  /** A Do card: the chat page opens with the question in its composer and «Utfør» chosen. The visitor sends. */
  const showDoCase = (scenario: Scenario) => {
    if (!chatReady()) openChatPage()
    retry(() => chatReady() && prepareCase(scenario))
  }

  /** Places one card on the start screen and one on the dashboard, once each per mount. */
  const placeCards = () => {
    const heading = document.querySelector<HTMLElement>(START_HEADING)
    if (heading && !heading.querySelector('.v-suggestions')) heading.appendChild(buildSuggestionBlock(set))
    const grid = document.querySelector<HTMLElement>(HOME_GRID)
    if (grid && !grid.querySelector(':scope > .v-suggestions--home')) {
      const block = buildSuggestionBlock(set)
      block.classList.add('v-suggestions--home')
      grid.prepend(block)
    }
  }

  const onClick = (event: MouseEvent) => {
    const target = event.target instanceof Element ? event.target : null
    if (!target) return
    if (target.closest(SEND_BUTTON)) {
      followApproval()
      showResultAtGate()
    }
    const link = target.closest<HTMLAnchorElement>(`a[href^="${LINK_PREFIX}"]`)
    if (link) {
      // A fallback link starts a case inside the page. It never navigates.
      const id = (link.getAttribute('href') ?? '').slice(LINK_PREFIX.length)
      const scenario = scenarios.find((candidate) => candidate.id === id)
      if (!scenario) return
      event.preventDefault()
      event.stopPropagation()
      startCase(scenario)
      return
    }
    // A card fills the composer (the card's own handler). A Do card also chooses «Utfør», in the chat composer.
    const card = target.closest('.v-suggestion')
    if (!card) return
    const text = card.querySelector('.v-suggestion__text')?.textContent?.trim()
    const scenario = scenarios.find((candidate) => candidate.suggestion.text === text)
    if (scenario?.mode === 'do') showDoCase(scenario)
  }

  // Cards are placed again whenever the start screen or the dashboard mounts. Placing adds only a card that
  // is missing, so the observer's own changes find nothing to do. It never clicks a product control.
  const observer = new MutationObserver(() => placeCards())
  const start = () => {
    observer.observe(document.body, { childList: true, subtree: true })
    placeCards()
  }
  // Enter in the composer sends too (Shift+Enter is a new line), so it starts the same follow window as the send button.
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return
    if (event.target instanceof Element && event.target.matches(COMPOSER_TEXTAREA)) {
      followApproval()
      showResultAtGate()
    }
  }
  document.addEventListener('click', onClick, true)
  document.addEventListener('keydown', onKeyDown, true)
  if (document.body) start()
  else document.addEventListener('DOMContentLoaded', start, { once: true })

  return {
    start: startCase,
    dispose() {
      disposed = true
      observer.disconnect()
      window.clearTimeout(watchTimer)
      window.clearTimeout(resultTimer)
      document.removeEventListener('click', onClick, true)
      document.removeEventListener('keydown', onKeyDown, true)
    },
  }
}
