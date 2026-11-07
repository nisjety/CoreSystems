import { describe, expect, it } from 'vitest'
import {
  BRIDGE_SHIM,
  FOLLOWUPS_PER_ANSWER_PER_MINUTE,
  MAX_BRIDGE_MESSAGE_BYTES,
  MAX_FOLLOWUP_CHARS,
  MAX_WIDGET_HEIGHT,
  MIN_WIDGET_HEIGHT,
  SIZE_MESSAGES_PER_SECOND,
  createAnswerFollowupBudget,
  createWidgetBridgeHost,
  parseWidgetBridgeEvent,
  type WidgetBridgeEvent,
} from './widget-bridge'

/** Stands in for the frame's `contentWindow`; the bridge only compares identity. */
const FRAME = { name: 'frame' }

function event(data: unknown, overrides: Partial<WidgetBridgeEvent> = {}): WidgetBridgeEvent {
  return { source: FRAME, origin: 'null', data, ...overrides }
}

function envelope(type: string, payload: unknown = {}) {
  return { v: 1, type, payload }
}

function clock(start = 0) {
  let at = start
  return { now: () => at, advance: (ms: number) => { at += ms } }
}

describe('parseWidgetBridgeEvent', () => {
  it('foreign source is ignored', () => {
    expect(parseWidgetBridgeEvent(event(envelope('ready'), { source: { name: 'other' } }), FRAME)).toEqual({ ok: false, reason: 'foreign_source' })
    expect(parseWidgetBridgeEvent(event(envelope('ready'), { source: null }), FRAME)).toEqual({ ok: false, reason: 'foreign_source' })
    // A frame that is not mounted has no window: nothing is accepted, not even a null source.
    expect(parseWidgetBridgeEvent(event(envelope('ready'), { source: null }), null)).toEqual({ ok: false, reason: 'foreign_source' })
  })

  it('non null origin is ignored', () => {
    for (const origin of ['http://localhost:5173', 'https://evil.example', '']) {
      expect(parseWidgetBridgeEvent(event(envelope('ready'), { origin }), FRAME)).toEqual({ ok: false, reason: 'non_null_origin' })
    }
  })

  it('unknown type is dropped', () => {
    expect(parseWidgetBridgeEvent(event(envelope('fetch', { url: 'https://x.example' })), FRAME)).toEqual({ ok: false, reason: 'unknown_type' })
    expect(parseWidgetBridgeEvent(event(envelope('storage_set')), FRAME)).toEqual({ ok: false, reason: 'unknown_type' })
  })

  it('a message that is not the version 1 envelope is malformed', () => {
    for (const data of [
      'size',
      ['size'],
      null,
      { type: 'size', payload: { height: 200 } },
      { v: 2, type: 'size', payload: { height: 200 } },
      { v: 1, type: 7 },
      { v: 1, type: 'ready', id: 9 },
      { v: 1, type: 'ready', id: 'x'.repeat(65) },
    ]) {
      expect(parseWidgetBridgeEvent(event(data), FRAME)).toEqual({ ok: false, reason: 'malformed' })
    }
    const cyclic: Record<string, unknown> = { v: 1, type: 'ready' }
    cyclic.self = cyclic
    expect(parseWidgetBridgeEvent(event(cyclic), FRAME)).toEqual({ ok: false, reason: 'malformed' })
  })

  it('oversized payload is dropped', () => {
    const text = 'a'.repeat(MAX_BRIDGE_MESSAGE_BYTES)
    expect(parseWidgetBridgeEvent(event(envelope('followup', { text })), FRAME)).toEqual({ ok: false, reason: 'oversized' })
    // The limit is bytes, not characters: multi-byte text reaches it sooner.
    const wide = 'ø'.repeat(MAX_BRIDGE_MESSAGE_BYTES / 2)
    expect(parseWidgetBridgeEvent(event(envelope('followup', { text: wide })), FRAME)).toEqual({ ok: false, reason: 'oversized' })
  })

  it('height is clamped', () => {
    const height = (value: unknown) => parseWidgetBridgeEvent(event(envelope('size', { height: value })), FRAME)
    expect(height(240)).toEqual({ ok: true, message: { type: 'size', id: undefined, height: 240 } })
    expect(height(10)).toMatchObject({ ok: true, message: { height: MIN_WIDGET_HEIGHT } })
    expect(height(-5)).toMatchObject({ ok: true, message: { height: MIN_WIDGET_HEIGHT } })
    expect(height(99_999)).toMatchObject({ ok: true, message: { height: MAX_WIDGET_HEIGHT } })
    expect(height(200.6)).toMatchObject({ ok: true, message: { height: 201 } })
    for (const bad of [Number.NaN, Number.POSITIVE_INFINITY, '300', null]) {
      expect(height(bad)).toEqual({ ok: false, reason: 'invalid_payload' })
    }
  })

  it('a follow-up is trimmed, non-empty and at most the limit', () => {
    expect(parseWidgetBridgeEvent(event(envelope('followup', { text: '  Vis for 25 år ' })), FRAME)).toEqual({
      ok: true,
      message: { type: 'followup', id: undefined, text: 'Vis for 25 år' },
    })
    expect(parseWidgetBridgeEvent(event(envelope('followup', { text: 'x'.repeat(MAX_FOLLOWUP_CHARS) })), FRAME).ok).toBe(true)
    for (const text of ['   ', 'x'.repeat(MAX_FOLLOWUP_CHARS + 1), 42]) {
      expect(parseWidgetBridgeEvent(event(envelope('followup', { text })), FRAME)).toEqual({ ok: false, reason: 'invalid_payload' })
    }
    expect(parseWidgetBridgeEvent(event(envelope('followup', 'text')), FRAME)).toEqual({ ok: false, reason: 'invalid_payload' })
  })

  it('a link must be a safe link', () => {
    expect(parseWidgetBridgeEvent(event(envelope('open_link', { url: 'https://example.com/rente' })), FRAME)).toEqual({
      ok: true,
      message: { type: 'open_link', id: undefined, url: 'https://example.com/rente' },
    })
    for (const url of ['javascript:alert(1)', 'data:text/html,x', '/api/v1/me', '//evil.example', 'java\nscript:alert(1)']) {
      expect(parseWidgetBridgeEvent(event(envelope('open_link', { url })), FRAME)).toEqual({ ok: false, reason: 'unsafe_link' })
    }
  })
})

describe('createWidgetBridgeHost', () => {
  function host(now = clock()) {
    const budget = createAnswerFollowupBudget(now.now)
    return { bridge: createWidgetBridgeHost({ frameWindow: () => FRAME, followupBudget: budget, now: now.now }), budget, now }
  }

  it('followups are rate limited', () => {
    const { bridge, now } = host()
    const suggest = () => bridge.handle(event(envelope('followup', { text: 'Vis for 25 år' })))

    // One staged at a time: a loop on suggest() shows one chip.
    expect(suggest()).toEqual({ ok: true, action: { kind: 'stage_followup', text: 'Vis for 25 år' } })
    for (let i = 0; i < 20; i += 1) expect(suggest()).toEqual({ ok: false, reason: 'followup_already_staged' })

    // Each send frees the slot, but the answer's budget holds at five a minute.
    for (let i = 1; i < FOLLOWUPS_PER_ANSWER_PER_MINUTE; i += 1) {
      bridge.releaseFollowup()
      expect(suggest().ok).toBe(true)
    }
    bridge.releaseFollowup()
    expect(suggest()).toEqual({ ok: false, reason: 'rate_limited' })

    now.advance(60_000 / FOLLOWUPS_PER_ANSWER_PER_MINUTE)
    expect(suggest().ok).toBe(true)
    expect(bridge.drops()).toMatchObject({ followup_already_staged: 20, rate_limited: 1 })
  })

  it('the follow-up budget is shared by the widgets of one answer', () => {
    const now = clock()
    const budget = createAnswerFollowupBudget(now.now)
    const frames = Array.from({ length: FOLLOWUPS_PER_ANSWER_PER_MINUTE + 1 }, () =>
      createWidgetBridgeHost({ frameWindow: () => FRAME, followupBudget: budget, now: now.now }),
    )
    const results = frames.map((frame) => frame.handle(event(envelope('followup', { text: 'Neste' }))).ok)
    expect(results.filter(Boolean)).toHaveLength(FOLLOWUPS_PER_ANSWER_PER_MINUTE)
    expect(results.at(-1)).toBe(false)
  })

  it('size messages are limited per second', () => {
    const { bridge, now } = host()
    const resize = () => bridge.handle(event(envelope('size', { height: 300 })))
    for (let i = 0; i < SIZE_MESSAGES_PER_SECOND; i += 1) {
      expect(resize()).toEqual({ ok: true, action: { kind: 'resize', height: 300 } })
    }
    expect(resize()).toEqual({ ok: false, reason: 'rate_limited' })
    now.advance(1000)
    expect(resize().ok).toBe(true)
  })

  it('ready needs no action and a safe link asks for confirmation', () => {
    const { bridge } = host()
    expect(bridge.handle(event(envelope('ready')))).toEqual({ ok: true, action: null })
    expect(bridge.handle(event(envelope('open_link', { url: 'https://example.com' })))).toEqual({
      ok: true,
      action: { kind: 'confirm_link', url: 'https://example.com' },
    })
  })

  it('counts drops per reason but not other frames’ traffic', () => {
    const { bridge } = host()
    bridge.handle(event(envelope('nope')))
    bridge.handle(event(envelope('open_link', { url: 'javascript:x' })))
    bridge.handle(event(envelope('ready'), { source: { name: 'other' } }))
    expect(bridge.drops()).toEqual({ unknown_type: 1, unsafe_link: 1 })
  })
})

describe('BRIDGE_SHIM', () => {
  it('cannot end the script element it is written into', () => {
    expect(BRIDGE_SHIM.toLowerCase()).not.toContain('</script')
  })

  it('posts version 1 envelopes for the three widget calls and ready', () => {
    const posted: Array<{ data: unknown; target: string }> = []
    const scope: { verevon?: { resize(h: unknown): void; suggest(t: unknown): void; openLink(u: unknown): void } } = {}
    const parent = { postMessage: (data: unknown, target: string) => posted.push({ data, target }) }
    new Function('window', 'parent', BRIDGE_SHIM)(scope, parent)
    scope.verevon?.resize(240)
    scope.verevon?.suggest('Vis for 25 år')
    scope.verevon?.openLink('https://example.com')
    expect(posted).toEqual([
      { data: { v: 1, type: 'ready', payload: {} }, target: '*' },
      { data: { v: 1, type: 'size', payload: { height: 240 } }, target: '*' },
      { data: { v: 1, type: 'followup', payload: { text: 'Vis for 25 år' } }, target: '*' },
      { data: { v: 1, type: 'open_link', payload: { url: 'https://example.com' } }, target: '*' },
    ])
    expect(Object.isFrozen(scope.verevon)).toBe(true)
  })
})
