/**
 * Scene runtime contract (plans/product-scenes/CONTRACT.md, section 2).
 *
 * `VerevonScene` is the type behind `window.__verevonScene` and matches the
 * contract text exactly. Everything below it describes how a scene is written
 * (`SceneDefinition`) so the runtime can drive the real chat workspace from a
 * fixture-backed fetch, a mock SSE stream and a scripted pointer.
 *
 * This folder exists only when `import.meta.env.VITE_SCENE_MODE === '1'`.
 */

export interface VerevonScene {
  readonly id: string;                       // "02-plan" | "03-undersok"
  readonly durationMs: number;               // 8000
  readonly viewport: { width: number; height: number };   // CSS px, from scene-config.json
  // Resolves when: fixtures installed, route mounted, document.fonts.ready, two rAFs after first paint,
  // composer prefilled, UI is exactly at frame 0. The timeline has NOT started.
  readonly ready: Promise<void>;
  // t = 0 now. Schedules every timeline event with setTimeout relative to this call. Throws if called twice.
  start(): void;
  // Where the pointer is at scene time tMs. Pure function of tMs and current layout (targets resolved lazily by selector).
  pointerAt(tMs: number): { x: number; y: number; visible: boolean; pressed: boolean };
  // Draws the in-page cursor overlay at CSS px (x,y). Called by the harness each frame.
  setCursor(x: number, y: number, visible: boolean, pressed: boolean): void;
  // Chronological log of fired events: { atMs, kind, note }. For assertions.
  readonly log: ReadonlyArray<{ atMs: number; kind: string; note: string }>;
}

export type SceneLogEntry = { atMs: number; kind: string; note: string }

export type ScenePointerState = { x: number; y: number; visible: boolean; pressed: boolean }

/**
 * A DOM target resolved lazily, at the moment the pointer position is asked
 * for, so a keyframe can point at an element that does not exist at scene
 * start (the approval button, the send button after the composer moved).
 *
 * `selector` is a CSS selector; `name` is an accessible name matched against
 * `aria-label`, `title` or trimmed text content of elements matching `role`
 * (default: buttons and links). The first visible match wins.
 */
export type SceneTarget = {
  selector?: string
  name?: string
  role?: 'button' | 'link' | 'textbox' | 'tab' | 'any'
  /**
   * Resolve once and keep that rectangle for the rest of the scene. Used for
   * a target that moves after the pointer reaches it (the send button jumps
   * to the composer dock the instant the first message is sent) so the
   * pointer stays where the click happened instead of following the button.
   */
  pin?: boolean
}

/**
 * One pointer keyframe. The pointer moves from the previous keyframe's point
 * to this one between `previous.at` and `at`, eased out. `visible` and
 * `pressed` are step functions: they take this keyframe's values from `at` on.
 * A keyframe is positioned either at an absolute CSS px point (`x`, `y`) or at
 * a target's rectangle plus `offset` (fractions of the rect, 0.5/0.5 = centre)
 * plus `nudge` (CSS px).
 */
export type ScenePointerKeyframe = {
  at: number
  x?: number
  y?: number
  target?: SceneTarget
  offset?: { x: number; y: number }
  nudge?: { x: number; y: number }
  visible: boolean
  pressed: boolean
  /** Motion curve into this keyframe. Default `ease-out`. */
  ease?: 'ease-out' | 'linear' | 'hold'
}

/** One frame of the mock `text/event-stream` body, in ms after the POST arrived. */
export type SceneStreamFrame = {
  at: number
  event: string
  data: Record<string, unknown>
  id?: string
}

/**
 * The chat answer as the real transport sees it. The mock fetch answers
 * `POST /api/v1/chat/stream` with a `ReadableStream` that enqueues each frame
 * with `setTimeout(frame.at)` relative to the instant the request arrived, so
 * the stream advances under paused or advancing virtual time.
 */
export type SceneStreamScript = {
  path: string
  frames: SceneStreamFrame[]
  /** Close the stream this long after the last frame. */
  closeAfterMs?: number
}

export type SceneEvent =
  /** A note in the log; nothing else happens. */
  | { at: number; kind: 'note'; note: string }
  /** Runtime-side UI action; only used when the harness is not driving the mouse (`?autopilot=1`). */
  | { at: number; kind: 'ui'; note: string; action: 'click' | 'hover'; target: SceneTarget }
  /** Marker for a stream beat the SSE script will produce around this time (for assertions). */
  | { at: number; kind: 'sse'; note: string; event: string }
  /** Marker for a pointer beat. */
  | { at: number; kind: 'cursor'; note: string }
  /** Marker for the final hover state. */
  | { at: number; kind: 'hover'; note: string; target: SceneTarget }

export type SceneDefinition = {
  id: string
  durationMs: number
  /**
   * What the composer holds at frame 0 (set through the real textarea + `input` event).
   * Empty means the composer starts empty: the scene fills it itself later (a
   * click on a prompt suggestion), so the start screen is the product's own.
   */
  composerPrefill: string
  /** CSS selectors that must exist before frame 0 (for example the suggestion card the scene injects). */
  readySelectors?: string[]
  /** Time-ordered. The scheduler fires each with `setTimeout(at)` relative to `start()`. */
  events: SceneEvent[]
  /** Time-ordered pointer keyframes; targets resolve lazily by selector or accessible name. */
  pointer: ScenePointerKeyframe[]
  /** The element whose `:hover` state closes the scene. */
  hoverTarget: SceneTarget
  /** The SSE script the mock fetch plays for the scene's chat request. */
  stream: SceneStreamScript
  /**
   * Optional set-up through the REAL UI before frame 0, after the composer
   * mounted and before it is prefilled (scene 03 selects «Utfør» with the
   * composer's own segment button). Everything it does settles before `ready`.
   */
  prepare?: (helpers: ScenePrepareHelpers) => Promise<void>
  /** Scene-specific JSON fixtures, matched before the shared routes in `install.ts`. */
  routes?: SceneFixtureRoute[]
  /** Run-event streams (`GET /api/v1/runs/:id/events`) the run panels subscribe to. */
  runStreams?: SceneRunStream[]
  /**
   * Fixture images for evidence the app loads with a plain `<img src>` (browser
   * step screenshots come from `/api/v1/browser/sessions/:run/artifacts/:id`,
   * which never passes through `fetch`). Request pathname → asset URL.
   */
  images?: Record<string, string>
}

export type ScenePrepareHelpers = {
  /** Click the first visible match of `target` (real `click()` on the element). */
  click: (target: SceneTarget) => Promise<void>
  /** Wait until `probe` returns a truthy value (rAF polling, 15 s timeout). */
  waitFor: <T>(probe: () => T | null | undefined) => Promise<T>
}

/** A scene-specific fixture answer. `match` is tested against pathname + search, then pathname. */
export type SceneFixtureRoute = {
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH'
  match: RegExp
  body: unknown
  status?: number
}

/**
 * A run's live event tail. Frame times share the chat stream's clock: ms after
 * the chat POST arrived. A subscriber that connects later receives the frames
 * it missed immediately (in order) and the rest on schedule, so both panels
 * that watch the same run see the same history. The tail stays open (a run
 * paused at an approval gate has not finished) unless `closeAfterMs` is set.
 */
export type SceneRunStream = {
  runId: string
  frames: SceneStreamFrame[]
  closeAfterMs?: number
}

/** What `install.ts` needs from the active scene beyond its chat stream. */
export type SceneExtras = {
  routes?: SceneFixtureRoute[]
  runStreams?: SceneRunStream[]
  images?: Record<string, string>
}

export type SceneConfig = {
  /** Default CSS viewport for a scene without its own entry. */
  viewport: { width: number; height: number }
  /** Per-scene CSS viewport chosen under CONTRACT amendment A (smallest working 16:10 width). */
  scenes?: Record<string, { viewport: { width: number; height: number } }>
}

declare global {
  interface Window {
    __verevonScene?: VerevonScene
    /** Requests per `/api` path, for the navbar-polling check in the preview script. */
    __verevonSceneRequestCounts?: ReadonlyMap<string, number>
  }
}
