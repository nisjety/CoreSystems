import { describe, expect, it } from 'vitest'
import {
  APP_CALL_REQUIRES_CHAT,
  JSON_RPC_METHOD_NOT_FOUND,
  MCP_APP_METHODS,
  MCP_APPS_PROTOCOL_VERSION,
  createMcpAppBridgeHost,
  parseMcpToolName,
  toolResultParams,
  type JsonRpcMessage,
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
