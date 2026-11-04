import { createMemo, createSignal, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { BrowserChrome } from '@/features/dashboard/home/BrowserChrome'
import type { BrowserSessionViewModel } from '@/features/dashboard/home/browser-session'
import type { BrowserNavigationFailure } from '@/features/browser/session/browser-navigation-error'
import type { BrowserSessionController } from '@/features/browser/session/use-browser-session-controller'
import type { KeepAliveOutcome } from '@/features/browser/session/session-keepalive'
import { BrowserSearchResults } from '@/features/browser/omnibox/BrowserSearchResults'
import { OmniboxField } from '@/features/browser/omnibox/OmniboxField'
import { currentOmniboxHistoryKey, recordOmniboxVisit } from '@/features/browser/omnibox/omnibox-history'
import { browserOmniboxSearchEnabled, type OmniboxIntent } from '@/features/browser/omnibox/omnibox-resolver'
import { keepAliveBrowserSession } from '@/shared/api/browser-client'
import { ApiError } from '@/shared/api/http'
import { useI18n } from '@/shared/i18n'
import { X, Globe } from '@/shared/icons'
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

      <div class="verevon-browser-dock-pane__body">
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
    </aside>
  )
}
