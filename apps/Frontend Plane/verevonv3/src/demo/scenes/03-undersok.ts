/**
 * Scene 03 · Undersøk · «Agent som undersøker»
 *
 * Capability sold: autonomous investigation under control, started by context.
 * Verevon notices a fictional invoice notice (invoice 20471 to Nordlys Handel
 * AS deviates from the agreed price), offers one concrete suggestion on the
 * start screen, and on a click fills the composer (it never sends). The
 * person sends the question; Verevon splits the check into three parallel
 * sub-tasks, reads the invoice in the accounting system through the browser
 * agent, sets agreement, order and invoice side by side, explains the
 * deviation with traceable sources and prepares a credit note. The last beat
 * is human approval: nothing goes to the customer before a person says yes.
 * The story is a customer problem (an invoice written at list price), not a
 * fault in Verevon.
 *
 * FALLBACK LADDER (CONTRACT §3): RUNG 1, the highest. Real panels, all fed
 * through the transports they already use, no component edited:
 *   - ChatLiveRunPanel (live split) hosting ChatWideResearchPanel
 *     («Bred forskning · 3 deloppgaver»): `subagent_attached` /
 *     `subagent_stopped` on `GET /api/v1/runs/:run/events`
 *     (run-console-client `streamRunEvents`, both panels subscribe).
 *   - BrowserChrome (compact): one `browser_action_dispatched` +
 *     `browser_observation_received` with a `screenshot_ref`; the `<img>` the
 *     app points at the BFF artifact route is mapped by the runtime to
 *     `fixtures/assets/faktura-20471.png`.
 *   - ArtifactsPanel (Resultat) with the comparison document, re-emitted with
 *     one more block each time, through `artifact` chat frames.
 *   - ApprovalRequests card «Godkjenning · Opprett kreditnota» under the
 *     answer: `run_paused_for_approval` on the run stream makes the live
 *     panel ask the turn to re-read `GET .../approvals` (the real HITL path).
 *
 * Viewport: 1024x640 CSS px (CONTRACT amendment A, `scene-config.json`
 * `scenes["03-undersok"]`), captured at device scale factor 2.5, as scene 02.
 *
 * Round 5 (CONTRACT.md amendment F): the film starts on the product's own
 * centred start screen. Greeting and composer are centred, there is no work
 * surface and no empty column; the live split opens, growing from the right
 * edge, only when the run has sub-tasks to show. The general welcome text is
 * for a fresh start with nothing to report, so here a suggestion card
 * (suggestions.ts) stands in its place.
 *
 * Timeline (scene ms; PLAN §2 beat sheet, re-timed for the extra click):
 *   0–700      Static start screen: brand mark, «God dag, Ida», the
 *              suggestion card (what happened, source, time), the empty
 *              composer with «Utfør» selected (the real «Arbeidsmodus»
 *              segment, clicked in `prepare`). Cursor hidden, parked.
 *   700–1300   Cursor fades in and glides to the suggestion.
 *   1300–1380  Press and release: the composer is FILLED. Nothing is sent.
 *   1800–2190  Cursor glides to «Send melding»; 2200 press, 2280 release.
 *   2290       The send: the greeting and card leave as a 320 ms cross-fade,
 *              the composer glides to its dock, the thread header and the
 *              question rise in. 2300 `connected` + `title` + `tool_call`
 *              «3_deloppgaver»: the thinking line reads «Tenker · Kjører 3
 *              deloppgaver» and the live split grows in from the right.
 *   2550–2790  The three sub-task rows rise 90 ms apart, all «Kjører».
 *   3300       Assistant line «Jeg sammenholder avtalen, ordren og
 *              fakturaen.» (one chunk, 300 ms fade-rise).
 *   3500       Browser step: the invoice frame with the price highlighted.
 *   3800–4400  Sub-tasks flip to «Ferdig», 300 ms apart.
 *   4750–4800  The finding block. 4800 the comparison claims Resultat: the
 *              live split stays fully visible until this swap and then
 *              cross-dissolves out over 200 ms (installRunPanelCrossfade)
 *              while the comparison slides in 16 px; rows and sections rise
 *              as they arrive (4800, 5050, 5300, 5550, 5800).
 *   5900       `done`. 6050 the run pauses for approval → the «Opprett
 *              kreditnota» card rises 8 px over 340 ms at the end of the
 *              answer.
 *   6450–7000  Cursor fades in ~45 px from «Godkjenn», glides 400 ms onto it.
 *   7000–8000  Real `:hover` on «Godkjenn», held. Never clicked.
 *
 * Film-only presentation lives in 03-undersok.css, scoped to
 * `html.verevon-scene--03-undersok`; no component or product CSS is edited.
 * Elements the product rebuilds on every streamed update (document lines,
 * answer paragraphs, sub-task rows, the approval card) get their entrance
 * from keyed Web Animations (`installEntranceDirector`), so a re-mount
 * continues the motion instead of replaying or cutting it.
 *
 * Deviations from the beat sheet, and why (all forced by real product
 * behaviour):
 *   - Status line: the real thinking line always starts with «Tenker» and
 *     names the turn's running tool through «Kjører {verktøy}» with the first
 *     letter capitalised. The fan-out tool is named «3_deloppgaver», so it
 *     reads «Tenker · Kjører 3 deloppgaver» rather than «Deler opp oppgaven».
 *   - «Utfør» is a plan-mode turn in the product, so a finished turn also
 *     derives the rung-only plan gate («Dette var en plan …», a second,
 *     disabled «Godkjenn» until a 12-character justification). The film hides
 *     that block (03-undersok.css) so the run's own approval card is the one
 *     gate on screen.
 *   - The Work canvas the run summons is closed with its own «Lukk
 *     arbeidsflaten» button the moment it mounts (a MutationObserver in
 *     `prepare`, before any paint), so the run is watched in the live split
 *     and the first artifact can claim Resultat. Work renders hard-coded
 *     English lifecycle rows while a turn streams.
 *   - The comparison is a list, not a table: the real table renderer is 620 px
 *     wide minimum and would scroll sideways inside the 440 px Resultat
 *     surface.
 *   - «Opprett kreditnota» is the approval card's title, its button reads
 *     «Godkjenn», and it sits under the answer: the Resultat viewer has no
 *     button surface (the same mapping as scene 02, CONTRACT amendment D(3)).
 *   - The thread header shows the title only (the «N meldinger» count is
 *     hidden): the product counts the assistant placeholder from the send.
 */
import { FJORDFORM_NOW_ISO } from '../fixtures/fjordform'
import {
  undersokAnswerChunks,
  undersokApproval,
  undersokArtifactId,
  undersokArtifactMarkdown,
  undersokArtifactTitle,
  undersokBatchId,
  undersokBrowserStep,
  undersokFindingChunks,
  undersokImages,
  undersokRequestId,
  undersokRunDetail,
  undersokRunId,
  undersokSubtasks,
  undersokSuggestion,
  undersokThreadId,
  undersokTitle,
  undersokToolName,
} from '../fixtures/fjordform-undersok'
import { installCanvasOpen, installComposerTravel, isSendPress } from '../choreography'
import type { SceneDefinition, SceneStreamFrame, SceneTarget } from '../scene-types'
import { installPromptSuggestions } from '../suggestions'
import './03-undersok.css'

const IS_SCENE_03 = typeof window !== 'undefined' && window.location.pathname.startsWith('/scene/03-undersok')

/**
 * The fictional clock (CONTRACT §0: Friday 9 October 2026 12:00 Oslo).
 *
 * The empty state greets by the hour it reads from `new Date()` at mount
 * («God morgen» before 10:00, «God dag» until 17:00). Before the harness
 * pauses virtual time `Date` is the real wall clock, so without this the
 * opening frame (and the loop seam) changed with the time of capture. The
 * clock advances with `performance.now()`, which follows virtual time under
 * the harness. Evaluated when ScenePage loads the scene registry, before
 * ChatPage mounts; acts only on /scene/03-undersok. Same patch as scene 02's,
 * guarded so the two never stack.
 */
function installScene03Clock(): void {
  const RealDate = Date
  if ((RealDate as unknown as { __verevonSceneFixed?: boolean }).__verevonSceneFixed) return
  const fixedMs = RealDate.parse(FJORDFORM_NOW_ISO)
  const perfAtInstall = performance.now()
  const sceneNow = () => fixedMs + (performance.now() - perfAtInstall)
  class SceneDate extends RealDate {
    constructor(...args: unknown[]) {
      if (args.length === 0) super(sceneNow())
      else super(...(args as [string | number | Date]))
    }
    static override now(): number {
      return Math.floor(sceneNow())
    }
  }
  Object.defineProperty(SceneDate, '__verevonSceneFixed', { value: true })
  window.Date = new Proxy(SceneDate, {
    apply: () => new SceneDate().toString(),
  }) as unknown as DateConstructor
}

/**
 * The product swaps the welcome state for the thread in one frame when the
 * first message is sent. For the film the greeting leaves softly instead, and
 * only once the user has acted: on the press the greeting block is copied
 * with its real classes and measured; when the real empty state unmounts (the
 * send), the copy is placed at the same spot and fades out 8 px upwards over
 * 320 ms (03-undersok.css `.v03-welcome-ghost`). Inert, aria-hidden, removed
 * when its fade ends. No component is touched.
 */
function installWelcomeCrossfade(): void {
  let pending: { node: HTMLElement; rect: DOMRect } | null = null
  let placed = false
  const capture = (event: Event) => {
    if (placed || !isSendPress(event)) return
    const heading = document.querySelector<HTMLElement>('.verevon-chat-empty__heading')
    if (!heading) return
    pending = { node: heading.cloneNode(true) as HTMLElement, rect: heading.getBoundingClientRect() }
  }
  // pointerdown from the harness's CDP mouse; click from ?autopilot=1.
  document.addEventListener('pointerdown', capture, true)
  document.addEventListener('click', capture, true)
  const observer = new MutationObserver(() => {
    if (!pending || placed) return
    if (document.querySelector('.verevon-chat-empty')) return
    const host = document.querySelector<HTMLElement>('.verevon-chat-section')
    if (!host) return
    const ghost = document.createElement('div')
    ghost.className = 'v03-welcome-ghost'
    ghost.setAttribute('aria-hidden', 'true')
    ghost.inert = true
    ghost.style.left = `${pending.rect.left}px`
    ghost.style.top = `${pending.rect.top}px`
    ghost.style.width = `${pending.rect.width}px`
    ghost.style.height = `${pending.rect.height}px`
    ghost.appendChild(pending.node)
    ghost.addEventListener('animationend', () => ghost.remove(), { once: true })
    host.appendChild(ghost)
    pending = null
    placed = true
    observer.disconnect()
  })
  const start = () => observer.observe(document.body, { childList: true, subtree: true })
  if (document.body) start()
  else document.addEventListener('DOMContentLoaded', start, { once: true })
}

/**
 * Entrances that survive the product's re-renders.
 *
 * The real markdown renderer rebuilds its blocks on every streamed update
 * (each `artifact` frame re-emits the whole document, each `chunk` the whole
 * answer), the research panel rebuilds a sub-task row when its status flips,
 * and the approval list re-renders when the turn re-reads approvals. A CSS
 * animation on such an element replays or is cut on every re-mount. Here each
 * entrance is a Web Animation keyed by the element's identity, started when
 * that identity first appears; a re-mounted node gets the same animation
 * advanced by the time elapsed since the first appearance, so only what is
 * new moves (diff line 3 rises at 5.3 s while lines 1-2 stay put).
 *
 * Time comes from `document.timeline.currentTime` (the frame clock the
 * capture harness drives exactly), never from `performance.now()`.
 */
type Entrance = {
  selector: string
  key: (element: Element) => string
  keyframes: Keyframe[]
  duration: number
  delay?: (element: Element) => number
}

/** The contract's 1 - (1 - p)^3: a gentler first frame than the expo curve, no late snap. */
const EASE_OUT = 'cubic-bezier(0.33, 1, 0.68, 1)'
const fade: Keyframe[] = [{ opacity: 0 }, { opacity: 1 }]
const rise = (px: number): Keyframe[] => [
  { opacity: 0, transform: `translateY(${px}px)` },
  { opacity: 1, transform: 'translateY(0)' },
]
const textKey = (element: Element) => (element.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 60)
const siblingIndex = (element: Element) => (element.parentElement ? Array.from(element.parentElement.children).indexOf(element) : 0)

const ENTRANCES: Entrance[] = [
  // Sub-task rows: 10 px rise, 90 ms apart, after the panel's own slide.
  {
    selector: '.verevon-wide-research__card',
    key: (e) => `task:${textKey(e.querySelector('.verevon-wide-research__goal') ?? e)}`,
    keyframes: rise(10),
    duration: 320,
    delay: (e) => 90 + 90 * siblingIndex(e),
  },
  // A status flip («Kjører» → «Ferdig») rebuilds the row; the new pill fades
  // in over 240 ms instead of swapping in one frame.
  {
    selector: '.verevon-wide-research__pill',
    key: (e) => `pill:${textKey(e.closest('.verevon-wide-research__card')?.querySelector('.verevon-wide-research__goal') ?? e)}:${textKey(e)}`,
    keyframes: fade,
    duration: 240,
  },
  // The browser agent's evidence frame.
  { selector: '.verevon-chat-run-panel .knowledge-browser-chrome--compact', key: () => 'browser', keyframes: rise(8), duration: 400 },
  // Answer paragraphs: the first line at 2.6 s, the finding at 4.6 s.
  {
    selector: '.verevon-chat-message--assistant .verevon-chat-markdown > *',
    key: (e) => `answer:${siblingIndex(e)}`,
    keyframes: rise(6),
    duration: 300,
  },
  // Comparison document (Resultat): each row and section rises as it arrives;
  // a row's source reference fades in just after its line.
  {
    selector: '.verevon-chat-workspace-canvas .verevon-chat-artifact-document li',
    key: (e) => `li:${textKey(e)}`,
    keyframes: rise(10),
    duration: 340,
  },
  {
    selector: '.verevon-chat-workspace-canvas .verevon-chat-artifact-document li > em',
    key: (e) => `ref:${textKey(e.parentElement ?? e)}`,
    keyframes: fade,
    duration: 320,
    delay: () => 200,
  },
  {
    selector: '.verevon-chat-workspace-canvas .verevon-chat-artifact-document h2',
    key: (e) => `h2:${textKey(e)}`,
    keyframes: rise(10),
    duration: 340,
  },
  // The approval card: the climax, 8 px rise over 340 ms, transform/opacity only.
  { selector: '.verevon-chat-approval', key: () => 'approval', keyframes: rise(8), duration: 340 },
]

function installEntranceDirector(): void {
  const started = new Map<string, number>()
  const now = () => {
    const value = document.timeline.currentTime
    return typeof value === 'number' ? value : Number(value ?? 0)
  }
  const apply = (element: Element, entrance: Entrance) => {
    if (!(element instanceof HTMLElement)) return
    const key = entrance.key(element)
    const delay = entrance.delay?.(element) ?? 0
    const total = delay + entrance.duration
    const at = now()
    const first = started.get(key)
    const elapsed = first === undefined ? 0 : at - first
    if (first === undefined) started.set(key, at)
    if (elapsed >= total) return
    const animation = element.animate(entrance.keyframes, {
      duration: entrance.duration,
      delay,
      easing: EASE_OUT,
      fill: 'backwards',
    })
    if (elapsed > 0) animation.currentTime = elapsed
  }
  const visit = (node: Node) => {
    if (!(node instanceof Element)) return
    for (const entrance of ENTRANCES) {
      if (node.matches(entrance.selector)) apply(node, entrance)
      for (const child of node.querySelectorAll(entrance.selector)) apply(child, entrance)
    }
  }
  const observer = new MutationObserver((records) => {
    for (const record of records) for (const node of record.addedNodes) visit(node)
  })
  const start = () => observer.observe(document.body, { childList: true, subtree: true })
  if (document.body) start()
  else document.addEventListener('DOMContentLoaded', start, { once: true })
}

/**
 * When the turn settles the product removes the orange «Stopp svar» control
 * in one frame (the idle send button that replaces it eases in, 03-undersok.css).
 * For the film the stop control leaves with a 240 ms fade instead: while it is
 * on screen its look is snapshotted (computed styles inlined onto a deep
 * clone, so the copy does not depend on ancestor selectors) with its rect;
 * once the product removes it, the copy is placed at the same rect and fades
 * out. Inert, aria-hidden, removed when faded. The composer never moves, so
 * the rect is stable.
 */
const STOP_CONTROL = 'button.dashboard-composer-submit-button[aria-label="Stopp svar"]'

function inlineComputed(source: Element, target: Element): void {
  if (!(target instanceof HTMLElement || target instanceof SVGElement)) return
  const style = getComputedStyle(source)
  for (const property of Array.from(style)) {
    target.style.setProperty(property, style.getPropertyValue(property))
  }
  // Snapshot of a resting state: no running animation or transition on the copy.
  target.style.setProperty('animation', 'none')
  target.style.setProperty('transition', 'none')
  const sourceChildren = Array.from(source.children)
  const targetChildren = Array.from(target.children)
  sourceChildren.forEach((child, index) => {
    const copy = targetChildren[index]
    if (copy) inlineComputed(child, copy)
  })
}

function installStopControlGhost(): void {
  let tracked: { element: HTMLElement; ghost: HTMLElement; rect: DOMRect } | null = null
  let done = false
  const observer = new MutationObserver(() => {
    if (done) return
    if (tracked && !tracked.element.isConnected) {
      const { ghost, rect } = tracked
      ghost.style.setProperty('position', 'fixed')
      ghost.style.setProperty('left', `${rect.left}px`)
      ghost.style.setProperty('top', `${rect.top}px`)
      ghost.style.setProperty('margin', '0')
      ghost.style.setProperty('z-index', '40')
      ghost.style.setProperty('pointer-events', 'none')
      ghost.style.setProperty('animation', 'v03-fade-out 240ms cubic-bezier(0.33, 1, 0.68, 1) forwards')
      ghost.addEventListener('animationend', () => ghost.remove(), { once: true })
      document.body.appendChild(ghost)
      done = true
      observer.disconnect()
      return
    }
    const stop = document.querySelector<HTMLElement>(STOP_CONTROL)
    // Re-snapshotted on every update while it is on screen, so the copy holds
    // its settled look (not a mid-entrance frame).
    if (!stop) return
    const ghost = stop.cloneNode(true) as HTMLElement
    inlineComputed(stop, ghost)
    ghost.removeAttribute('class')
    ghost.removeAttribute('id')
    ghost.setAttribute('aria-hidden', 'true')
    ghost.removeAttribute('aria-label')
    ghost.inert = true
    tracked = { element: stop, ghost, rect: stop.getBoundingClientRect() }
  })
  const start = () => observer.observe(document.body, { childList: true, subtree: true })
  if (document.body) start()
  else document.addEventListener('DOMContentLoaded', start, { once: true })
}

/**
 * Live split to Resultat, hand-over. When the first artifact arrives the
 * product mounts the Resultat canvas beside the live split and hides the split
 * with an inline `display: none`, in one frame. Fading the split out BEFORE
 * that left the right column almost empty for three frames (its content fell to
 * about 1 %). Instead the split stays at full opacity until the swap, and at
 * the swap a copy of it (its DOM, taken in the same task before the next paint)
 * is laid over the same 440 px slot as a ghost that fades out
 * (03-undersok.css `.v03-run-ghost`) while the comparison slides in: a true
 * cross-dissolve of the two real surfaces. The ghost is inert, aria-hidden and
 * removed when its fade ends. No component is touched.
 */
function installRunPanelCrossfade(): void {
  let done = false
  const ghostOf = (panel: HTMLElement) => {
    if (done) return
    const page = panel.closest<HTMLElement>('.verevon-chat-page')
    // Only when the document takes the slot (not when the panel is closed).
    if (!page || !page.querySelector(':scope > .verevon-chat-workspace-canvas')) return
    done = true
    const ghost = panel.cloneNode(true) as HTMLElement
    ghost.style.removeProperty('display')
    ghost.classList.add('v03-run-ghost')
    ghost.setAttribute('aria-hidden', 'true')
    ghost.removeAttribute('aria-label')
    ghost.inert = true
    ghost.addEventListener('animationend', () => ghost.remove(), { once: true })
    page.appendChild(ghost)
  }
  const observer = new MutationObserver((records) => {
    for (const record of records) {
      const target = record.target
      if (
        record.type === 'attributes' &&
        target instanceof HTMLElement &&
        target.classList.contains('verevon-chat-run-panel') &&
        target.style.display === 'none'
      ) ghostOf(target)
    }
  })
  const start = () => observer.observe(document.body, { attributes: true, attributeFilter: ['style'], subtree: true })
  if (document.body) start()
  else document.addEventListener('DOMContentLoaded', start, { once: true })
}

/**
 * Set-up that must exist before the chat page renders (this module is
 * evaluated when ScenePage loads the scene registry, before ChatPage mounts).
 * Acts only on /scene/03-undersok.
 */
function installScene03Setup(): void {
  if (!IS_SCENE_03) return
  // Film-only presentation rules in 03-undersok.css are scoped to this class.
  document.documentElement.classList.add('verevon-scene--03-undersok')
  installScene03Clock()
  // The suggestion card that stands in for the welcome text, the welcome
  // cross-fade, the composer's glide to its dock and the live split's growth
  // from the right edge (choreography.ts).
  installPromptSuggestions({
    label: undersokSuggestion.label,
    items: [{ text: undersokSuggestion.text, source: undersokSuggestion.source, time: undersokSuggestion.time, icon: 'receipt' }],
  })
  installWelcomeCrossfade()
  installComposerTravel()
  installCanvasOpen('.verevon-chat-run-panel')
  installRunPanelCrossfade()
  installEntranceDirector()
  installStopControlGhost()
  // Pressing «Send melding» would focus it, and the composer's :focus-within
  // ring flashed for the frames between press and send. A film has no
  // keyboard user, so the press does not move focus (default action of
  // mousedown only; the click and the real submit are untouched).
  document.addEventListener(
    'mousedown',
    (event) => {
      const target = event.target instanceof Element ? event.target.closest('button[aria-label="Send melding"]') : null
      if (target) event.preventDefault()
    },
    true,
  )
}

installScene03Setup()

/** The suggestion card on the start screen (suggestions.ts). */
const SUGGESTION: SceneTarget = { selector: '.v-suggestion', pin: true }
const SEND_BUTTON: SceneTarget = { name: 'Send melding', role: 'button', pin: true }
const APPROVE_BUTTON: SceneTarget = { selector: '.verevon-chat-approval__approve', pin: true }
const DO_MODE: SceneTarget = { name: 'Utfør', role: 'button' }

/** Mouse release on the suggestion: its click fills the composer (nothing is sent). */
const CHOOSE_AT = 1_380
const SEND_AT = 2_280

/**
 * Entrance animations in 03-undersok.css that are CSS (they mount once in the
 * normal flow) play once per element. Should the app re-create one of them,
 * the first element seen for a key keeps its entrance; a later element with
 * the same key gets `data-v03-settled`, which the CSS reads as «no entrance».
 * Runs in the MutationObserver callback, a microtask before the next paint.
 */
const ENTRANCE_KEYS: Array<{ selector: string; key: (element: HTMLElement) => string }> = [
  { selector: '.verevon-chat-header', key: () => 'header' },
  { selector: '.verevon-chat-message--user', key: () => 'user' },
  { selector: '.verevon-chat-message--assistant', key: () => 'assistant' },
  { selector: '.verevon-chat-run-panel', key: () => 'run-panel' },
  { selector: '.verevon-chat-workspace-canvas', key: () => 'canvas' },
]
const seenEntranceKeys = new Set<string>()
const seenEntranceElements = new WeakSet<Element>()

function settleRemounts() {
  for (const { selector, key } of ENTRANCE_KEYS) {
    for (const element of document.querySelectorAll<HTMLElement>(selector)) {
      if (seenEntranceElements.has(element)) continue
      seenEntranceElements.add(element)
      // The hand-over ghost (installRunPanelCrossfade) is a copy that must run
      // its own fade-out; «settled» would switch every animation off and leave
      // it over the canvas for good.
      if (element.classList.contains('v03-run-ghost')) continue
      const id = `${selector}::${key(element)}`
      if (seenEntranceKeys.has(id)) element.dataset.v03Settled = ''
      else seenEntranceKeys.add(id)
    }
  }
}

/** Scene time at which the chat POST arrives (the release + the submit path). */
const POST_AT = 2_290

/** A frame at scene time `t` (ms), on the chat POST's clock. */
function at(t: number, event: string, data: Record<string, unknown>): SceneStreamFrame {
  return { at: Math.max(0, t - POST_AT), event, data }
}

function chunk(t: number, delta: string): SceneStreamFrame {
  return at(t, 'chunk', { delta, request_id: undersokRequestId })
}

function artifact(t: number, stage: 1 | 2 | 3 | 4 | 5): SceneStreamFrame {
  return at(t, 'artifact', {
    id: undersokArtifactId,
    kind: 'markdown',
    title: undersokArtifactTitle,
    content: undersokArtifactMarkdown(stage),
    version: 1,
  })
}

/** The turn's one tool call: the fan-out into three sub-tasks. Names the thinking line's step. */
const SPLIT_TOOL_ID = 'call_del_opp_20471'

const chatFrames: SceneStreamFrame[] = [
  at(POST_AT + 10, 'connected', { ok: true, request_id: undersokRequestId, thread_id: undersokThreadId, model_used: 'verevon-balance', run_id: undersokRunId }),
  // With `connected`; the header's entrance starts 60 ms after the send, so
  // its first visible frame already carries this title.
  at(POST_AT + 10, 'title', { title: undersokTitle, request_id: undersokRequestId }),
  at(POST_AT + 10, 'tool_call', { id: SPLIT_TOOL_ID, name: undersokToolName, args: { deloppgaver: 3 } }),
  at(3_300, 'tool_result', { id: SPLIT_TOOL_ID, status: 'completed', output: '3 deloppgaver startet' }),
  // Each paragraph arrives whole and rises in (no mid-sentence chunk stutter).
  chunk(3_300, undersokAnswerChunks.join('')),
  chunk(4_750, undersokFindingChunks.join('')),
  // Agreement, order, invoice (three rows), then the deviation, then the proposal.
  artifact(4_800, 1),
  artifact(5_050, 2),
  artifact(5_300, 3),
  artifact(5_550, 4),
  artifact(5_800, 5),
  // `done` before the gate: the turn settles first, so the approval card is
  // the last thing to enter.
  at(5_900, 'done', { request_id: undersokRequestId, model_used: 'verevon-balance', stop_reason: 'end_turn' }),
].sort((a, b) => a.at - b.at)

const runFrames: SceneStreamFrame[] = [
  // All three attach with the run; the rows' 90 ms stagger is the keyed
  // entrance above, so the panel's layout settles in one invisible frame.
  ...undersokSubtasks.map((task) => at(POST_AT + 10, 'subagent_attached', {
    parent_run_id: undersokRunId,
    child_run_id: task.childRunId,
    role: task.role,
    goal_summary: task.goal,
    batch_id: undersokBatchId,
  })),
  at(3_500, 'browser_action_dispatched', {
    run_id: undersokRunId,
    action_id: undersokBrowserStep.actionId,
    action_type: undersokBrowserStep.actionType,
    reason: undersokBrowserStep.reason,
  }),
  at(3_500, 'browser_observation_received', {
    run_id: undersokRunId,
    action_id: undersokBrowserStep.actionId,
    status: 'success',
    page_title: undersokBrowserStep.pageTitle,
    screenshot_ref: undersokBrowserStep.screenshotRef,
  }),
  ...undersokSubtasks.map((task, index) => at(3_800 + index * 300, 'subagent_stopped', {
    parent_run_id: undersokRunId,
    child_run_id: task.childRunId,
    status: 'completed',
  })),
  // The gate: the run pauses for a person. The live panel hears it and asks
  // the chat turn to re-read the run's approvals (the real HITL path).
  at(6_050, 'run_paused_for_approval', { run_id: undersokRunId, approval_id: undersokApproval.id }),
]

export const scene03Undersok: SceneDefinition = {
  id: '03-undersok',
  durationMs: 8_000,
  // Empty: the start screen is the product's own. The click on the suggestion
  // card fills the composer (suggestions.ts).
  composerPrefill: '',
  readySelectors: ['.v-suggestion'],
  async prepare({ click, waitFor }) {
    document.documentElement.classList.add('verevon-scene--03-undersok')
    // The evidence frame is decoded before frame 0, so its first paint at
    // 3.5 s never waits on the network (or on paused virtual time).
    await Promise.all(Object.values(undersokImages).map(async (src) => {
      const image = new Image()
      image.src = src
      await image.decode()
    }))
    // This checkout uses Planmodus in place of the older «Utfør» segment.
    if (document.querySelector('.dashboard-composer-intent-segment button')) {
      await click(DO_MODE)
      await waitFor(() => document.querySelector('.dashboard-composer-intent-segment button.is-active')?.textContent?.trim() === 'Utfør' ? true : null)
    } else {
      await click({ selector: 'button[aria-label^="Planmodus"]' })
      await waitFor(() => document.querySelector('button[aria-label^="Planmodus"]')?.getAttribute('aria-pressed') === 'true' ? true : null)
    }
    // The run summons the Work canvas; close it with its own button the
    // moment it mounts (before paint), so the run is watched in the live
    // split. Kept for the whole scene, so a later summon is closed the same way.
    const observer = new MutationObserver(() => {
      const close = document.querySelector<HTMLButtonElement>('.verevon-chat-run-panel--workspace .verevon-chat-run-panel__toggle')
      if (close) close.click()
      settleRemounts()
    })
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['class'] })
  },
  stream: { path: '/api/v1/chat/stream', frames: chatFrames, closeAfterMs: 40 },
  runStreams: [{ runId: undersokRunId, frames: runFrames }],
  images: undersokImages,
  routes: [
    { method: 'GET', match: new RegExp(`^/api/v1/agents/runs/${undersokRunId}$`), body: undersokRunDetail },
    { method: 'GET', match: new RegExp(`^/api/v1/runs/${undersokRunId}/events/replay`), body: { run_id: undersokRunId, events: [], truncated: false } },
    { method: 'GET', match: new RegExp(`^/api/v1/runs/${undersokRunId}/watchers$`), body: { watching: false } },
    { method: 'GET', match: new RegExp(`^/api/v1/orchestration/runs/${undersokRunId}/approvals$`), body: { approvals: [undersokApproval] } },
    // Read once by the Work tree (RunPlanPanel) as it mounts, before it is closed.
    { method: 'GET', match: new RegExp(`^/api/v1/orchestration/runs/${undersokRunId}/plans$`), body: { plans: [] } },
    { method: 'GET', match: new RegExp(`^/api/v1/orchestration/threads/${undersokThreadId}/todos`), body: { todos: [] } },
    { method: 'GET', match: new RegExp(`^/api/v1/orchestration/threads/${undersokThreadId}/lineage$`), body: { nodes: [], edges: [] } },
  ],
  hoverTarget: APPROVE_BUTTON,
  pointer: [
    // First 0.5 s static, cursor hidden and parked off the app card (a hidden
    // pointer over the card would put a real :hover state into the resting
    // frames, and into the frames the loop dissolves into).
    { at: 0, x: 4, y: 634, visible: false, pressed: false },
    // It fades in a little below-left of the suggestion, rests, then glides
    // onto it (500 ms ease-out).
    { at: 700, target: SUGGESTION, nudge: { x: -80, y: 80 }, visible: true, pressed: false },
    { at: 800, target: SUGGESTION, nudge: { x: -80, y: 80 }, visible: true, pressed: false, ease: 'hold' },
    { at: 1_300, target: SUGGESTION, offset: { x: 0.62, y: 0.55 }, visible: true, pressed: false },
    // Press and release: the real click fills the composer.
    { at: 1_310, target: SUGGESTION, offset: { x: 0.62, y: 0.55 }, visible: true, pressed: true },
    { at: CHOOSE_AT, target: SUGGESTION, offset: { x: 0.62, y: 0.55 }, visible: true, pressed: false },
    // A beat to read the filled composer, then one glide to «Send melding»
    // (390 ms ease-out), press, release.
    { at: 1_800, target: SUGGESTION, offset: { x: 0.62, y: 0.55 }, visible: true, pressed: false, ease: 'hold' },
    { at: 2_190, target: SEND_BUTTON, visible: true, pressed: false },
    { at: 2_200, target: SEND_BUTTON, visible: true, pressed: true },
    { at: SEND_AT, target: SEND_BUTTON, visible: true, pressed: false },
    // Fades out where it clicked.
    { at: 2_400, target: SEND_BUTTON, visible: false, pressed: false, ease: 'hold' },
    // Parked off the app card once invisible, so no hover state is held
    // anywhere while the answer builds.
    { at: 2_520, x: 4, y: 634, visible: false, pressed: false },
    // Fades in ~45 px from «Godkjenn», then one 400 ms glide onto it.
    { at: 6_450, target: APPROVE_BUTTON, nudge: { x: -34, y: 30 }, visible: true, pressed: false },
    { at: 6_600, target: APPROVE_BUTTON, nudge: { x: -34, y: 30 }, visible: true, pressed: false, ease: 'hold' },
    { at: 7_000, target: APPROVE_BUTTON, offset: { x: 0.6, y: 0.7 }, visible: true, pressed: false },
    { at: 8_000, target: APPROVE_BUTTON, offset: { x: 0.6, y: 0.7 }, visible: true, pressed: false, ease: 'hold' },
  ],
  events: [
    { at: 0, kind: 'note', note: 'frame 0: centred start screen, suggestion card, empty composer with Utfør, no work surface, cursor hidden' },
    { at: 800, kind: 'cursor', note: 'cursor glides to the suggestion' },
    { at: CHOOSE_AT, kind: 'ui', note: 'click the suggestion: fills the composer (autopilot only)', action: 'click', target: SUGGESTION },
    { at: 1_800, kind: 'cursor', note: 'cursor glides to Send melding' },
    { at: SEND_AT, kind: 'ui', note: 'click Send melding (autopilot only)', action: 'click', target: { name: 'Send melding', role: 'button' } },
    { at: 2_550, kind: 'sse', note: 'three sub-tasks attach; Bred forskning rows grow in', event: 'subagent_attached' },
    { at: 3_300, kind: 'sse', note: 'answer line', event: 'chunk' },
    { at: 3_500, kind: 'sse', note: 'browser step with the invoice frame', event: 'browser_observation_received' },
    { at: 3_800, kind: 'sse', note: 'sub-tasks flip to Ferdig, 300 ms apart', event: 'subagent_stopped' },
    { at: 4_800, kind: 'sse', note: 'finding block; comparison opens Resultat, rows 250 ms apart', event: 'artifact' },
    { at: 6_050, kind: 'sse', note: 'run pauses for approval; Opprett kreditnota card', event: 'run_paused_for_approval' },
    { at: 6_600, kind: 'cursor', note: 'cursor glides to Godkjenn' },
    { at: 7_000, kind: 'hover', note: 'Godkjenn hovered, held to the end', target: APPROVE_BUTTON },
    { at: 7_000, kind: 'ui', note: 'hover Godkjenn (autopilot only)', action: 'hover', target: APPROVE_BUTTON },
  ],
}

// Self-accepting: editing this scene (or its fixtures) must not full-reload
// other agents' pages that play scene 02 from the same registry.
if (import.meta.hot) import.meta.hot.accept()
