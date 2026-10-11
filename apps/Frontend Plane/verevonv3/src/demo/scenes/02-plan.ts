/**
 * Scene 02 · Plan · «Svar med kilder»
 *
 * Capability sold: a context-aware assistant. Verevon notices a relevant event
 * (a fictional delay notice from Bring about the Nordlys delivery), offers one
 * concrete suggestion on the start screen, and on a click fills the composer
 * (it never sends). The person sends the question; Verevon shows the status
 * with its sources, proposes the next steps and waits for approval before
 * anything is sent on. Tracking status comes from Bring, not from the order
 * system.
 *
 * Captured at 1024x640 CSS px (scene-capture `--viewport 1024x640`, see
 * src/demo/README.md "Viewport").
 *
 * Round 5 (CONTRACT.md amendment F): the film starts on the product's own
 * centred start screen. Greeting and composer are centred, there is no work
 * surface and no empty column; the surface opens only when there is evidence
 * to show. The general welcome text is for a fresh start with nothing to
 * report, so here a suggestion card (suggestions.ts) stands in its place.
 *
 * Timeline (scene ms; PLAN §2 beat sheet, re-timed for the extra click):
 *
 *   0–700      Static start screen: brand mark, «God dag, Ida», the
 *              suggestion card (what happened, source, time), the empty
 *              composer. Cursor hidden and parked off the app card.
 *   700–1300   Cursor fades in and glides to the suggestion.
 *   1300–1380  Press and release: the composer is FILLED with the question
 *              (the product's starter behaviour). Nothing is sent.
 *   1800–2190  Cursor glides to «Send melding»; 2200 press, 2280 release.
 *   2290       The send. The greeting and card leave as a 320 ms cross-fade
 *              (a frozen copy, installWelcomeCrossfade), the composer glides
 *              to its dock (installComposerTravel), the thread header and the
 *              question rise in.
 *   2560–2800  Three `citation` frames, 120 ms apart. The real controller
 *              opens the Kilder surface on the first one; it grows from the
 *              right edge (installCanvasOpen) while the conversation narrows.
 *   3300–3500  The assistant line streams. The sources hold until the swap.
 *   3800–3860  The first step joins Arbeid; the artifact opens Resultat. The
 *              removed Kilder body cross-dissolves out (installKilderCrossfade)
 *              while the status document rises in.
 *   3860–4760  Four findings, 300 ms apart, each with its source reference.
 *   5000–5450  Draft block under the answer; «Klar for neste steg» with three
 *              plan rows (110 ms apart).
 *   5700–5860  The third step pauses for approval; the real controller fetches
 *              the run's approvals; the approval card rises in.
 *   6300–6950  Cursor fades in just below the card and glides to «Godkjenn».
 *   6950–8000  Real `:hover` on «Godkjenn», held. Never clicked.
 *
 * How the beat sheet maps onto the REAL chat workspace (CONTRACT.md amendment
 * D, still in force): the sources are cards in the Kilder surface (not chips
 * under the bubble), the status line is the product's «Tenker», the approval
 * target is the product's approval card «Godkjenning · Forbered oppfølging»
 * with «Godkjenn» / «Avvis».
 */
import {
  FJORDFORM_NOW_ISO,
  nordlysAnswerChunks,
  nordlysApproval,
  nordlysArtifactId,
  nordlysArtifactMarkdown,
  nordlysArtifactTitle,
  nordlysDraftChunks,
  nordlysRequestId,
  nordlysRunId,
  nordlysSources,
  nordlysSteps,
  nordlysSuggestion,
  nordlysThreadId,
  nordlysTitle,
} from '../fixtures/fjordform'
import { installCanvasOpen, installComposerTravel, isSendPress } from '../choreography'
import type { SceneDefinition, SceneStreamFrame, SceneTarget } from '../scene-types'
import { installPromptSuggestions } from '../suggestions'
import './02-plan.css'

/**
 * Film set-up that must exist before the chat page renders. This module is
 * evaluated when ScenePage loads (it imports the scene registry), which is
 * before ScenePage mounts ChatPage, and it acts only on /scene/02-plan.
 *
 * 1. The fictional clock. The empty state greets by the hour it reads from
 *    `new Date()` once at mount («God morgen» before 10:00, «God dag» until
 *    17:00, «God kveld» after). Before the harness pauses virtual time `Date`
 *    is the real wall clock, so the opening frame said «God morgen» or «God
 *    dag» depending on when the film was captured. Fixed at 12:00 Oslo on
 *    Friday 9 October 2026 (CONTRACT.md §0) it always says «God dag». The
 *    clock advances with `performance.now()`, which follows virtual time under
 *    the harness, so relative labels («Nå nettopp», elapsed waits) behave.
 * 2. The scope attribute for `02-plan.css` (film-only layout and motion).
 * 3. The suggestion card that stands in for the welcome text, the welcome
 *    cross-fade, the composer's glide to its dock and the work surface's
 *    growth (choreography.ts).
 */
function installScene02Setup(): void {
  if (typeof window === 'undefined') return
  if (!window.location.pathname.startsWith('/scene/02-plan')) return
  document.documentElement.setAttribute('data-verevon-scene', '02-plan')
  installPromptSuggestions({
    label: nordlysSuggestion.label,
    items: [{ text: nordlysSuggestion.text, source: nordlysSuggestion.source, time: nordlysSuggestion.time, icon: 'package' }],
  })
  installWelcomeCrossfade()
  installComposerTravel()
  installCanvasOpen('aside.verevon-chat-workspace-canvas')
  installEntranceDirector()
  installKilderCrossfade()
  // Pressing «Send melding» would focus it, and the composer's :focus-within
  // ring flashed for the three frames between press and send. A film has no
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
  // `Date()` called as a function returns a string of "now"; keep that too.
  window.Date = new Proxy(SceneDate, {
    apply: () => new SceneDate().toString(),
  }) as unknown as DateConstructor
}

/**
 * The product swaps the start screen for the thread in one frame when the
 * first message is sent. For the film the greeting and the suggestion card
 * leave softly instead: at the press on «Send melding» (a real `pointerdown`
 * from the harness's CDP mouse) the heading block is copied with its real
 * classes and measured; when the real start screen unmounts, the copy is
 * placed at the same spot inside the chat section and fades out
 * (02-plan.css `.verevon-s02-welcome-ghost`). It is inert, aria-hidden and
 * removed when its fade ends. No component is touched.
 */
function installWelcomeCrossfade(): void {
  let pending: { node: HTMLElement; rect: DOMRect } | null = null
  let placed = false
  const capture = (event: Event) => {
    if (placed || pending || !isSendPress(event)) return
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
    ghost.className = 'verevon-s02-welcome-ghost'
    ghost.setAttribute('aria-hidden', 'true')
    ghost.inert = true
    ghost.style.left = `${pending.rect.left}px`
    ghost.style.top = `${pending.rect.top}px`
    ghost.style.width = `${pending.rect.width}px`
    ghost.style.height = `${pending.rect.height}px`
    ghost.appendChild(pending.node)
    // Gone once faded, so the text dumps no longer carry the greeting.
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
 * Kilder to Resultat, body half of the hand-over. The product swaps the canvas
 * body in one frame when the first artifact arrives: the Kilder panel is
 * removed and the artifact workspace mounted. Fading the Kilder panel out
 * BEFORE the swap left the whole column white for a few frames. Instead the
 * panel stays at full opacity until the swap, and here the removed panel (its
 * DOM intact) is put back into a ghost body laid over the real body, which
 * fades out (02-plan.css) while the document rises in. It is a true
 * cross-dissolve of the two real surfaces, and no frame holds an empty column.
 * The ghost is inert, aria-hidden and removed when its fade ends. No component
 * is touched.
 */
function installKilderCrossfade(): void {
  const isKilderPanel = (node: Node): node is HTMLElement =>
    node instanceof HTMLElement && node.classList.contains('verevon-chat-panel')
  const observer = new MutationObserver((records) => {
    for (const record of records) {
      const target = record.target
      if (!(target instanceof HTMLElement) || !target.classList.contains('verevon-chat-workspace-canvas__body')) continue
      if (target.classList.contains('verevon-s02-kilder-ghost')) continue
      const removed = Array.from(record.removedNodes).find(isKilderPanel)
      if (!removed) continue
      // Only when the document replaces it (not when the canvas closes).
      if (!target.querySelector(':scope > .verevon-chat-artifact-workspace')) continue
      const canvas = target.closest<HTMLElement>('.verevon-chat-workspace-canvas')
      if (!canvas) continue
      const ghost = document.createElement('div')
      ghost.className = 'verevon-chat-workspace-canvas__body verevon-s02-kilder-ghost'
      ghost.setAttribute('aria-hidden', 'true')
      ghost.inert = true
      ghost.style.top = `${target.offsetTop}px`
      ghost.style.left = `${target.offsetLeft}px`
      ghost.style.width = `${target.offsetWidth}px`
      ghost.style.height = `${target.offsetHeight}px`
      ghost.appendChild(removed)
      ghost.addEventListener('animationend', () => ghost.remove(), { once: true })
      canvas.appendChild(ghost)
    }
  })
  const start = () => observer.observe(document.body, { childList: true, subtree: true })
  if (document.body) start()
  else document.addEventListener('DOMContentLoaded', start, { once: true })
}

/**
 * Entrances that survive the product's re-renders.
 *
 * The real source list, markdown renderer and approval list rebuild their DOM
 * on every streamed update (measured: each citation re-mounts every source
 * card, each artifact update every list item, `done` the approval card). A CSS
 * animation on such an element restarts or is cut short on every re-mount,
 * which made cards pop in, slides collapse into fades and the approval card
 * arrive without any entrance. Here every entrance is a Web Animation keyed by
 * the element's identity (its text), started when that identity first
 * appears. When the product re-mounts the element, the new node gets the same
 * animation already advanced by the time elapsed since the first appearance,
 * so the motion continues as if nothing happened.
 *
 * Time comes from `document.timeline.currentTime` (the frame clock the capture
 * harness drives exactly), never from `performance.now()`.
 */
type Entrance = {
  selector: string
  key: (element: Element) => string
  keyframes: Keyframe[]
  duration: number
  delay?: (element: Element) => number
  /** Default: the contract's ease-out over the whole span. */
  easing?: string
  fill?: FillMode
}

const EASE_OUT = 'cubic-bezier(0.33, 1, 0.68, 1)' // 1 - (1 - p)^3

const rise = (px: number): Keyframe[] => [
  { opacity: 0, transform: `translateY(${px}px)` },
  { opacity: 1, transform: 'translateY(0)' },
]
const fade: Keyframe[] = [{ opacity: 0 }, { opacity: 1 }]
const textKey = (element: Element) => (element.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 60)
const siblingIndex = (element: Element) => (element.parentElement ? Array.from(element.parentElement.children).indexOf(element) : 0)
const isPlanRow = (element: Element) => element.parentElement?.previousElementSibling?.tagName === 'H2'

/**
 * Kilder → Resultat. The product swaps the canvas body (and re-builds the tab
 * strip, which gains Arbeid and Resultat) in one frame when the first artifact
 * arrives. Both are keyed to the Kilder surface's mount (first citation,
 * scene 1.50 s): the sources hold, the source list and the tab strip fade out
 * from 2.56 s to 2.78 s, the artifact arrives at 2.80 s and rises in
 * (02-plan.css). The tab strip stays out until the first step has added the
 * Arbeid tab (2.96 s, which shifts Resultat and Kilder sideways) and fades
 * back in from 3.06 s to 3.42 s, so no tab is ever seen jumping. The fade is
 * one keyed animation, so it continues across the tab strip's re-mount.
 */
const KILDER_MOUNT_TO_EXIT = 1_140
const KILDER_EXIT_SPAN = 100
// Round 4 retune (the design critic's blank-column defect): the Kilder body no
// longer fades out before the swap. It stays fully visible until the artifact
// arrives (2.80 s) and then cross-fades through a ghost (installKilderCrossfade)
// while Resultat rises in. The tab strip leaves late and short (fades out
// 2.64 to 2.74 s, hidden only while the first step joins Arbeid and Resultat
// opens) and fades back in WITH the card from 2.80 s to 3.10 s, so the header
// is never empty while the document is on screen.
const KILDER_TABS_BACK = 1_300
const KILDER_TABS_SPAN = 1_600
const KILDER_EXIT: Entrance[] = [
  {
    selector: '.verevon-chat-workspace-canvas__head .verevon-chat-tabs',
    key: () => 'kilder-tabs',
    keyframes: [
      { opacity: 1, offset: 0 },
      { opacity: 1, offset: KILDER_MOUNT_TO_EXIT / KILDER_TABS_SPAN, easing: 'cubic-bezier(0.32, 0, 0.67, 0)' },
      { opacity: 0, offset: (KILDER_MOUNT_TO_EXIT + KILDER_EXIT_SPAN) / KILDER_TABS_SPAN },
      { opacity: 0, offset: KILDER_TABS_BACK / KILDER_TABS_SPAN, easing: EASE_OUT },
      { opacity: 1, offset: 1 },
    ],
    duration: KILDER_TABS_SPAN,
    easing: 'linear',
    fill: 'none',
  },
]

const ENTRANCES: Entrance[] = [
  // Source cards: 12 px rise, 380 ms; the citations arrive 120 ms apart.
  { selector: '.verevon-chat-source-card', key: (e) => `source:${textKey(e)}`, keyframes: rise(12), duration: 380 },
  // Status findings rise one by one; the plan rows (under «Klar for neste
  // steg») arrive in one update and stagger 110 ms after the heading.
  {
    selector: '.verevon-chat-artifact-document li',
    key: (e) => `li:${textKey(e)}`,
    keyframes: rise(10),
    duration: 380,
    delay: (e) => (isPlanRow(e) ? 110 * (siblingIndex(e) + 1) : 0),
  },
  // A finding's source reference fades in 200 ms after its line.
  { selector: '.verevon-chat-artifact-document li > em', key: (e) => `ref:${textKey(e.parentElement ?? e)}`, keyframes: fade, duration: 320, delay: () => 200 },
  { selector: '.verevon-chat-artifact-document h2', key: (e) => `h2:${textKey(e)}`, keyframes: rise(10), duration: 380 },
  // The draft block under the answer (re-mounted on every streamed chunk).
  { selector: '.verevon-chat-message--assistant .verevon-chat-markdown > blockquote', key: () => 'draft', keyframes: rise(8), duration: 400 },
  // The approval card: the climax. Born inside the frame (the thread fits
  // without scrolling, 02-plan.css), it rises 10 px over 400 ms.
  { selector: '.verevon-chat-approval', key: () => 'approval', keyframes: rise(10), duration: 400, delay: () => 40 },
  ...KILDER_EXIT,
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
      easing: entrance.easing ?? EASE_OUT,
      fill: entrance.fill ?? 'backwards',
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

installScene02Setup()

/** The suggestion card on the start screen (suggestions.ts). */
const SUGGESTION: SceneTarget = { selector: '.v-suggestion', pin: true }
const SEND_BUTTON: SceneTarget = { name: 'Send melding', role: 'button', pin: true }
const APPROVE_BUTTON: SceneTarget = { selector: '.verevon-chat-approval__approve', pin: true }

/** Mouse release on the suggestion: its click fills the composer (nothing is sent). */
const CHOOSE_AT = 1_380
/**
 * Mouse release that sends the message; the POST arrives a few ms later.
 * The composer's rotating placeholder suggestion (DashboardComposer, every
 * 6.0 s after it empties) is hidden in 02-plan.css, so it no longer constrains
 * the send time.
 */
const SEND_AT = 2_280
/** Scene time at which the stream clock starts (the intercepted POST). */
const POST_AT = 2_290
/**
 * First citation: the Kilder surface opens. The entrance constants above
 * (KILDER_MOUNT_TO_EXIT, KILDER_TABS_BACK) are relative to this moment.
 */
const KILDER_AT = 2_560

/** A stream frame placed at scene time `sceneMs`. */
function frame(sceneMs: number, event: string, data: Record<string, unknown>): SceneStreamFrame {
  return { at: sceneMs - POST_AT, event, data }
}

function chunk(sceneMs: number, delta: string): SceneStreamFrame {
  return frame(sceneMs, 'chunk', { delta, request_id: nordlysRequestId })
}

function step(sceneMs: number, index: number, status: string): SceneStreamFrame {
  const value = nordlysSteps[index]!
  return frame(sceneMs, 'step_update', { id: value.id, title: value.title, detail: value.detail, status })
}

function artifact(sceneMs: number, lines: number, withPlan: boolean): SceneStreamFrame {
  return frame(sceneMs, 'artifact', {
    id: nordlysArtifactId,
    // 'markdown' renders through the document viewer like 'document' does.
    // Its raw kind chip is hidden with the rest of the metadata strip.
    kind: 'markdown',
    title: nordlysArtifactTitle,
    content: nordlysArtifactMarkdown(lines, withPlan),
    version: 1,
  })
}

const frames: SceneStreamFrame[] = [
  frame(POST_AT + 10, 'connected', { ok: true, request_id: nordlysRequestId, thread_id: nordlysThreadId, model_used: 'verevon-balance', run_id: nordlysRunId }),
  // The thread title right away: until it arrives the header shows the
  // question itself, cut with an ellipsis.
  frame(POST_AT + 20, 'title', { title: nordlysTitle, request_id: nordlysRequestId }),
  // Sources, 120 ms apart. One source group, so the real list shows no
  // per-provider headings (a heading inserted above the first card when the
  // second provider arrived pushed it down 37 px in one frame). The list
  // re-mounts every card on each citation; the entrance director above keeps
  // each card's rise running across those re-mounts.
  ...nordlysSources.map((source, index) => frame(KILDER_AT + index * 120, 'citation', { ...source, source_group_id: 'kb:fjordform', source_group_label: 'Fjordform AS' })),
  // Assistant line. The three sources hold at full opacity until the swap.
  chunk(3_300, nordlysAnswerChunks[0]!),
  chunk(3_500, nordlysAnswerChunks[1]!),
  // Arbeid badge 1 → 2 → 3. The first step lands 60 ms before the artifact,
  // while the tab strip is faded out, so the strip is rebuilt once in its
  // final layout (Arbeid, Resultat, Kilder) and fades back in with the
  // document (KILDER_EXIT). Later steps only change the badge digit.
  step(KILDER_AT + 1_240, 0, 'done'),
  artifact(KILDER_AT + 1_300, 1, false),
  artifact(4_160, 2, false),
  step(4_300, 1, 'done'),
  artifact(4_460, 3, false),
  artifact(4_760, 4, false),
  step(4_900, 2, 'done'),
  // Draft block + plan rows.
  chunk(5_000, nordlysDraftChunks[0]!),
  artifact(5_100, 4, true),
  chunk(5_250, nordlysDraftChunks[1]!),
  chunk(5_450, nordlysDraftChunks[2]!),
  // The approval gate: the third step (id = run id) pauses for a person, so
  // the real controller fetches the run's pending approvals.
  step(5_700, 2, 'awaiting_approval'),
  // `done` follows the pausing step by 160 ms. Sent in the same tick, the
  // approval card mounted after the turn had settled and the transcript no
  // longer followed it.
  frame(5_860, 'done', { request_id: nordlysRequestId, model_used: 'verevon-balance', stop_reason: 'end_turn' }),
]

export const scene02Plan: SceneDefinition = {
  id: '02-plan',
  durationMs: 8_000,
  // Empty: the start screen is the product's own. The click on the suggestion
  // card fills the composer (suggestions.ts).
  composerPrefill: '',
  readySelectors: ['.v-suggestion'],
  stream: { path: '/api/v1/chat/stream', frames, closeAfterMs: 40 },
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
    // Rest on the button for 120 ms and fade out in place.
    { at: 2_400, target: SEND_BUTTON, visible: false, pressed: false, ease: 'hold' },
    // Parked off the app card once invisible, so no hover state is held
    // anywhere while the answer builds.
    { at: 2_520, x: 4, y: 634, visible: false, pressed: false },
    // Fade in just below the approval card and glide to «Godkjenn» (550 ms).
    { at: 6_300, target: APPROVE_BUTTON, nudge: { x: 44, y: 46 }, visible: true, pressed: false },
    { at: 6_400, target: APPROVE_BUTTON, nudge: { x: 44, y: 46 }, visible: true, pressed: false, ease: 'hold' },
    { at: 6_950, target: APPROVE_BUTTON, offset: { x: 0.7, y: 0.62 }, visible: true, pressed: false },
    { at: 8_000, target: APPROVE_BUTTON, offset: { x: 0.7, y: 0.62 }, visible: true, pressed: false, ease: 'hold' },
  ],
  events: [
    { at: 0, kind: 'note', note: 'frame 0: centred start screen, suggestion card, empty composer, no work surface, cursor hidden' },
    { at: 800, kind: 'cursor', note: 'cursor glides to the suggestion' },
    { at: CHOOSE_AT, kind: 'ui', note: 'click the suggestion: fills the composer (autopilot only)', action: 'click', target: SUGGESTION },
    { at: 1_800, kind: 'cursor', note: 'cursor glides to Send melding' },
    { at: SEND_AT, kind: 'ui', note: 'click Send melding (autopilot only)', action: 'click', target: { name: 'Send melding', role: 'button' } },
    { at: KILDER_AT, kind: 'sse', note: 'citations start; the Kilder surface grows in', event: 'citation' },
    { at: 3_300, kind: 'sse', note: 'answer streams; sources hold', event: 'chunk' },
    { at: KILDER_AT + 1_300, kind: 'sse', note: 'Resultat opens with the status document', event: 'artifact' },
    { at: 5_000, kind: 'sse', note: 'draft block; plan rows; approval gate', event: 'step_update' },
    { at: 6_400, kind: 'cursor', note: 'cursor glides to Godkjenn' },
    { at: 6_950, kind: 'hover', note: 'Godkjenn hovered, held to the end', target: APPROVE_BUTTON },
    { at: 6_950, kind: 'ui', note: 'hover Godkjenn (autopilot only)', action: 'hover', target: APPROVE_BUTTON },
  ],
}

export const scene02Approval = nordlysApproval
