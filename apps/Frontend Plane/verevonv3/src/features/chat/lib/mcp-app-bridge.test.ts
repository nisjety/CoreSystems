import { describe, expect, it, vi } from 'vitest'
import {
  APP_CALL_REQUIRES_CHAT,
  APP_TOOL_CALLS_PER_MINUTE,
  JSON_RPC_DECLINED,
  JSON_RPC_INVALID_PARAMS,
  JSON_RPC_METHOD_NOT_FOUND,
  MCP_APP_METHODS,
  MCP_APPS_PROTOCOL_VERSION,
  createMcpAppBridgeHost,
  createMcpAppToolCaller,
  parseMcpToolName,
  toolCallReply,
  toolResultParams,
  type JsonRpcMessage,
  type McpAppToolCaller,
} from './mcp-app-bridge'
import { createTokenBucket, MAX_WIDGET_HEIGHT } from './widget-bridge'

/** Stands in for the frame's window; the host compares `event.source` with it by identity. */
const FRAME = { name: 'app-frame' }

function host(budget = createTokenBucket(5, 60_000, () => 0)) {
  return createMcpAppBridgeHost({
    frameWindow: () => FRAME,
    messageBudget: budget,
    hostContext: { theme: 'dark', locale: 'nb' },
    toolInput: { region: 'nord' },
    toolOutput: '{"content":[{"type":"text","text":"3 rader"}],"structuredContent":{"rows":3}}',
    now: () => 0,
  })
}

const request = (id: number, method: string, params: Record<string, unknown> = {}) => ({
  source: FRAME,
  origin: 'null',
  data: { jsonrpc: '2.0', id, method, params },
})

const notify = (method: string, params: Record<string, unknown> = {}) => ({
  source: FRAME,
  origin: 'null',
  data: { jsonrpc: '2.0', method, params },
})

function sent(outcome: ReturnType<ReturnType<typeof host>['handle']>): JsonRpcMessage[] {
  if (!outcome.ok) throw new Error(`dropped: ${outcome.reason}`)
  return outcome.send
}

describe('mcp-app-bridge', () => {
  it('initialize_handshake', () => {
    const bridge = host()
    const [reply] = sent(bridge.handle(request(1, MCP_APP_METHODS.initialize, { protocolVersion: MCP_APPS_PROTOCOL_VERSION })))
    expect(reply).toMatchObject({
      jsonrpc: '2.0',
      id: 1,
      result: {
        protocolVersion: MCP_APPS_PROTOCOL_VERSION,
        hostInfo: { name: 'Verevon' },
        hostContext: { theme: 'dark', locale: 'nb', displayMode: 'inline' },
      },
    })
    // No server tools or resources are offered while app-initiated calls are off.
    const capabilities = (reply as unknown as { result: { hostCapabilities: Record<string, unknown> } }).result.hostCapabilities
    expect(Object.keys(capabilities)).toEqual(['openLinks'])
  })

  it('tool_input_and_result_sent_once', () => {
    const bridge = host()
    // Nothing goes to the app before the handshake completes.
    expect(sent(bridge.handle(notify(MCP_APP_METHODS.initialized)))).toEqual([])
    sent(bridge.handle(request(1, MCP_APP_METHODS.initialize)))
    const first = sent(bridge.handle(notify(MCP_APP_METHODS.initialized)))
    expect(first).toEqual([
      { jsonrpc: '2.0', method: MCP_APP_METHODS.toolInput, params: { arguments: { region: 'nord' } } },
      {
        jsonrpc: '2.0',
        method: MCP_APP_METHODS.toolResult,
        params: { content: [{ type: 'text', text: '3 rader' }], structuredContent: { rows: 3 } },
      },
    ])
    expect(sent(bridge.handle(notify(MCP_APP_METHODS.initialized)))).toEqual([])
  })

  it('tools_call_refused_while_s10_off', () => {
    const [reply] = sent(host().handle(request(7, MCP_APP_METHODS.toolsCall, { name: 'delete_all', arguments: {} })))
    expect(reply).toEqual({
      jsonrpc: '2.0',
      id: 7,
      error: { code: JSON_RPC_METHOD_NOT_FOUND, message: APP_CALL_REQUIRES_CHAT, data: { reason: APP_CALL_REQUIRES_CHAT } },
    })
  })

  it('unknown_method_is_method_not_found', () => {
    for (const method of ['resources/read', 'ui/request-display-mode', 'sampling/createMessage']) {
      const [reply] = sent(host().handle(request(3, method)))
      expect(reply).toMatchObject({ id: 3, error: { code: JSON_RPC_METHOD_NOT_FOUND, message: 'method_not_found' } })
    }
    // A notification is never answered, even an unknown one.
    expect(sent(host().handle(notify('notifications/message', { level: 'info' })))).toEqual([])
  })

  it('message_is_staged', () => {
    const bridge = host()
    const params = { role: 'user', content: { type: 'text', text: 'Vis som tabell' } }
    const first = bridge.handle(request(1, MCP_APP_METHODS.message, params))
    expect(first).toEqual({ ok: true, send: [{ jsonrpc: '2.0', id: 1, result: {} }], action: { kind: 'stage_message', text: 'Vis som tabell' } })
    // One staged at a time: a loop on ui/message is refused, not stacked.
    const second = bridge.handle(request(2, MCP_APP_METHODS.message, params))
    expect(second).toMatchObject({ ok: true, action: null, send: [{ id: 2, error: { message: 'message_already_staged' } }] })
    bridge.releaseMessage()
    expect(bridge.handle(request(3, MCP_APP_METHODS.message, params))).toMatchObject({ action: { kind: 'stage_message' } })
  })

  it('messages share the answer budget', () => {
    const bridge = host(createTokenBucket(1, 60_000, () => 0))
    const params = { role: 'user', content: { type: 'text', text: 'Neste' } }
    bridge.handle(request(1, MCP_APP_METHODS.message, params))
    bridge.releaseMessage()
    expect(bridge.handle(request(2, MCP_APP_METHODS.message, params))).toMatchObject({ send: [{ error: { message: 'rate_limited' } }] })
  })

  it('an over-long or non-user message is refused', () => {
    const bridge = host()
    for (const params of [
      { role: 'user', content: { type: 'text', text: 'x'.repeat(501) } },
      { role: 'assistant', content: { type: 'text', text: 'Neste' } },
      { role: 'user', content: { type: 'image', data: 'AAAA' } },
    ]) {
      expect(bridge.handle(request(1, MCP_APP_METHODS.message, params))).toMatchObject({ action: null, send: [{ error: { message: 'invalid_message' } }] })
    }
  })

  it('open_link goes through the safe-link check', () => {
    const bridge = host()
    expect(bridge.handle(request(1, MCP_APP_METHODS.openLink, { url: 'https://example.com/rapport' }))).toMatchObject({
      action: { kind: 'confirm_link', url: 'https://example.com/rapport' },
    })
    expect(bridge.handle(request(2, MCP_APP_METHODS.openLink, { url: 'javascript:alert(1)' }))).toMatchObject({
      action: null,
      send: [{ error: { message: 'unsafe_link' } }],
    })
  })

  it('size is clamped', () => {
    const outcome = host().handle(notify(MCP_APP_METHODS.sizeChanged, { width: 400, height: 99_999 }))
    expect(outcome).toEqual({ ok: true, send: [], action: { kind: 'resize', height: MAX_WIDGET_HEIGHT } })
  })

  it('foreign source and non-null origin are dropped unanswered', () => {
    const bridge = host()
    expect(bridge.handle({ ...request(1, MCP_APP_METHODS.initialize), source: {} })).toEqual({ ok: false, reason: 'foreign_source' })
    expect(bridge.handle({ ...request(1, MCP_APP_METHODS.initialize), origin: 'https://evil.example' })).toEqual({ ok: false, reason: 'non_null_origin' })
    expect(bridge.handle({ source: FRAME, origin: 'null', data: { jsonrpc: '2.0', id: 1, result: {} } })).toEqual({ ok: false, reason: 'unexpected_response' })
    expect(bridge.handle({ source: FRAME, origin: 'null', data: { v: 1, type: 'size' } })).toEqual({ ok: false, reason: 'malformed' })
    expect(bridge.handle(request(1, MCP_APP_METHODS.message, { content: { type: 'text', text: 'x'.repeat(9000) } }))).toEqual({ ok: false, reason: 'oversized' })
    expect(bridge.drops()).toEqual({ non_null_origin: 1, unexpected_response: 1, malformed: 1, oversized: 1 })
  })

  it('a plain text output is one text block', () => {
    expect(toolResultParams('3 rader')).toEqual({ content: [{ type: 'text', text: '3 rader' }] })
    expect(toolResultParams('{"rows":3}')).toEqual({ content: [{ type: 'text', text: '{"rows":3}' }] })
  })

  it('parses the namespaced tool name as execution-core does', () => {
    expect(parseMcpToolName('mcp__01HSRV__show_chart')).toEqual({ serverId: '01HSRV', tool: 'show_chart' })
    expect(parseMcpToolName('mcp__01HSRV__a__b')).toEqual({ serverId: '01HSRV', tool: 'a__b' })
    for (const name of ['web_search', 'mcp__', 'mcp__srv', 'mcp____tool', 'mcp__srv__']) {
      expect(parseMcpToolName(name)).toBeNull()
    }
  })
})

describe('mcp-app-bridge tools/call (EP-4 S10)', () => {
  /** A frame window that records what the host posts to it. */
  function frameWithInbox() {
    const inbox: unknown[] = []
    return { inbox, window: { postMessage: (message: unknown) => inbox.push(message) } }
  }

  function hostWith(frameWindow: unknown, toolCaller: McpAppToolCaller | null) {
    return createMcpAppBridgeHost({
      frameWindow: () => frameWindow,
      messageBudget: createTokenBucket(5, 60_000, () => 0),
      hostContext: { theme: 'light', locale: 'nb' },
      toolInput: {},
      toolOutput: '',
      toolCaller,
      now: () => 0,
    })
  }

  const callRequest = (source: unknown, id: number, params: Record<string, unknown>) => ({
    source,
    origin: 'null',
    data: { jsonrpc: '2.0', id, method: MCP_APP_METHODS.toolsCall, params },
  })

  const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

  it('makes no caller while the switch is off or without a run', () => {
    const call = vi.fn()
    expect(createMcpAppToolCaller({ runId: 'run-1', serverId: 'srv-1', enabled: false, call })).toBeNull()
    expect(createMcpAppToolCaller({ runId: undefined, serverId: 'srv-1', enabled: true, call })).toBeNull()
    expect(createMcpAppToolCaller({ runId: '  ', serverId: 'srv-1', enabled: true, call })).toBeNull()
    expect(call).not.toHaveBeenCalled()
  })

  it('tools_call_forwards_when_on', async () => {
    const call = vi.fn().mockResolvedValue({ ok: true, output: '{"content":[{"type":"text","text":"ok"}]}', truncated: false })
    const caller = createMcpAppToolCaller({ runId: 'run-1', serverId: 'srv-1', enabled: true, call, frameNonce: 'frame-0123456789abcdef' })
    const frame = frameWithInbox()
    const bridge = hostWith(frame.window, caller)
    // The capability is advertised only with a caller.
    const initialize = { source: frame.window, origin: 'null', data: { jsonrpc: '2.0', id: 1, method: MCP_APP_METHODS.initialize, params: {} } }
    const [init] = sent(bridge.handle(initialize))
    expect((init as unknown as { result: { hostCapabilities: Record<string, unknown> } }).result.hostCapabilities).toEqual({ openLinks: {}, serverTools: {} })

    // The app names a tool and its arguments; the run, server and nonce are the frame's.
    const outcome = bridge.handle(callRequest(frame.window, 2, { name: 'echo_read', arguments: { q: 'x' }, serverId: 'srv-2', runId: 'run-2' }))
    expect(outcome).toEqual({ ok: true, send: [], action: null })
    expect(call).toHaveBeenCalledWith({ runId: 'run-1', serverId: 'srv-1', tool: 'echo_read', arguments: { q: 'x' }, frameNonce: 'frame-0123456789abcdef' })
    await settle()
    expect(frame.inbox).toEqual([{ jsonrpc: '2.0', id: 2, result: { content: [{ type: 'text', text: 'ok' }] } }])
  })

  it('refusal_maps_to_jsonrpc_error', () => {
    expect(toolCallReply(3, { ok: false, code: APP_CALL_REQUIRES_CHAT })).toEqual({
      jsonrpc: '2.0',
      id: 3,
      error: { code: JSON_RPC_METHOD_NOT_FOUND, message: APP_CALL_REQUIRES_CHAT, data: { reason: APP_CALL_REQUIRES_CHAT } },
    })
    expect(toolCallReply(4, { ok: false, code: 'rate_limited' })).toMatchObject({ error: { code: JSON_RPC_DECLINED, message: 'rate_limited' } })
    // A code the host does not know is not passed on.
    expect(toolCallReply(5, { ok: false, code: 'registry_unavailable' })).toMatchObject({ error: { code: JSON_RPC_DECLINED, message: 'tool_call_failed' } })
    expect(toolCallReply(6, { ok: true, output: 'plain', truncated: true })).toEqual({
      jsonrpc: '2.0',
      id: 6,
      result: { content: [{ type: 'text', text: 'plain' }] },
    })
  })

  it('a failed call is answered, and a malformed one is refused before it is sent', async () => {
    const call = vi.fn().mockRejectedValue(new Error('network'))
    const caller = createMcpAppToolCaller({ runId: 'run-1', serverId: 'srv-1', enabled: true, call })
    const frame = frameWithInbox()
    const bridge = hostWith(frame.window, caller)
    for (const params of [{}, { name: '' }, { name: 'x'.repeat(257) }, { name: 'echo_read', arguments: [1] }]) {
      expect(bridge.handle(callRequest(frame.window, 1, params))).toMatchObject({
        send: [{ error: { code: JSON_RPC_INVALID_PARAMS, message: 'invalid_tool_call' } }],
      })
    }
    expect(call).not.toHaveBeenCalled()
    bridge.handle(callRequest(frame.window, 9, { name: 'echo_read' }))
    await settle()
    expect(frame.inbox).toEqual([
      { jsonrpc: '2.0', id: 9, error: { code: JSON_RPC_DECLINED, message: 'tool_call_failed', data: { reason: 'tool_call_failed' } } },
    ])
  })

  it('calls are bounded per frame', () => {
    const call = vi.fn().mockReturnValue(new Promise(() => {}))
    const caller = createMcpAppToolCaller({ runId: 'run-1', serverId: 'srv-1', enabled: true, call })
    const frame = frameWithInbox()
    const bridge = hostWith(frame.window, caller)
    for (let id = 0; id < APP_TOOL_CALLS_PER_MINUTE; id += 1) {
      expect(bridge.handle(callRequest(frame.window, id, { name: 'echo_read' }))).toEqual({ ok: true, send: [], action: null })
    }
    expect(bridge.handle(callRequest(frame.window, 99, { name: 'echo_read' }))).toMatchObject({
      send: [{ id: 99, error: { code: JSON_RPC_DECLINED, message: 'rate_limited' } }],
    })
    expect(call).toHaveBeenCalledTimes(APP_TOOL_CALLS_PER_MINUTE)
  })
})
