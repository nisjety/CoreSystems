/**
 * The MCP App host's side of the frame (EP-4 S9): JSON-RPC 2.0 over postMessage,
 * as the MCP Apps extension defines it, held to the inline widget bridge's limits.
 *
 * An MCP tool may name an HTML app that draws its result. The app runs in the
 * same opaque-origin sandbox as an inline widget (`sandbox="allow-scripts"`,
 * `connect-src 'none'`), so everything it can do goes through this host: it is
 * told the call's input and result once, may size itself, may ask to open a safe
 * link the person confirms, and may stage a message the person sends (D3). It
 * gets no storage, no network and no host data. A `tools/call` from the app is
 * refused with `app_call_requires_chat` until app-initiated calls exist (S10);
 * every other request is method-not-found.
 *
 * The extension's method names live in `MCP_APP_METHODS` only (checked against
 * the 2026-01-26 revision), because the extension is young and they may move.
 *
 * Pure: the host takes its frame, clock and budgets as arguments, so every rule
 * is testable without a frame.
 */

import { isSafeLinkHref } from './safe-href'
import {
  clampWidgetHeight,
  createTokenBucket,
  MAX_BRIDGE_MESSAGE_BYTES,
  MAX_FOLLOWUP_CHARS,
  SIZE_MESSAGES_PER_SECOND,
  type TokenBucket,
  type WidgetBridgeEvent,
} from './widget-bridge'

/** The extension revision this host speaks; sent back in the `ui/initialize` result. */
export const MCP_APPS_PROTOCOL_VERSION = '2026-01-26'

/** Every extension method the host reads or sends. A name not here is method-not-found. */
export const MCP_APP_METHODS = {
  /** App to host request: the handshake. */
  initialize: 'ui/initialize',
  /** App to host notification: the app is ready; only then does the host send anything. */
  initialized: 'ui/notifications/initialized',
  /** Host to app notification: the call's complete arguments, once. */
  toolInput: 'ui/notifications/tool-input',
  /** Host to app notification: the call's `CallToolResult`, once, after the input. */
  toolResult: 'ui/notifications/tool-result',
  /** App to host notification: the app's rendered size. */
  sizeChanged: 'ui/notifications/size-changed',
  /** App to host request: open a link (the person confirms it). */
  openLink: 'ui/open-link',
  /** App to host request: a user message (staged as a chip the person sends). */
  message: 'ui/message',
  /** App to host request: call a tool on the app's server (refused until S10). */
  toolsCall: 'tools/call',
  /** App to host request: liveness. */
  ping: 'ping',
} as const

/** JSON-RPC 2.0 error codes the host answers with. */
export const JSON_RPC_METHOD_NOT_FOUND = -32601
export const JSON_RPC_INVALID_PARAMS = -32602
/** Implementation-defined server error: the request was valid but the host declined it now. */
export const JSON_RPC_DECLINED = -32000

/** The refusal an app reads for `tools/call` while app-initiated calls are off (D13). */
export const APP_CALL_REQUIRES_CHAT = 'app_call_requires_chat'

/** Longer than any request id an app needs; bounds what the host echoes back. */
const MAX_ID_CHARS = 64

export type JsonRpcId = string | number

export type JsonRpcMessage =
  | { jsonrpc: '2.0'; id: JsonRpcId; result: Record<string, unknown> }
  | { jsonrpc: '2.0'; id: JsonRpcId; error: { code: number; message: string; data?: Record<string, unknown> } }
  | { jsonrpc: '2.0'; method: string; params: Record<string, unknown> }

/** Why a message was dropped without an answer. Counted per reason, never shown to the app. */
export type McpAppBridgeDrop = 'foreign_source' | 'non_null_origin' | 'malformed' | 'oversized' | 'unexpected_response'

/** The context `ui/initialize` hands the app: the theme and how it is shown, nothing about the person. */
export type McpAppHostContext = {
  theme: 'light' | 'dark'
  locale: string
}

/** What the host does with an accepted message, besides answering it. */
export type McpAppAction =
  | { kind: 'resize'; height: number }
  | { kind: 'stage_message'; text: string }
  | { kind: 'confirm_link'; url: string }

export type McpAppBridgeOutcome =
  | { ok: true; send: JsonRpcMessage[]; action: McpAppAction | null }
  | { ok: false; reason: McpAppBridgeDrop }

export type McpAppBridgeHost = {
  handle(event: WidgetBridgeEvent): McpAppBridgeOutcome
  /** The person sent or dismissed the staged message: the app may stage another. */
  releaseMessage(): void
  /** Drops per reason since the frame mounted. */
  drops(): Readonly<Partial<Record<McpAppBridgeDrop, number>>>
}

type Inbound =
  | { kind: 'request'; id: JsonRpcId; method: string; params: Record<string, unknown> }
  | { kind: 'notification'; method: string; params: Record<string, unknown> }

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false
  const proto = Object.getPrototypeOf(value)
  return proto === Object.prototype || proto === null
}

/** UTF-8 size of the message as JSON; null when it does not serialize (cycles, BigInt). */
function serializedBytes(data: unknown): number | null {
  try {
    const json = JSON.stringify(data)
    return json === undefined ? null : new TextEncoder().encode(json).length
  } catch {
    return null
  }
}

function isJsonRpcId(value: unknown): value is JsonRpcId {
  if (typeof value === 'string') return value.length <= MAX_ID_CHARS
  return typeof value === 'number' && Number.isSafeInteger(value)
}

/**
 * Read one posted message as JSON-RPC. Accepted only from this frame's own window
 * and from the opaque origin (`'null'`) a sandboxed frame without
 * `allow-same-origin` always has; anything else is dropped before its data is read.
 */
function parseInbound(event: WidgetBridgeEvent, frameWindow: unknown): { ok: true; message: Inbound } | { ok: false; reason: McpAppBridgeDrop } {
  if (frameWindow == null || event.source !== frameWindow) return { ok: false, reason: 'foreign_source' }
  if (event.origin !== 'null') return { ok: false, reason: 'non_null_origin' }

  const data = event.data
  if (!isPlainObject(data)) return { ok: false, reason: 'malformed' }
  const bytes = serializedBytes(data)
  if (bytes === null) return { ok: false, reason: 'malformed' }
  if (bytes > MAX_BRIDGE_MESSAGE_BYTES) return { ok: false, reason: 'oversized' }
  if (data.jsonrpc !== '2.0') return { ok: false, reason: 'malformed' }
  // The host sends no requests, so an app has nothing to answer.
  if (typeof data.method !== 'string') {
    return 'result' in data || 'error' in data ? { ok: false, reason: 'unexpected_response' } : { ok: false, reason: 'malformed' }
  }
  const params = data.params === undefined ? {} : data.params
  if (!isPlainObject(params)) return { ok: false, reason: 'malformed' }
  if (data.id === undefined) return { ok: true, message: { kind: 'notification', method: data.method, params } }
  if (!isJsonRpcId(data.id)) return { ok: false, reason: 'malformed' }
  return { ok: true, message: { kind: 'request', id: data.id, method: data.method, params } }
}

const result = (id: JsonRpcId, value: Record<string, unknown> = {}): JsonRpcMessage => ({ jsonrpc: '2.0', id, result: value })

const failure = (id: JsonRpcId, code: number, message: string): JsonRpcMessage => ({
  jsonrpc: '2.0',
  id,
  error: { code, message, data: { reason: message } },
})

const notification = (method: string, params: Record<string, unknown>): JsonRpcMessage => ({ jsonrpc: '2.0', method, params })

/**
 * The text of a `ui/message`: a user-role text block (the extension sends one
 * block; a list of blocks is read for its text blocks). Null when there is no
 * text or it is over the follow-up limit, which is dropped, not cut, so the chip
 * never misquotes.
 */
export function stagedMessageText(params: Record<string, unknown>): string | null {
  if (params.role !== undefined && params.role !== 'user') return null
  const blocks = Array.isArray(params.content) ? params.content : [params.content]
  const text = blocks
    .filter((block): block is Record<string, unknown> => isPlainObject(block) && block.type === 'text' && typeof block.text === 'string')
    .map((block) => block.text as string)
    .join('\n')
    .trim()
  return text && text.length <= MAX_FOLLOWUP_CHARS ? text : null
}

/**
 * A tool's stored output as the `CallToolResult` the app reads: an output that
 * already is one (a `content` list) goes as it is, anything else as one text block.
 */
export function toolResultParams(output: string): Record<string, unknown> {
  try {
    const parsed: unknown = JSON.parse(output)
    if (isPlainObject(parsed) && Array.isArray(parsed.content)) return parsed
  } catch {
    // Not JSON: the output is plain text.
  }
  return { content: [{ type: 'text', text: output }] }
}

/**
 * One frame's side of the app protocol: the source and origin check, the size
 * cap, the handshake, the call's input and result sent once after the app says
 * it is initialized, one staged message at a time against the answer's shared
 * budget, and the frame's own size budget.
 */
export function createMcpAppBridgeHost(options: {
  frameWindow: () => unknown
  messageBudget: TokenBucket
  hostContext: McpAppHostContext
  /** The settled call's arguments, as the transcript holds them. */
  toolInput: unknown
  /** The settled call's output text. */
  toolOutput: string
  now?: () => number
}): McpAppBridgeHost {
  const sizeBudget = createTokenBucket(SIZE_MESSAGES_PER_SECOND, 1000, options.now)
  const dropped: Partial<Record<McpAppBridgeDrop, number>> = {}
  let initializeAnswered = false
  let callSent = false
  let messageStaged = false

  const drop = (reason: McpAppBridgeDrop): McpAppBridgeOutcome => {
    dropped[reason] = (dropped[reason] ?? 0) + 1
    return { ok: false, reason }
  }
  const answer = (send: JsonRpcMessage[], action: McpAppAction | null = null): McpAppBridgeOutcome => ({ ok: true, send, action })

  const handleRequest = (id: JsonRpcId, method: string, params: Record<string, unknown>): McpAppBridgeOutcome => {
    switch (method) {
      case MCP_APP_METHODS.initialize:
        initializeAnswered = true
        return answer([
          result(id, {
            protocolVersion: MCP_APPS_PROTOCOL_VERSION,
            hostInfo: { name: 'Verevon', version: '1' },
            // Links only: no server tools or resources (S10), no logging, no display modes beyond inline.
            hostCapabilities: { openLinks: {} },
            hostContext: {
              theme: options.hostContext.theme,
              locale: options.hostContext.locale,
              displayMode: 'inline',
              availableDisplayModes: ['inline'],
              platform: 'web',
            },
          }),
        ])
      case MCP_APP_METHODS.ping:
        return answer([result(id)])
      case MCP_APP_METHODS.openLink: {
        const url = typeof params.url === 'string' ? params.url.trim() : ''
        if (!isSafeLinkHref(url)) return answer([failure(id, JSON_RPC_INVALID_PARAMS, 'unsafe_link')])
        return answer([result(id)], { kind: 'confirm_link', url })
      }
      case MCP_APP_METHODS.message: {
        const text = stagedMessageText(params)
        if (text === null) return answer([failure(id, JSON_RPC_INVALID_PARAMS, 'invalid_message')])
        // The slot is checked first, so an app looping on ui/message does not spend the answer's budget.
        if (messageStaged) return answer([failure(id, JSON_RPC_DECLINED, 'message_already_staged')])
        if (!options.messageBudget.take()) return answer([failure(id, JSON_RPC_DECLINED, 'rate_limited')])
        messageStaged = true
        return answer([result(id)], { kind: 'stage_message', text })
      }
      case MCP_APP_METHODS.toolsCall:
        return answer([failure(id, JSON_RPC_METHOD_NOT_FOUND, APP_CALL_REQUIRES_CHAT)])
      default:
        return answer([failure(id, JSON_RPC_METHOD_NOT_FOUND, 'method_not_found')])
    }
  }

  const handleNotification = (method: string, params: Record<string, unknown>): McpAppBridgeOutcome => {
    switch (method) {
      case MCP_APP_METHODS.initialized: {
        // Nothing goes to the app before the handshake completes, and the call goes once.
        if (!initializeAnswered || callSent) return answer([])
        callSent = true
        const input = isPlainObject(options.toolInput) ? options.toolInput : {}
        return answer([
          notification(MCP_APP_METHODS.toolInput, { arguments: input }),
          notification(MCP_APP_METHODS.toolResult, toolResultParams(options.toolOutput)),
        ])
      }
      case MCP_APP_METHODS.sizeChanged: {
        const height = clampWidgetHeight(params.height)
        if (height === null) return answer([])
        // A resize loop beyond the budget is ignored, not echoed.
        return sizeBudget.take() ? answer([], { kind: 'resize', height }) : answer([])
      }
      default:
        // JSON-RPC never answers a notification; an unknown one is ignored.
        return answer([])
    }
  }

  return {
    handle(event) {
      const parsed = parseInbound(event, options.frameWindow())
      // A foreign source is another frame's or the page's own traffic, not this app's to count.
      if (!parsed.ok) return parsed.reason === 'foreign_source' ? parsed : drop(parsed.reason)
      const message = parsed.message
      return message.kind === 'request'
        ? handleRequest(message.id, message.method, message.params)
        : handleNotification(message.method, message.params)
    },
    releaseMessage() {
      messageStaged = false
    },
    drops() {
      return { ...dropped }
    },
  }
}

/**
 * Split `mcp__<server_id>__<tool>` into its parts, as execution-core's
 * `parse_mcp_tool_name` does: on the FIRST `__` after the prefix (a tool name may
 * itself contain `__`; a server id does not). Null for any other name.
 */
export function parseMcpToolName(name: string): { serverId: string; tool: string } | null {
  if (!name.startsWith('mcp__')) return null
  const rest = name.slice('mcp__'.length)
  const split = rest.indexOf('__')
  if (split <= 0) return null
  const tool = rest.slice(split + 2)
  return tool ? { serverId: rest.slice(0, split), tool } : null
}
