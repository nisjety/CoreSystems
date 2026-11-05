/**
 * MCP Apps in a chat answer (EP-4 S9): under a settled `mcp__<server>__<tool>`
 * call whose tool declares an app, the app drawn in the same opaque-origin
 * sandbox as an inline widget, fed the call's input and result, able to size
 * itself, stage a message the person sends and ask to open a link the person
 * confirms (`chat.open_mcp_app`, `chat.stage_mcp_app_message`; protocol and
 * limits in `mcp-app-bridge.ts`).
 *
 * The document is read on demand through the gateway with the viewer's own
 * authority (`getMcpAppResource`) and held only as long as the answer that drew
 * it: nothing is stored, and a temporary (zero-retention) chat draws no app.
 * The frame's CSP stays `PREVIEW_CSP`: an app that declares network origins is
 * told so in a notice and still renders without them (D12).
 */

import { createEffect, createMemo, createSignal, For, onCleanup, Show, useContext } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { useI18n } from '@/shared/i18n'
import { sandboxWidgetDocument } from '@/shared/lib/sandbox-html'
import { getMcpAppResource, type McpAppResource } from '@/shared/api/chat-client'
import { widgetThemeCss } from '../lib/inline-widget'
import { createAnswerFollowupBudget, type TokenBucket } from '../lib/widget-bridge'
import { createMcpAppBridgeHost, parseMcpToolName, type McpAppHostContext } from '../lib/mcp-app-bridge'
import { InlineWidgetSurface, WIDGET_SANDBOX, inlineWidgetsEnabled, widgetBridgeEnabled } from './InlineWidget'
import type { ChatToolCall } from './chat-types'
import './inline-widget.css'

/**
 * `VITE_MCP_APPS`, literal `'true'` only. It acts only where widgets run with
 * their bridge (`VITE_CHAT_INLINE_WIDGETS` and `VITE_CHAT_WIDGET_BRIDGE`). Off:
 * no resource is read and no frame is drawn.
 */
export const mcpAppsEnabled = () =>
  import.meta.env.VITE_MCP_APPS === 'true' && inlineWidgetsEnabled() && widgetBridgeEnabled()

/** Apps one answer draws on its own; the others wait for "Vis app". Each frame is a whole document. */
export const MAX_LIVE_APPS_PER_ANSWER = 2

/** Distinct tools one answer looks up, so a run of many MCP calls does not open a session per call. */
export const MAX_APP_LOOKUPS_PER_ANSWER = 4

export type McpAppLoader = (serverId: string, tool: string) => Promise<McpAppResource | null>

/** One read per tool for the answer's lifetime: the lookup and the frame share it. */
function answerScopedLoader(load: McpAppLoader): McpAppLoader {
  const reads = new Map<string, Promise<McpAppResource | null>>()
  return (serverId, tool) => {
    const key = `${serverId}\u0000${tool}`
    let read = reads.get(key)
    if (!read) {
      read = load(serverId, tool).catch(() => null)
      reads.set(key, read)
    }
    return read
  }
}

/** A call the app host may draw for: an MCP call that has settled with an output. */
function settledMcpCall(call: ChatToolCall): { serverId: string; tool: string } | null {
  if (!call.status || call.status === 'running' || call.status === 'error' || call.error) return null
  if (typeof call.output !== 'string') return null
  return parseMcpToolName(call.name)
}

function hostContext(locale: string): McpAppHostContext {
  const scheme = getComputedStyle(document.documentElement).getPropertyValue('color-scheme')
  return { theme: scheme.includes('dark') ? 'dark' : 'light', locale }
}

function hostThemeCss(): string {
  const style = getComputedStyle(document.documentElement)
  return widgetThemeCss((name) => style.getPropertyValue(name))
}

/**
 * The apps under one assistant answer. Drawn only on the chat surface
 * (`InlineWidgetSurface` is `live`), never in a temporary chat, and only for
 * calls whose tool declares an app; the first `MAX_LIVE_APPS_PER_ANSWER` open
 * on their own.
 */
export function McpAppsForAnswer(props: {
  calls: readonly ChatToolCall[]
  streaming?: boolean
  temporaryChat?: boolean
  onSelectFollowUp?: (text: string) => void
  load?: McpAppLoader
}): JSX.Element {
  const surfaceLive = useContext(InlineWidgetSurface) === 'live'
  const load = answerScopedLoader(props.load ?? getMcpAppResource)
  // The apps of one answer share one message budget, as its widgets share theirs.
  const messageBudget = createAnswerFollowupBudget()
  const [apps, setApps] = createSignal<ReadonlyMap<string, McpAppResource | null>>(new Map())

  const candidates = createMemo(() => {
    if (!surfaceLive || !mcpAppsEnabled() || props.temporaryChat || props.streaming) return []
    return props.calls.flatMap((call) => {
      const parsed = settledMcpCall(call)
      return parsed ? [{ call, ...parsed, key: `${parsed.serverId}\u0000${parsed.tool}` }] : []
    })
  })
  // The distinct tools to look up, first seen first. `load` reads each once, so a re-run reads nothing new.
  const lookups = createMemo(() => {
    const seen = new Map<string, { key: string; serverId: string; tool: string }>()
    for (const candidate of candidates()) {
      if (seen.size >= MAX_APP_LOOKUPS_PER_ANSWER) break
      if (!seen.has(candidate.key)) seen.set(candidate.key, { key: candidate.key, serverId: candidate.serverId, tool: candidate.tool })
    }
    return [...seen.values()]
  })

  createEffect(lookups, (pending) => {
    let live = true
    for (const lookup of pending) {
      void load(lookup.serverId, lookup.tool).then((resource) => {
        if (live) setApps((known) => new Map(known).set(lookup.key, resource))
      })
    }
    return () => {
      live = false
    }
  })

  const appCalls = createMemo(() => candidates().filter((candidate) => apps().get(candidate.key)))

  return (
    <Show when={appCalls().length > 0}>
      <div class="verevon-mcp-apps">
        <For each={appCalls()}>
          {(candidate, index) => (
            <McpAppSlot autoOpen={index() < MAX_LIVE_APPS_PER_ANSWER} tool={candidate.tool}>
              <McpAppFrame
                serverId={candidate.serverId}
                tool={candidate.tool}
                call={candidate.call}
                load={load}
                messageBudget={messageBudget}
                onSelectFollowUp={props.onSelectFollowUp}
              />
            </McpAppSlot>
          )}
        </For>
      </div>
    </Show>
  )
}

/** An app past the answer's live limit waits behind "Vis app" until the person clicks. */
function McpAppSlot(props: { autoOpen: boolean; tool: string; children: JSX.Element }): JSX.Element {
  const i18n = useI18n()
  const [opened, setOpened] = createSignal(false)
  return (
    <Show
      when={props.autoOpen || opened()}
      fallback={
        <button type="button" class="verevon-inline-widget__activate" onClick={() => setOpened(true)}>
          {i18n.tr(`Vis app (${props.tool})`, `Show app (${props.tool})`)}
        </button>
      }
    >
      {props.children}
    </Show>
  )
}

/**
 * One app. Reads its document (through `load`, `getMcpAppResource` by default);
 * a call without an app, a refused read or a failed one draws nothing, so the
 * tool call reads as it did before.
 */
export function McpAppFrame(props: {
  serverId: string
  tool: string
  call: ChatToolCall
  load?: McpAppLoader
  messageBudget?: TokenBucket
  onSelectFollowUp?: (text: string) => void
}): JSX.Element {
  const i18n = useI18n()
  const [resource, setResource] = createSignal<McpAppResource | null>(null)
  const load = props.load ?? getMcpAppResource

  // Solid v2 has no onMount: a two-phase effect with a constant compute runs once after mount.
  createEffect(() => undefined, () => {
    let live = true
    load(props.serverId, props.tool)
      .then((read) => {
        if (live) setResource(read)
      })
      .catch(() => {
        if (live) setResource(null)
      })
    return () => {
      live = false
    }
  })

  return (
    <Show when={resource()}>
      {(read) => (
        <McpAppDocument
          resource={read()}
          serverId={props.serverId}
          tool={props.tool}
          call={props.call}
          locale={i18n.locale()}
          messageBudget={props.messageBudget ?? createAnswerFollowupBudget()}
          onSelectFollowUp={props.onSelectFollowUp}
        />
      )}
    </Show>
  )
}

function McpAppDocument(props: {
  resource: McpAppResource
  serverId: string
  tool: string
  call: ChatToolCall
  locale: string
  messageBudget: TokenBucket
  onSelectFollowUp?: (text: string) => void
}): JSX.Element {
  const i18n = useI18n()
  const label = () => i18n.tr(`App fra ${props.serverId}`, `App from ${props.serverId}`)
  // The theme is read once: the frame reloads whenever srcdoc changes. No widget shim: an app brings its own
  // MCP Apps client, which speaks JSON-RPC to `parent` directly.
  const documentHtml = createMemo(() => sandboxWidgetDocument(props.resource.html, hostThemeCss()))

  let frameElement: HTMLIFrameElement | undefined
  const [height, setHeight] = createSignal<number | null>(null)
  const [stagedMessage, setStagedMessage] = createSignal<string | null>(null)
  const [pendingLink, setPendingLink] = createSignal<string | null>(null)

  const bridge = createMcpAppBridgeHost({
    frameWindow: () => frameElement?.contentWindow ?? null,
    messageBudget: props.messageBudget,
    hostContext: hostContext(props.locale),
    toolInput: props.call.args,
    toolOutput: props.call.output ?? '',
  })

  const onMessage = (event: MessageEvent) => {
    const outcome = bridge.handle(event)
    if (!outcome.ok) return
    const target = frameElement?.contentWindow
    // The target is '*' because an opaque-origin frame has no origin to name; the window is this frame's own.
    for (const reply of outcome.send) target?.postMessage(reply, '*')
    const action = outcome.action
    if (!action) return
    switch (action.kind) {
      case 'resize':
        setHeight(action.height)
        return
      case 'stage_message':
        if (!props.onSelectFollowUp) {
          bridge.releaseMessage()
          return
        }
        setStagedMessage(action.text)
        return
      case 'confirm_link':
        // One question at a time: an app cannot stack link prompts over the one the person reads.
        if (pendingLink() === null) setPendingLink(action.url)
        return
    }
  }
  window.addEventListener('message', onMessage)
  onCleanup(() => window.removeEventListener('message', onMessage))

  const clearMessage = () => {
    setStagedMessage(null)
    bridge.releaseMessage()
  }
  const sendMessage = (text: string) => {
    clearMessage()
    props.onSelectFollowUp?.(text)
  }

  return (
    <figure class="verevon-inline-widget verevon-mcp-app" data-mcp-server={props.serverId}>
      <figcaption class="verevon-inline-widget__toolbar">
        <span class="verevon-inline-widget__title">{label()}</span>
        <span class="verevon-inline-widget__note">{props.tool}</span>
      </figcaption>
      <Show when={props.resource.needsNetwork}>
        <p class="verevon-inline-widget__note" role="note">
          {i18n.tr(
            'Denne appen trenger nettverkstilgang, som ikke er tilgjengelig her',
            'This app needs network access, which is not available here',
          )}
        </p>
      </Show>
      <iframe
        ref={(element) => { frameElement = element }}
        class="verevon-inline-widget__frame"
        title={label()}
        sandbox={WIDGET_SANDBOX}
        srcdoc={documentHtml()}
        referrerpolicy="no-referrer"
        loading="lazy"
        style={height() !== null ? { height: `${height()}px` } : undefined}
      />
      <Show when={stagedMessage()}>
        {(text) => (
          <div class="verevon-chat-followups" role="group" aria-label={i18n.tr('Forslag fra appen', 'Suggestion from the app')}>
            <span class="verevon-inline-widget__note">{i18n.tr('Forslag fra appen:', 'Suggestion from the app:')}</span>
            <button type="button" class="verevon-chat-followup-chip" onClick={() => sendMessage(text())}>
              {text()}
            </button>
            <button type="button" class="verevon-inline-widget__action" onClick={clearMessage}>
              {i18n.tr('Avvis', 'Dismiss')}
            </button>
          </div>
        )}
      </Show>
      <Show when={pendingLink()}>
        {(url) => (
          <div class="verevon-chat-followups" role="group" aria-label={i18n.tr('Lenke fra appen', 'Link from the app')}>
            <span class="verevon-inline-widget__note">
              {i18n.tr('Appen vil åpne:', 'The app wants to open:')} {url()}
            </span>
            <a
              class="verevon-chat-followup-chip"
              href={url()}
              target="_blank"
              rel="noopener noreferrer"
              onClick={() => setPendingLink(null)}
            >
              {i18n.tr('Åpne lenken', 'Open the link')}
            </a>
            <button type="button" class="verevon-inline-widget__action" onClick={() => setPendingLink(null)}>
              {i18n.tr('Avbryt', 'Cancel')}
            </button>
          </div>
        )}
      </Show>
    </figure>
  )
}
