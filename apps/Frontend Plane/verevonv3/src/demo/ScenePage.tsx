/**
 * Route `/scene/:id` (scene mode only).
 *
 * Mounts the REAL chat workspace (`ChatPage`) inside the real shell, hides the
 * surrounding chrome through `scene.css` (scoped under `html.verevon-scene`),
 * prefills the composer through the real textarea and a real `input` event,
 * and installs `window.__verevonScene` (CONTRACT.md section 2).
 *
 * `ready` resolves only when fonts are loaded, the session is authenticated
 * from fixtures, the composer holds the prefill, and two animation frames
 * have passed after that — the UI is exactly at frame 0. `start()` then
 * schedules the timeline with `setTimeout` only.
 *
 * Open with `?autopilot=1` to let the runtime perform the click/hover itself
 * (synthetic events) for a quick look in a normal browser. The capture
 * harness and the preview script drive the mouse through CDP instead, so the
 * `:hover` and the click are genuine.
 */
import { Show, createEffect, createSignal, onCleanup } from 'solid-js'
import ChatPage from '@/features/chat/components/ChatPage'
import { chatAppearanceTokens } from '@/features/chat/lib/chat-appearance-preview'
import { getSession } from '@/shared/session/session-store'
import sceneConfigRaw from './scene-config.json?raw'
import { SceneCursor, setSceneCursor } from './SceneCursor'
import { appendSceneLog, getSceneLog, markSceneStart, setActiveSceneExtras, setActiveSceneStream } from './install'
import { createSceneScheduler, pointerAt, resolveSceneTarget } from './scene-timeline'
import type { SceneConfig, SceneDefinition, SceneEvent, SceneTarget, VerevonScene } from './scene-types'
import { findScene } from './scenes'
import './scene.css'

const COMPOSER_TEXTAREA = 'textarea#verevon-dashboard-input'
/** The CSS viewport the films are captured at (CONTRACT.md amendment A). */
const sceneConfig = JSON.parse(sceneConfigRaw) as SceneConfig

function sceneIdFromPath(): string {
  const parts = window.location.pathname.split('/').filter(Boolean)
  return parts[1] ?? ''
}

function nextFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()))
}

function waitFor<T>(probe: () => T | null | undefined, timeoutMs = 15_000): Promise<T> {
  return new Promise((resolve, reject) => {
    const startedAt = performance.now()
    const tick = () => {
      const value = probe()
      if (value) {
        resolve(value)
        return
      }
      if (performance.now() - startedAt > timeoutMs) {
        reject(new Error('Scene ready timed out'))
        return
      }
      requestAnimationFrame(tick)
    }
    tick()
  })
}

/** Set the real textarea value and let the composer hear it as typed input. */
function prefillComposer(textarea: HTMLTextAreaElement, text: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set
  if (setter) setter.call(textarea, text)
  else textarea.value = text
  textarea.dispatchEvent(new Event('input', { bubbles: true }))
}

function elementFor(target: SceneTarget): Element | null {
  if (target.selector) return document.querySelector(target.selector)
  if (target.name) {
    const wanted = target.name.trim().toLowerCase()
    return Array.from(document.querySelectorAll('button, a[href], [role="button"]')).find((element) => {
      const label = (element.getAttribute('aria-label') ?? element.getAttribute('title') ?? element.textContent ?? '').trim().toLowerCase()
      return label === wanted
    }) ?? null
  }
  return null
}

function installSceneGlobal(scene: SceneDefinition, readyPromise: Promise<void>, autopilot: boolean): () => void {
  const pinned = new WeakMap<SceneTarget, DOMRect>()
  const resolver = (target: SceneTarget): DOMRect | null => {
    const kept = pinned.get(target)
    if (kept) return kept
    const rect = resolveSceneTarget(target)
    if (rect && target.pin) pinned.set(target, rect)
    return rect
  }

  const fire = (event: SceneEvent) => {
    appendSceneLog(event.kind, event.note)
    if (event.kind === 'ui' && autopilot) {
      const element = elementFor(event.target)
      if (!(element instanceof HTMLElement)) return
      if (event.action === 'click') element.click()
      else element.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }))
      return
    }
    if (event.kind === 'ui' && !autopilot) return
  }
  const scheduler = createSceneScheduler(scene.events, fire)

  // Autopilot draws the pointer itself, with the same `pointerAt`, so a plain
  // browser preview shows the cursor path the harness would dispatch.
  let autopilotTimer: number | undefined
  const api: VerevonScene = {
    id: scene.id,
    durationMs: scene.durationMs,
    // The scene's own entry (amendment A) wins over the default.
    viewport: sceneConfig.scenes?.[scene.id]?.viewport ?? sceneConfig.viewport,
    ready: readyPromise,
    start() {
      markSceneStart()
      scheduler.start()
      appendSceneLog('runtime', 'start()')
      if (autopilot) {
        const startedAt = performance.now()
        const tick = () => {
          const t = performance.now() - startedAt
          const pointer = pointerAt(scene.pointer, t, resolver)
          setSceneCursor(pointer.x, pointer.y, pointer.visible, pointer.pressed)
          if (t < scene.durationMs) autopilotTimer = window.setTimeout(tick, 16)
        }
        tick()
      }
    },
    pointerAt(tMs: number) {
      return pointerAt(scene.pointer, tMs, resolver)
    },
    setCursor(x: number, y: number, visible: boolean, pressed: boolean) {
      setSceneCursor(x, y, visible, pressed)
    },
    get log() {
      return getSceneLog()
    },
  }
  window.__verevonScene = api
  return () => {
    scheduler.dispose()
    if (autopilotTimer !== undefined) window.clearTimeout(autopilotTimer)
    if (window.__verevonScene === api) delete window.__verevonScene
  }
}

export default function ScenePage() {
  const session = getSession()
  const scene = findScene(sceneIdFromPath())
  const autopilot = new URLSearchParams(window.location.search).get('autopilot') === '1'
  const [failure, setFailure] = createSignal<string | null>(null)

  if (!scene) {
    return (
      <div class="verevon-scene-missing" role="alert">
        <p>Ukjent scene: {sceneIdFromPath() || '(tom)'}</p>
      </div>
    )
  }

  document.documentElement.classList.add('verevon-scene')
  setActiveSceneStream(scene.stream)
  setActiveSceneExtras({ routes: scene.routes, runStreams: scene.runStreams, images: scene.images })
  let resolveReady: () => void = () => undefined
  let rejectReady: (error: Error) => void = () => undefined
  const ready = new Promise<void>((resolve, reject) => {
    resolveReady = resolve
    rejectReady = reject
  })
  const dispose = installSceneGlobal(scene, ready, autopilot)

  onCleanup(() => {
    dispose()
    setActiveSceneStream(null)
    setActiveSceneExtras(null)
    document.documentElement.classList.remove('verevon-scene')
  })

  // Frame 0: fonts loaded, session from fixtures, composer prefilled, two
  // frames settled. Runs once; the session store flips to authenticated when
  // the shell's loadSession() receives the fixture answers.
  createEffect(
    () => session.status === 'authenticated',
    (authenticated) => {
      if (!authenticated) return
      void (async () => {
        try {
          await document.fonts.ready
          // The shell keys the refined chat appearance on the `/chat` route
          // (CoreShell's `data-chat-appearance`); this route is `/scene/*`, so
          // the same tokens are applied here, once, before frame 0. Without
          // them the composer renders its legacy look (context label outside
          // the chip, squeezed to «K…»).
          const shell = await waitFor(() => document.querySelector<HTMLElement>('.core-product-shell'))
          shell.setAttribute('data-chat-appearance', chatAppearanceTokens())
          const textarea = await waitFor(() => document.querySelector<HTMLTextAreaElement>(COMPOSER_TEXTAREA))
          if (scene.prepare) {
            // Real UI set-up before frame 0 (e.g. «Utfør» in the composer).
            await scene.prepare({
              click: async (target) => {
                const element = await waitFor(() => {
                  const found = elementFor(target)
                  return found instanceof HTMLElement && !(found as HTMLButtonElement).disabled ? found : null
                })
                element.click()
                element.blur()
              },
              waitFor: (probe) => waitFor(probe),
            })
            appendSceneLog('runtime', 'prepare: done')
          }
          prefillComposer(textarea, scene.composerPrefill)
          await waitFor(() => (textarea.value === scene.composerPrefill ? textarea : null))
          // The send button is enabled once the composer has accepted the text.
          await waitFor(() => {
            const button = elementFor({ name: 'Send melding', role: 'button' })
            return button instanceof HTMLButtonElement && !button.disabled ? button : null
          })
          textarea.blur()
          // The composer eases out of its focused look (160 ms transitions)
          // after the blur; frame 0 must be the settled resting state.
          await new Promise<void>((resolve) => window.setTimeout(resolve, 320))
          await nextFrame()
          await nextFrame()
          appendSceneLog('runtime', 'ready: fonts loaded, composer prefilled, frame 0')
          resolveReady()
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error)
          setFailure(message)
          rejectReady(error instanceof Error ? error : new Error(message))
        }
      })()
    },
  )

  return (
    <>
      <Show when={session.status === 'authenticated'}>
        <ChatPage />
      </Show>
      <Show when={failure()}>
        {(message) => <div class="verevon-scene-missing" role="alert"><p>Scenen kunne ikke klargjøres: {message()}</p></div>}
      </Show>
      <SceneCursor />
    </>
  )
}
