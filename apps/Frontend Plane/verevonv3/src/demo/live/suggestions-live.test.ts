// @vitest-environment jsdom
/**
 * The live suggestions in jsdom. A fallback link starts its case through the composer and the send button and never
 * navigates. «Utfør» is chosen in the chat composer: a Do case from a link or a chat card chooses it there, and a Do
 * card on the dashboard opens the chat page with the question and «Utfør» set, sending nothing. The cards are placed
 * once each. A DOM change never clicks a product control: the fold that clicked the Work canvas on every mutation
 * looped against the product's own auto-open, so no observer may click.
 */
import { afterEach, describe, expect, it } from 'vitest'
import type { Scenario } from './scenario-types'
import { scenarios } from './scenarios'
import { installLiveSuggestions, type LiveSuggestions } from './suggestions-live'

const COMPOSER = 'textarea#verevon-dashboard-input'
const SEND = 'button[aria-label="Send melding"]'

const tick = (ms = 0) => new Promise<void>((resolve) => window.setTimeout(resolve, ms))
const originalMatchMedia = window.matchMedia

/** Answers the phone query as the given screen would (jsdom has no media queries). */
function stubPhone(matches: boolean): void {
  const answer = (query: string) => ({
    matches,
    media: query,
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  })
  Object.defineProperty(window, 'matchMedia', { value: answer, configurable: true, writable: true })
}

/** A transcript of a given height that keeps the scroll position it is given (jsdom has no layout). */
function mountTranscript(height: number): HTMLElement {
  const transcript = document.createElement('div')
  transcript.className = 'verevon-chat-message-list'
  Object.defineProperty(transcript, 'scrollHeight', { value: height })
  Object.defineProperty(transcript, 'scrollTop', { value: 0, writable: true })
  document.body.append(transcript)
  return transcript
}

function required<T extends Element>(element: T | null, what: string): T {
  if (!element) throw new Error(`missing ${what}`)
  return element
}

function scenarioOf(id: string): Scenario {
  const found = scenarios.find((candidate) => candidate.id === id)
  if (!found) throw new Error(`no scenario ${id}`)
  return found
}

const TILBUD = scenarioOf('tilbud-solhaug')
const REKLAMASJON = scenarioOf('reklamasjon-bordplate')

/** The dashboard: the start-screen heading, the grid, and the dashboard composer with its Do chip. */
function mountDashboard(): void {
  document.body.innerHTML = `
    <main>
      <div class="verevon-chat-empty__heading"></div>
      <div class="home-overview-grid"></div>
      <span class="dashboard-composer-intent-toggle">
        <button type="button" aria-label="Ask-modus" aria-pressed="false"></button>
        <span class="dashboard-composer-intent-toggle__label">Ask</span>
      </span>
      <textarea id="verevon-dashboard-input"></textarea>
      <button type="button" aria-label="Send melding">Send</button>
    </main>
  `
}

/** The chat page: the start-screen heading, and the chat composer with its Ask and Utfør segment. */
function mountChat(): void {
  document.body.innerHTML = `
    <main class="verevon-chat-page">
      <div class="verevon-chat-empty__heading"></div>
      <div class="dashboard-composer-intent-segment" role="group">
        <button type="button" aria-pressed="true" class="is-active">Spør</button>
        <button type="button" aria-pressed="false">Utfør</button>
      </div>
      <textarea id="verevon-dashboard-input"></textarea>
      <button type="button" aria-label="Send melding">Send</button>
    </main>
  `
}

type Probe = { sent: string[]; planClicks: number; routed: number }
let stopProbe: (() => void) | undefined

/**
 * Records what the product would see: the text at each send, each choice of plan mode, and each route to the chat
 * page. A choice presses its control (aria-pressed), as the product does. Event delegation, so it survives a remount.
 * A route to /chat mounts the chat page, as the demo's router does.
 */
function instrument(): Probe {
  const probe: Probe = { sent: [], planClicks: 0, routed: 0 }
  const onClick = (event: MouseEvent) => {
    const anchor = event.target instanceof Element ? event.target.closest('a') : null
    if (anchor?.getAttribute('href') === '/chat') {
      event.preventDefault()
      probe.routed += 1
      mountChat()
      return
    }
    const button = event.target instanceof Element ? event.target.closest('button') : null
    if (!button) return
    if (button.matches(SEND)) {
      probe.sent.push(required(document.querySelector<HTMLTextAreaElement>(COMPOSER), 'composer').value)
      return
    }
    const segment = button.closest('.dashboard-composer-intent-segment')
    if (segment && button.textContent?.trim() === 'Utfør') {
      probe.planClicks += 1
      for (const other of segment.querySelectorAll('button')) other.setAttribute('aria-pressed', other === button ? 'true' : 'false')
      return
    }
    if (button.closest('.dashboard-composer-intent-toggle')) {
      probe.planClicks += 1
      const pressed = button.getAttribute('aria-pressed') !== 'true'
      button.setAttribute('aria-pressed', pressed ? 'true' : 'false')
      const label = document.querySelector('.dashboard-composer-intent-toggle__label')
      if (label) label.textContent = pressed ? 'Do' : 'Ask'
    }
  }
  document.addEventListener('click', onClick, true)
  stopProbe = () => document.removeEventListener('click', onClick, true)
  return probe
}

function fallbackLink(id: string): HTMLAnchorElement {
  const link = document.createElement('a')
  link.setAttribute('href', `#demo:${id}`)
  link.textContent = 'Lenke'
  document.body.append(link)
  return link
}

function cardFor(text: string): HTMLElement {
  const cards = Array.from(document.querySelectorAll<HTMLElement>('.v-suggestion'))
  return required(cards.find((card) => card.querySelector('.v-suggestion__text')?.textContent?.trim() === text) ?? null, `card ${text}`)
}

function composerValue(): string {
  return required(document.querySelector<HTMLTextAreaElement>(COMPOSER), 'composer').value
}

function pressedUtfor(): string | null {
  const button = Array.from(document.querySelectorAll<HTMLButtonElement>('.dashboard-composer-intent-segment button')).find((item) => item.textContent?.trim() === 'Utfør')
  return button?.getAttribute('aria-pressed') ?? null
}

describe('installLiveSuggestions', () => {
  let handle: LiveSuggestions | undefined

  afterEach(() => {
    handle?.dispose()
    handle = undefined
    stopProbe?.()
    stopProbe = undefined
    Object.defineProperty(window, 'matchMedia', { value: originalMatchMedia, configurable: true, writable: true })
    document.body.innerHTML = ''
  })

  it('a fallback link starts its case through the composer and the send button, and never navigates', () => {
    mountChat()
    const probe = instrument()
    handle = installLiveSuggestions({ scenarios })
    const event = new MouseEvent('click', { bubbles: true, cancelable: true })
    fallbackLink(REKLAMASJON.id).dispatchEvent(event)
    expect(event.defaultPrevented).toBe(true)
    expect(probe.sent).toEqual([REKLAMASJON.suggestion.text])
    expect(probe.planClicks).toBe(0)
    expect(probe.routed).toBe(0)
  })

  it('a Do case started from its link chooses Utfør in the chat composer before it sends', () => {
    mountChat()
    const probe = instrument()
    handle = installLiveSuggestions({ scenarios })
    fallbackLink(TILBUD.id).click()
    expect(probe.planClicks).toBe(1)
    expect(pressedUtfor()).toBe('true')
    expect(probe.sent).toEqual([TILBUD.suggestion.text])
  })

  it('a Do card on the dashboard opens the chat page with its question and Utfør chosen, and sends nothing', () => {
    mountDashboard()
    const probe = instrument()
    handle = installLiveSuggestions({ scenarios })
    cardFor(TILBUD.suggestion.text).click()
    expect(probe.routed).toBe(1)
    expect(composerValue()).toBe(TILBUD.suggestion.text)
    expect(pressedUtfor()).toBe('true')
    expect(probe.sent).toEqual([])
  })

  it('a Do card on the chat start screen chooses Utfør and sends nothing', () => {
    mountChat()
    const probe = instrument()
    handle = installLiveSuggestions({ scenarios })
    cardFor(TILBUD.suggestion.text).click()
    expect(probe.routed).toBe(0)
    expect(composerValue()).toBe(TILBUD.suggestion.text)
    expect(pressedUtfor()).toBe('true')
    expect(probe.sent).toEqual([])
  })

  it('an Ask card only fills the composer, and chooses nothing', () => {
    mountDashboard()
    const probe = instrument()
    handle = installLiveSuggestions({ scenarios })
    cardFor(REKLAMASJON.suggestion.text).click()
    expect(composerValue()).toBe(REKLAMASJON.suggestion.text)
    expect(probe.planClicks).toBe(0)
    expect(probe.routed).toBe(0)
    expect(probe.sent).toEqual([])
  })

  it('does not press Utfør again when it is already chosen', () => {
    mountChat()
    const probe = instrument()
    required(document.querySelector('.dashboard-composer-intent-segment button[aria-pressed="false"]'), 'Utfør').setAttribute('aria-pressed', 'true')
    handle = installLiveSuggestions({ scenarios })
    fallbackLink(TILBUD.id).click()
    expect(probe.planClicks).toBe(0)
    expect(probe.sent).toEqual([TILBUD.suggestion.text])
  })

  it('places one card block on the start screen and one on the dashboard, and adds none on later changes', async () => {
    mountDashboard()
    handle = installLiveSuggestions({ scenarios })
    await tick()
    expect(document.querySelectorAll('.verevon-chat-empty__heading > .v-suggestions')).toHaveLength(1)
    expect(document.querySelectorAll('.home-overview-grid > .v-suggestions--home')).toHaveLength(1)
    expect(document.querySelectorAll('.v-suggestion')).toHaveLength(scenarios.length * 2)
    for (let round = 0; round < 5; round += 1) document.body.append(document.createElement('div'))
    await tick()
    expect(document.querySelectorAll('.verevon-chat-empty__heading > .v-suggestions')).toHaveLength(1)
    expect(document.querySelectorAll('.home-overview-grid > .v-suggestions--home')).toHaveLength(1)
  })

  it('never clicks a product control because the DOM changed, however often the Work panel re-mounts', async () => {
    mountDashboard()
    const panelMarkup = `
      <aside class="verevon-chat-run-panel verevon-chat-run-panel--workspace">
        <button type="button" class="verevon-chat-run-panel__toggle">Lukk</button>
      </aside>`
    document.body.insertAdjacentHTML('beforeend', panelMarkup)
    let toggled = 0
    required(document.querySelector<HTMLButtonElement>('.verevon-chat-run-panel__toggle'), 'toggle').addEventListener('click', () => {
      toggled += 1
    })
    handle = installLiveSuggestions({ scenarios })
    for (let round = 0; round < 5; round += 1) {
      document.querySelector('.verevon-chat-run-panel')?.remove()
      document.body.insertAdjacentHTML('beforeend', panelMarkup)
      await tick()
    }
    expect(toggled).toBe(0)
  })

  it('a start waits while the send button is disabled, then sends once', async () => {
    mountChat()
    const probe = instrument()
    const send = required(document.querySelector<HTMLButtonElement>(SEND), 'send')
    send.disabled = true
    handle = installLiveSuggestions({ scenarios })
    handle.start(TILBUD)
    expect(probe.sent).toEqual([])
    await tick(60)
    send.disabled = false
    await tick(300)
    expect(probe.sent).toEqual([TILBUD.suggestion.text])
    expect(probe.planClicks).toBe(1)
  })

  it('after dispose a link no longer starts a case', () => {
    mountChat()
    const probe = instrument()
    handle = installLiveSuggestions({ scenarios })
    handle.dispose()
    handle = undefined
    const event = new MouseEvent('click', { bubbles: true, cancelable: true })
    fallbackLink(REKLAMASJON.id).dispatchEvent(event)
    expect(event.defaultPrevented).toBe(false)
    expect(probe.sent).toEqual([])
  })

  it('on a phone, after a send, moves the transcript to its end once for the approval that arrives', async () => {
    mountChat()
    const transcript = mountTranscript(1000)
    stubPhone(true)
    handle = installLiveSuggestions({ scenarios })
    required(document.querySelector<HTMLButtonElement>(SEND), 'send').click()
    await tick(300)
    expect(transcript.scrollTop).toBe(0)
    const approval = document.createElement('div')
    approval.className = 'verevon-chat-approval'
    transcript.append(approval)
    await tick(400)
    expect(transcript.scrollTop).toBe(1000)
  })

  it('on a wide screen the transcript is left where the product put it', async () => {
    mountChat()
    const transcript = mountTranscript(1000)
    stubPhone(false)
    handle = installLiveSuggestions({ scenarios })
    required(document.querySelector<HTMLButtonElement>(SEND), 'send').click()
    const approval = document.createElement('div')
    approval.className = 'verevon-chat-approval'
    transcript.append(approval)
    await tick(600)
    expect(transcript.scrollTop).toBe(0)
  })

  it('a visitor who scrolls back up after the approval is not pulled down again', async () => {
    mountChat()
    const transcript = mountTranscript(1000)
    stubPhone(true)
    handle = installLiveSuggestions({ scenarios })
    const send = required(document.querySelector<HTMLButtonElement>(SEND), 'send')
    send.click()
    const approval = document.createElement('div')
    approval.className = 'verevon-chat-approval'
    transcript.append(approval)
    await tick(600)
    expect(transcript.scrollTop).toBe(1000)
    transcript.scrollTop = 0
    send.click()
    await tick(600)
    expect(transcript.scrollTop).toBe(0)
  })

  it('on a phone, follows again when the Work pane opens after the approval', async () => {
    mountChat()
    const transcript = mountTranscript(1000)
    stubPhone(true)
    handle = installLiveSuggestions({ scenarios })
    required(document.querySelector<HTMLButtonElement>(SEND), 'send').click()
    const approval = document.createElement('div')
    approval.className = 'verevon-chat-approval'
    transcript.append(approval)
    await tick(300)
    expect(transcript.scrollTop).toBe(1000)
    // The pane opens later and covers the lower part of the transcript, so the visitor is brought back to the end.
    transcript.scrollTop = 400
    const pane = document.createElement('aside')
    pane.className = 'verevon-chat-workspace-canvas'
    Object.defineProperty(pane, 'getClientRects', { value: () => [{}] })
    document.body.append(pane)
    await tick(400)
    expect(transcript.scrollTop).toBe(1000)
  })

  it('a send scrolls the transcript at most three times, however often the Work pane opens and closes', async () => {
    mountChat()
    const transcript = document.createElement('div')
    transcript.className = 'verevon-chat-message-list'
    let position = 0
    let writes = 0
    Object.defineProperty(transcript, 'scrollHeight', { value: 1000 })
    Object.defineProperty(transcript, 'scrollTop', {
      get: () => position,
      set: (value: number) => {
        writes += 1
        position = value
      },
    })
    document.body.append(transcript)
    stubPhone(true)
    handle = installLiveSuggestions({ scenarios })
    required(document.querySelector<HTMLButtonElement>(SEND), 'send').click()
    const approval = document.createElement('div')
    approval.className = 'verevon-chat-approval'
    transcript.append(approval)
    const pane = document.createElement('aside')
    pane.className = 'verevon-chat-workspace-canvas'
    document.body.append(pane)
    for (let cycle = 0; cycle < 6; cycle += 1) {
      Object.defineProperty(pane, 'getClientRects', { configurable: true, value: () => (cycle % 2 === 0 ? [{}] : []) })
      await tick(300)
    }
    expect(writes).toBe(3)
  })
})

describe('the Resultat tab of a Do turn', () => {
  let handle: LiveSuggestions | undefined

  afterEach(() => {
    handle?.dispose()
    handle = undefined
    document.body.innerHTML = ''
  })

  /** The run panel with its tabs, as the product renders it for a Do turn: Arbeid is shown. */
  function mountRunPanel(resultShown = false): { result: HTMLButtonElement; clicks: () => number } {
    const panel = document.createElement('aside')
    panel.className = 'verevon-chat-run-panel verevon-chat-run-panel--workspace'
    panel.innerHTML = `
      <button type="button" role="tab" aria-pressed="${resultShown ? 'false' : 'true'}">Arbeid<span>1</span></button>
      <button type="button" role="tab" aria-pressed="${resultShown ? 'true' : 'false'}">Resultat<span>1</span></button>
      <button type="button" role="tab" aria-pressed="false">Kilder<span>4</span></button>`
    document.body.append(panel)
    const result = required(Array.from(panel.querySelectorAll<HTMLButtonElement>('[role="tab"]')).find((tab) => tab.textContent?.startsWith('Resultat')) ?? null, 'result tab')
    let count = 0
    result.addEventListener('click', () => {
      count += 1
    })
    return { result, clicks: () => count }
  }

  function addApproval(transcript: HTMLElement): HTMLElement {
    const approval = document.createElement('div')
    approval.className = 'verevon-chat-approval'
    transcript.append(approval)
    return approval
  }

  it('is shown once when the approval appears, so the quote is the first thing in the pane', async () => {
    mountChat()
    const transcript = mountTranscript(1000)
    const { clicks } = mountRunPanel()
    handle = installLiveSuggestions({ scenarios })
    required(document.querySelector<HTMLButtonElement>(SEND), 'send').click()
    await tick(300)
    expect(clicks()).toBe(0)
    addApproval(transcript)
    await tick(400)
    expect(clicks()).toBe(1)
    // Nothing more happens for the same approval, however long the page keeps changing.
    for (let round = 0; round < 4; round += 1) {
      transcript.append(document.createElement('p'))
      await tick(300)
    }
    expect(clicks()).toBe(1)
  })

  it('is shown again for the approval of a later send, and only once for each', async () => {
    mountChat()
    const transcript = mountTranscript(1000)
    const { clicks } = mountRunPanel()
    handle = installLiveSuggestions({ scenarios })
    required(document.querySelector<HTMLButtonElement>(SEND), 'send').click()
    addApproval(transcript)
    await tick(400)
    expect(clicks()).toBe(1)
    required(document.querySelector<HTMLButtonElement>(SEND), 'send').click()
    await tick(300)
    expect(clicks()).toBe(1)
    addApproval(transcript)
    await tick(400)
    expect(clicks()).toBe(2)
  })

  it('does not press a tab that is already shown, and does nothing for an Ask turn without a run panel', async () => {
    mountChat()
    const transcript = mountTranscript(1000)
    const { clicks } = mountRunPanel(true)
    handle = installLiveSuggestions({ scenarios })
    required(document.querySelector<HTMLButtonElement>(SEND), 'send').click()
    addApproval(transcript)
    await tick(400)
    expect(clicks()).toBe(0)
    document.querySelector('.verevon-chat-run-panel')?.remove()
    required(document.querySelector<HTMLButtonElement>(SEND), 'send').click()
    addApproval(transcript)
    await tick(400)
    expect(clicks()).toBe(0)
  })

  it('stops watching after dispose', async () => {
    mountChat()
    const transcript = mountTranscript(1000)
    const { clicks } = mountRunPanel()
    handle = installLiveSuggestions({ scenarios })
    required(document.querySelector<HTMLButtonElement>(SEND), 'send').click()
    handle.dispose()
    addApproval(transcript)
    await tick(400)
    expect(clicks()).toBe(0)
  })
})
