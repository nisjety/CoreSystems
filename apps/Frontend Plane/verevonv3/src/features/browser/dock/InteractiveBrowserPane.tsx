import { createEffect, createMemo, createSignal, Show } from 'solid-js'
import { Portal, type JSX } from '@solidjs/web'
import { BrowserChrome } from '@/features/dashboard/home/BrowserChrome'
import type { BrowserSessionViewModel } from '@/features/dashboard/home/browser-session'
import type { BrowserNavigationFailure } from '@/features/browser/session/browser-navigation-error'
import type { BrowserSessionController } from '@/features/browser/session/use-browser-session-controller'
import type { KeepAliveOutcome } from '@/features/browser/session/session-keepalive'
import { BrowserSearchResults } from '@/features/browser/omnibox/BrowserSearchResults'
import { OmniboxField } from '@/features/browser/omnibox/OmniboxField'
import { currentOmniboxHistoryKey, recordOmniboxVisit } from '@/features/browser/omnibox/omnibox-history'
import { browserOmniboxSearchEnabled, type OmniboxIntent } from '@/features/browser/omnibox/omnibox-resolver'
import {
  attachBrowserRegion,
  browserErrorMessage,
  keepAliveBrowserSession,
} from '@/shared/api/browser-client'
import {
  buildRegionAttachRequest,
  cropFrameRegion,
  normalizeSelectionText,
  regionFromDrag,
  type BrowserRegionAttachment,
  type ClientPoint,
  type DisplayBox,
  type RegionAttachRefusal,
  type RegionImagePayload,
  type RegionRect,
} from '@/features/browser/page-context/region-attach'
import { ApiError } from '@/shared/api/http'
import { useI18n } from '@/shared/i18n'
import { X, Globe, ScanSearch } from '@/shared/icons'
import { BrowserDockControls } from './BrowserDockControls'
import type { BrowserDockPosition } from './browser-dock-state'
import { AskAboutPageButton } from './OpenBrowserButton'
import './interactive-browser-pane.css'

// The interactive browser body of the chat dock (WS9.H). It takes the rail while the browser is open
// beside the chat, in the same place as the run watch. With no session open it shows the start state: an
// omnibox, its search results and any error. With a session open it mounts BrowserChrome in its compact
// form, wired to the page's controller. The agent loop is not wired here: the model controls stay off in
// this body, and the run watch keeps them.
//
// Nothing on this pane sends page content to the model. The omnibox's "Ask Verevon" row hands over only
// the text the person typed.

// The region select (E3.3-d) is dark until the build sets VITE_BROWSER_REGION_ATTACH to "true".
function browserRegionAttachEnabled(): boolean {
  return import.meta.env.VITE_BROWSER_REGION_ATTACH === 'true'
}

// The live frame is the interactive screenshot BrowserChrome draws for the active tab.
const LIVE_FRAME_SELECTOR = '.knowledge-browser-screenshot--interactive img'
// The aspect tolerance BrowserChrome uses before it draws on the frame: a picture of another shape is not
// mapped to the page, because a region drawn on it would be the wrong part of the page.
const FRAME_ASPECT_TOLERANCE = 0.01
const REGION_ATTACH_FALLBACK = { no: 'Området kunne ikke legges ved.', en: 'The area could not be attached.' }

function activeTab(model: BrowserSessionViewModel | null) {
  return model?.tabs.find((tab) => tab.active) ?? null
}

// The picture on screen is the live page when it is a streamed frame (an inline image) or the live polling frame.
// A replay step or a stored screenshot is neither, and a region is never cut from one: the gateway cannot tell
// those pixels from the page the tab shows now.
function isLivePicture(img: HTMLImageElement, model: BrowserSessionViewModel): boolean {
  const src = img.getAttribute('src') ?? ''
  if (src.startsWith('data:image/')) return true
  const live = model.liveFrameUrl
  return typeof live === 'string' && live !== '' && src.startsWith(live)
}

const LIVE_ONLY_COPY = {
  no: 'Området kan bare velges fra den direkte sidevisningen.',
  en: 'An area can only be selected from the live page view.',
}

/**
 * The box the frame's picture is drawn in, in client coordinates. The frame is an `<img>` with
 * `object-fit: contain`, so a picture that does not fill its box is letterboxed inside it. Null when the
 * picture cannot be mapped: it has no size yet, or it uses an object-fit this select does not map.
 */
function frameDisplayBox(img: HTMLImageElement): DisplayBox | null {
  const rect = img.getBoundingClientRect()
  const { naturalWidth, naturalHeight } = img
  if (!(rect.width > 0) || !(rect.height > 0) || !(naturalWidth > 0) || !(naturalHeight > 0)) return null
  const style = getComputedStyle(img)
  if (style.objectFit === 'fill') return { left: rect.left, top: rect.top, width: rect.width, height: rect.height }
  if (style.objectFit !== 'contain') return null
  const scale = Math.min(rect.width / naturalWidth, rect.height / naturalHeight)
  const width = naturalWidth * scale
  const height = naturalHeight * scale
  const [shareX, shareY] = objectPositionShares(style.objectPosition)
  return {
    left: rect.left + (rect.width - width) * shareX,
    top: rect.top + (rect.height - height) * shareY,
    width,
    height,
  }
}

/** The share of the free space an `object-position` leaves before the picture, per axis, from 0 to 1. */
function objectPositionShares(position: string): [number, number] {
  const share = (token: string | undefined): number => {
    if (token === undefined) return 0.5
    if (token.endsWith('%')) {
      const value = Number.parseFloat(token)
      return Number.isFinite(value) ? Math.min(Math.max(value / 100, 0), 1) : 0.5
    }
    if (token === 'left' || token === 'top') return 0
    if (token === 'right' || token === 'bottom') return 1
    return 0.5
  }
  const [horizontal, vertical] = position.trim().split(/\s+/)
  return [share(horizontal), share(vertical)]
}

/** The text the person has selected inside the browser body. Read at the press, since a press clears it. */
function selectedPageText(scope: HTMLElement | undefined): string | null {
  const selection = window.getSelection()
  if (!scope || !selection || selection.isCollapsed || !selection.anchorNode || !scope.contains(selection.anchorNode)) {
    return null
  }
  return normalizeSelectionText(selection.toString())
}

/** The drag rectangle in client coordinates, whichever way the drag went. */
function regionRectStyle(drag: { start: ClientPoint; end: ClientPoint }) {
  return {
    left: `${Math.min(drag.start.x, drag.end.x)}px`,
    top: `${Math.min(drag.start.y, drag.end.y)}px`,
    width: `${Math.abs(drag.end.x - drag.start.x)}px`,
    height: `${Math.abs(drag.end.y - drag.start.y)}px`,
  }
}

function regionRefusalText(reason: RegionAttachRefusal, tr: (noText: string, enText: string) => string): string {
  switch (reason) {
    case 'zero-retention':
      return tr(
        'Områder fra en økt med Zero Data Retention legges ikke ved.',
        'Regions from a session with Zero Data Retention are not attached.',
      )
    case 'invalid-tab':
      return tr('Fanen kan ikke brukes. Velg fanen på nytt.', 'The tab cannot be used. Select the tab again.')
    case 'region-too-small':
      return tr('Området er for lite.', 'The area is too small.')
    case 'image-too-large':
      return tr('Utsnittet er for stort. Velg et mindre område.', 'The picture is too large. Select a smaller area.')
    case 'selection-too-long':
      return tr(
        'Den markerte teksten er for lang. Fjern markeringen og prøv igjen.',
        'The selected text is too long. Clear the selection and try again.',
      )
  }
}

export type InteractiveBrowserPaneProps = {
  /** The chat page's session controller (see dock-session.ts). */
  controller: BrowserSessionController
  /** The active workspace, for the keep-alive call. */
  orgId: () => string | null | undefined
  /** Zero retention on this page: the session is ZDR and the omnibox keeps no history. */
  zdr: () => boolean
  /** The layout the dock is in. */
  dockPosition: BrowserDockPosition
  /** Asks for a layout. `hidden` collapses the rail; any other value opens the dock in that layout. */
  onDockPositionChange: (position: BrowserDockPosition) => void
  /** The stored share of the page height for the bottom layout, as CSS. */
  sizeCss?: string
  /** A canvas tab owns the rail: the pane stays mounted and is not shown. */
  hidden?: boolean
  /** Omnibox search. Defaults to the build flag VITE_BROWSER_OMNIBOX_SEARCH. */
  searchEnabled?: boolean
  /** Hands the typed text to chat: the omnibox's "Ask Verevon" row. Omit it and the row is not offered. */
  onAskVerevon?: (text: string) => void
  /** Moves focus to the chat composer. No page content is attached. */
  onAskAboutPage?: () => void
  /** Closes the browser body and returns to the run watch. */
  onClose?: () => void
  /**
   * A region of the live frame was attached (E3.3-d): the ephemeral attachment the gateway answered, for
   * the composer to carry. Omit it and the attachment is dropped once it is answered.
   */
  onRegionAttached?: (attachment: BrowserRegionAttachment) => void
}

export function InteractiveBrowserPane(props: InteractiveBrowserPaneProps): JSX.Element {
  const i18n = useI18n()
  const tr = i18n.tr
  const controller = props.controller
  // The projection is rebuilt on each preview change, so it is memoised: BrowserChrome reads one object
  // until the session really changes.
  const view = createMemo<BrowserSessionViewModel | null>(() => controller.view())

  const [omniboxValue, setOmniboxValue] = createSignal('')
  const [query, setQuery] = createSignal<string | null>(null)
  // The controller keeps a navigation failure until the next action. Dismissing hides that same failure
  // here, by identity, so a new failure shows again.
  const [dismissed, setDismissed] = createSignal<BrowserNavigationFailure | null>(null)
  const failure = (): BrowserNavigationFailure | null => {
    const current = controller.navigationError()
    return current && current !== dismissed() ? current : null
  }
  const searchEnabled = (): boolean => props.searchEnabled ?? browserOmniboxSearchEnabled()

  // Opens an address in a new session. A session that opened is recorded in the omnibox's history, which
  // the zero-retention posture keeps empty.
  const openUrl = async (url: string): Promise<void> => {
    setQuery(null)
    setOmniboxValue(url)
    await controller.open(url)
    const opened = controller.preview()
    if (opened?.browserSession) {
      recordOmniboxVisit({ url: opened.url || url, title: opened.title }, { zdr: props.zdr() })
    }
  }

  // The omnibox resolves what was typed; this only runs the result. A noop (an empty field, or search off)
  // does nothing.
  const handleIntent = (intent: OmniboxIntent): void => {
    if (intent.kind === 'navigate') {
      void openUrl(intent.url)
      return
    }
    if (intent.kind === 'search') {
      setQuery(intent.query)
      return
    }
    if (intent.kind === 'ask') props.onAskVerevon?.(intent.text)
  }

  // WS8.D: one keep-alive call for the open human session. BrowserChrome runs its timer only for a session
  // that advertises human_browsing, which Quarry does not project yet, so nothing calls this until it does.
  const keepAlive = async (): Promise<KeepAliveOutcome> => {
    const id = props.orgId()
    const sessionId = view()?.sessionId
    if (!id || !sessionId) return 'failed'
    try {
      await keepAliveBrowserSession(id, sessionId)
      return 'alive'
    } catch (error) {
      return error instanceof ApiError && error.code === 'browser_session_expired' ? 'expired' : 'failed'
    }
  }

  // Region select (E3.3-d). A drag over the live frame picks a region: the picture on screen is cropped and
  // sent to the gateway, which re-checks the tab and the region and answers an ephemeral attachment. Nothing
  // is attached without the drag. Escape cancels, and a press that selects too little does nothing.
  const [regionMode, setRegionMode] = createSignal(false)
  const [regionDrag, setRegionDrag] = createSignal<{ start: ClientPoint; end: ClientPoint } | null>(null)
  const [regionBusy, setRegionBusy] = createSignal(false)
  const [regionError, setRegionError] = createSignal<string | null>(null)
  let bodyRef: HTMLDivElement | undefined
  let layerRef: HTMLDivElement | undefined
  // The press in progress: its pointer, where it started, and the page text selected when it began.
  let press: { pointerId: number; start: ClientPoint; selection: string | null } | null = null

  // Offered only with the build flag on and on a session that is not zero retention.
  const regionSelectAvailable = (): boolean => {
    const model = view()
    return browserRegionAttachEnabled() && model !== null && !model.zdr && !props.zdr()
  }
  const liveFrame = (): HTMLImageElement | null => bodyRef?.querySelector<HTMLImageElement>(LIVE_FRAME_SELECTOR) ?? null

  const cancelRegionMode = (): void => {
    press = null
    setRegionDrag(null)
    setRegionMode(false)
  }

  // An attempt that is refused, or that reaches the gateway, ends the mode. The person selects again to retry.
  const failRegion = (message: string): void => {
    cancelRegionMode()
    setRegionError(message)
  }

  const startRegionMode = (): void => {
    setRegionError(null)
    const model = view()
    if (!model || !activeTab(model)) return
    const img = liveFrame()
    if (!img || !img.complete || !(img.naturalWidth > 0) || !(img.naturalHeight > 0)) {
      setRegionError(tr('Det er ingen sidevisning å velge fra ennå.', 'There is no page view to select from yet.'))
      return
    }
    if (!isLivePicture(img, model)) {
      setRegionError(tr(LIVE_ONLY_COPY.no, LIVE_ONLY_COPY.en))
      return
    }
    const pictureAspect = img.naturalWidth / img.naturalHeight
    const viewportAspect = model.viewport.width / model.viewport.height
    if (!(Math.abs(pictureAspect - viewportAspect) < FRAME_ASPECT_TOLERANCE)) {
      setRegionError(tr(
        'Området kan ikke velges mens siden vises i en annen størrelse.',
        'An area cannot be selected while the page is shown at another size.',
      ))
      return
    }
    setRegionMode(true)
  }

  const toggleRegionMode = (): void => {
    if (regionMode()) cancelRegionMode()
    else startRegionMode()
  }

  // Escape cancels while the mode is on. The listener lives and dies with the mode.
  createEffect(() => regionMode(), (on) => {
    if (!on) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') cancelRegionMode()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  })
  // A session that closes, or a rail that is hidden, ends the select: a later session does not open with it on.
  createEffect(() => view() === null || props.hidden === true, (ended) => {
    if (ended) cancelRegionMode()
  })

  const attachRegion = async (
    model: BrowserSessionViewModel,
    img: HTMLImageElement,
    region: RegionRect,
    selection: string | null,
  ): Promise<void> => {
    const sessionId = model.sessionId
    const tab = activeTab(model)
    if (!sessionId || !tab) {
      failRegion(tr(REGION_ATTACH_FALLBACK.no, REGION_ATTACH_FALLBACK.en))
      return
    }
    // The crop is the picture already on screen, scaled to the region. A picture the canvas will not read is refused.
    let image: RegionImagePayload | null = null
    try {
      image = cropFrameRegion({ image: img, width: img.naturalWidth, height: img.naturalHeight }, region, model.viewport)
    } catch {
      image = null
    }
    if (!image) {
      failRegion(tr('Området kunne ikke leses fra siden.', 'The area could not be read from the page.'))
      return
    }
    const built = buildRegionAttachRequest({
      tabId: tab.tabId,
      region,
      viewport: model.viewport,
      image,
      selectionText: selection,
      zeroRetention: props.zdr() || model.zdr,
    })
    if (!built.ok) {
      failRegion(regionRefusalText(built.reason, tr))
      return
    }
    setRegionBusy(true)
    try {
      const attachment = await attachBrowserRegion(sessionId, built.request)
      cancelRegionMode()
      props.onRegionAttached?.(attachment)
    } catch (error) {
      failRegion(browserErrorMessage(error, tr, REGION_ATTACH_FALLBACK))
    } finally {
      setRegionBusy(false)
    }
  }

  const onLayerPointerDown = (event: PointerEvent): void => {
    if (event.button !== 0 || regionBusy() || !liveFrame()) return
    event.preventDefault()
    try {
      layerRef?.setPointerCapture(event.pointerId)
    } catch {
      // Capture only keeps the drag on the layer; without it the drag still reads where it ends.
    }
    const start = { x: event.clientX, y: event.clientY }
    press = { pointerId: event.pointerId, start, selection: selectedPageText(bodyRef) }
    setRegionDrag({ start, end: start })
  }

  const onLayerPointerMove = (event: PointerEvent): void => {
    const current = press
    if (!current || event.pointerId !== current.pointerId) return
    setRegionDrag({ start: current.start, end: { x: event.clientX, y: event.clientY } })
  }

  const onLayerPointerUp = (event: PointerEvent): void => {
    const current = press
    if (!current || event.pointerId !== current.pointerId) return
    press = null
    setRegionDrag(null)
    const model = view()
    const img = liveFrame()
    const display = img ? frameDisplayBox(img) : null
    if (!model || !img || !display) {
      failRegion(tr('Området kunne ikke leses fra siden.', 'The area could not be read from the page.'))
      return
    }
    const region = regionFromDrag(current.start, { x: event.clientX, y: event.clientY }, display, model.viewport)
    // A click, or a drag under REGION_MIN_EDGE_PX, selects nothing: the mode stays on for another try.
    if (!region) return
    if (!isLivePicture(img, model)) {
      failRegion(tr(LIVE_ONLY_COPY.no, LIVE_ONLY_COPY.en))
      return
    }
    void attachRegion(model, img, region, current.selection)
  }

  const onLayerPointerCancel = (event: PointerEvent): void => {
    if (!press || event.pointerId !== press.pointerId) return
    press = null
    setRegionDrag(null)
  }

  return (
    <aside
      class="verevon-chat-run-panel verevon-browser-dock-pane"
      aria-label={tr('Nettleser', 'Browser')}
      data-zdr={props.zdr() ? 'on' : 'off'}
      style={{
        '--browser-dock-size': props.sizeCss ?? '40%',
        display: props.hidden ? 'none' : undefined,
      }}
    >
      <header class="verevon-browser-dock-pane__head">
        <div class="verevon-browser-dock-pane__title">
          <Globe size={15} aria-hidden="true" />
          <strong>{tr('Nettleser', 'Browser')}</strong>
        </div>
        <div class="verevon-browser-dock-pane__actions">
          <BrowserDockControls position={props.dockPosition} onPositionChange={props.onDockPositionChange} />
          <Show when={view()}>
            <AskAboutPageButton onAsk={() => props.onAskAboutPage?.()} />
          </Show>
          <Show when={regionSelectAvailable()}>
            <button
              type="button"
              class="browser-dock-pane__ask browser-dock-pane__region"
              aria-pressed={regionMode() ? 'true' : 'false'}
              disabled={regionBusy() || !activeTab(view())}
              title={activeTab(view())
                ? tr('Velg et område på siden', 'Select an area of the page')
                : tr('Ingen åpen fane å velge fra.', 'No open tab to select from.')}
              onClick={toggleRegionMode}
            >
              <ScanSearch size={13} aria-hidden="true" />
              <span>{tr('Velg område', 'Select region')}</span>
            </button>
          </Show>
          <button
            type="button"
            class="verevon-browser-dock-pane__close"
            aria-label={tr('Lukk nettleseren', 'Close the browser')}
            title={tr('Lukk nettleseren', 'Close the browser')}
            onClick={() => props.onClose?.()}
          >
            <X size={14} aria-hidden="true" />
          </button>
        </div>
      </header>

      <div class="verevon-browser-dock-pane__body" ref={(element) => { bodyRef = element }}>
        <Show when={!view()}>
          <div class="browser-dock-pane__start">
            <h2 class="browser-dock-pane__start-title">{tr('Åpne nettleseren', 'Open the browser')}</h2>
            <p class="browser-dock-pane__start-copy">
              {tr(
                'Skriv en adresse eller et søk. Dette er din egen økt: agenten bruker den ikke, og siden sendes ikke til samtalen.',
                'Type an address or a search. This is your own session: the agent does not use it, and the page is not sent to the chat.',
              )}
            </p>
            <Show when={props.zdr()}>
              <p class="browser-dock-pane__zdr">
                {tr(
                  'Midlertidig samtale: økten og besøkene lagres ikke.',
                  'Temporary chat: the session and its visits are not kept.',
                )}
              </p>
            </Show>
            <OmniboxField
              value={omniboxValue()}
              disabled={controller.busy()}
              label={tr('Nettleseradresse', 'Browser address')}
              placeholder={tr('Adresse eller søk', 'Address or search')}
              searchEnabled={searchEnabled()}
              askEnabled={props.onAskVerevon !== undefined}
              fallbackUrl={null}
              historyKey={currentOmniboxHistoryKey}
              zdr={props.zdr}
              onValueChange={setOmniboxValue}
              onIntent={handleIntent}
            />
            <Show when={query()}>
              {(text) => (
                <BrowserSearchResults
                  query={text()}
                  zdr={props.zdr()}
                  onOpen={(url) => {
                    setQuery(null)
                    void openUrl(url)
                  }}
                  onClose={() => setQuery(null)}
                />
              )}
            </Show>
            <Show when={controller.busy()}>
              <p class="browser-dock-pane__status" role="status">
                {tr('Åpner nettleseren …', 'Opening the browser …')}
              </p>
            </Show>
            <Show when={controller.error()}>
              {(message) => (
                <p class="browser-dock-pane__error" role="alert">
                  {message()}
                </p>
              )}
            </Show>
          </div>
        </Show>

        <Show when={view()}>
          {(model) => (
            <>
              <Show when={regionError()}>
                {(message) => (
                  <p class="browser-dock-pane__error" role="alert">
                    {message()}
                  </p>
                )}
              </Show>
              <Show when={Boolean(model().host) && failure()?.reason !== 'outside_scope'}>
                <p class="browser-dock-pane__limit">
                  {tr(`Denne økten er begrenset til ${model().host}.`, `This session is limited to ${model().host}.`)}
                </p>
              </Show>
              <BrowserChrome
                compact
                session={model()}
                browserApprovals={controller.approvals()}
                browserBusy={controller.busy()}
                browserLoop={controller.loop()}
                browserLoopRationale={controller.aiRunRationale()}
                decidingBrowserApprovalKeys={controller.decidingApprovalKeys()}
                browserNavigationError={failure()}
                onBrowserNavigationDismiss={() => setDismissed(controller.navigationError())}
                onBrowserNavigationRetry={(url) => void controller.runAction({ type: 'navigate', url })}
                onBrowserAction={(action) => void controller.runAction(action)}
                onBrowserControlMode={(mode, expectedEpoch) => void controller.setControlMode(mode, { expectedEpoch })}
                onKeepAlive={keepAlive}
                onBrowserNewTab={() => controller.newTab()}
                onBrowserSelectTab={(tabId) => controller.selectTab(tabId)}
                onBrowserSocketObservation={(observation, session) => controller.applySocketObservation(observation, session)}
                onDecideBrowserApproval={(approvalKey, decision) => void controller.decideApproval(approvalKey, decision)}
                onAskVerevon={props.onAskVerevon}
                searchEnabled={searchEnabled()}
              />
            </>
          )}
        </Show>
      </div>
      <Show when={regionMode() && !props.hidden && view()}>
        <Portal>
          <div
            ref={(element) => { layerRef = element }}
            class="browser-dock-region-layer"
            aria-hidden="true"
            onPointerDown={onLayerPointerDown}
            onPointerMove={onLayerPointerMove}
            onPointerUp={onLayerPointerUp}
            onPointerCancel={onLayerPointerCancel}
          >
            <Show when={regionDrag()}>
              {(drag) => <div class="browser-dock-region-rect" style={regionRectStyle(drag())} />}
            </Show>
          </div>
          <div class="browser-dock-region-hint" role="status" aria-live="polite">
            <span>
              {regionBusy()
                ? tr('Legger til området …', 'Attaching the area …')
                : tr('Dra over siden for å velge et område. Esc avbryter.', 'Drag over the page to select an area. Esc cancels.')}
            </span>
            <button type="button" class="browser-dock-pane__ask" disabled={regionBusy()} onClick={() => cancelRegionMode()}>
              {tr('Avbryt', 'Cancel')}
            </button>
          </div>
        </Portal>
      </Show>
    </aside>
  )
}
