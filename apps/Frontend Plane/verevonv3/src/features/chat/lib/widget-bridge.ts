/**
 * The narrow widget bridge (EP-4 S3): the only messages an inline widget may post
 * to the host, and the limits the host holds it to.
 *
 * A widget may size itself, propose a follow-up the person may send, and ask to
 * open a safe link. Nothing else crosses the frame: the host sends nothing back,
 * the widget gets no storage, no network and no host data. A follow-up is staged
 * as a chip the person clicks (D3), never sent by the widget, so a widget can
 * never drive the model on its own; a link opens only from the person's click on
 * the host's own confirmation.
 *
 * Pure: the host state takes its clock and frame as arguments, so every limit is
 * testable without a frame.
 */

import { isSafeLinkHref } from './safe-href'

/** The envelope version. A message without `v: 1` is dropped. */
export const WIDGET_BRIDGE_VERSION = 1

/** The serialized envelope's size limit: the bridge carries a height, a sentence or a URL, never content. */
export const MAX_BRIDGE_MESSAGE_BYTES = 8 * 1024

/** The height a widget may ask for: below 80 px it would vanish, above 1200 px it would bury the answer. */
export const MIN_WIDGET_HEIGHT = 80
export const MAX_WIDGET_HEIGHT = 1200

/** A follow-up is a question, not a document. Longer ones are dropped, not cut, so the chip never misquotes. */
export const MAX_FOLLOWUP_CHARS = 500

/** Follow-ups one answer's widgets may stage per minute, together. */
export const FOLLOWUPS_PER_ANSWER_PER_MINUTE = 5

/** Size messages one frame may send per second; a resize loop beyond it is ignored, not echoed. */
export const SIZE_MESSAGES_PER_SECOND = 20

/** Longer than any request id a widget needs; bounds what the host keeps of it. */
const MAX_ID_CHARS = 64

/**
 * The script the frame gets before the widget when the bridge is on. It only
 * posts envelopes; the target is `'*'` because an opaque-origin frame has no
 * origin to name, and the host checks `event.source` instead.
 */
export const BRIDGE_SHIM =
  '(function(){' +
  'var post=function(type,payload){parent.postMessage({v:1,type:type,payload:payload},"*")};' +
  'window.verevon=Object.freeze({' +
  'resize:function(h){post("size",{height:Number(h)})},' +
  'suggest:function(t){post("followup",{text:String(t)})},' +
  'openLink:function(u){post("open_link",{url:String(u)})}' +
  '});' +
  'post("ready",{})' +
  '})()'

export type WidgetBridgeMessage =
  | { type: 'ready'; id?: string }
  | { type: 'size'; id?: string; height: number }
  | { type: 'followup'; id?: string; text: string }
  | { type: 'open_link'; id?: string; url: string }

/** Why a message was dropped. Kept as a count per reason, never shown to the widget. */
export type WidgetBridgeDrop =
  | 'foreign_source'
  | 'non_null_origin'
  | 'malformed'
  | 'oversized'
  | 'unknown_type'
  | 'invalid_payload'
  | 'unsafe_link'
  | 'rate_limited'
  | 'followup_already_staged'

export type WidgetBridgeParse = { ok: true; message: WidgetBridgeMessage } | { ok: false; reason: WidgetBridgeDrop }

/** The parts of a `MessageEvent` the bridge reads. */
export type WidgetBridgeEvent = { source: unknown; origin: string; data: unknown }

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false
  const proto = Object.getPrototypeOf(value)
  return proto === Object.prototype || proto === null
}

/** UTF-8 size of the envelope as JSON; null when it does not serialize (cycles, BigInt). */
function serializedBytes(data: unknown): number | null {
  try {
    const json = JSON.stringify(data)
    return json === undefined ? null : new TextEncoder().encode(json).length
  } catch {
    return null
  }
}

/** A height in px, clamped; null when it is not a finite number. */
export function clampWidgetHeight(raw: unknown): number | null {
  if (typeof raw !== 'number' || !Number.isFinite(raw)) return null
  return Math.min(MAX_WIDGET_HEIGHT, Math.max(MIN_WIDGET_HEIGHT, Math.round(raw)))
}

/**
 * Read one posted message. Accepted only from this frame's own window
 * (`frameWindow`) and from the opaque origin (`'null'`) a sandboxed frame without
 * `allow-same-origin` always has; anything else is dropped before its data is read.
 */
export function parseWidgetBridgeEvent(event: WidgetBridgeEvent, frameWindow: unknown): WidgetBridgeParse {
  if (frameWindow == null || event.source !== frameWindow) return { ok: false, reason: 'foreign_source' }
  if (event.origin !== 'null') return { ok: false, reason: 'non_null_origin' }

  const data = event.data
  if (!isPlainObject(data)) return { ok: false, reason: 'malformed' }
  const bytes = serializedBytes(data)
  if (bytes === null) return { ok: false, reason: 'malformed' }
  if (bytes > MAX_BRIDGE_MESSAGE_BYTES) return { ok: false, reason: 'oversized' }
  if (data.v !== WIDGET_BRIDGE_VERSION || typeof data.type !== 'string') return { ok: false, reason: 'malformed' }
  if (data.id !== undefined && (typeof data.id !== 'string' || data.id.length > MAX_ID_CHARS)) {
    return { ok: false, reason: 'malformed' }
  }
  const id = data.id as string | undefined
  const payload = data.payload === undefined ? {} : data.payload
  if (!isPlainObject(payload)) return { ok: false, reason: 'invalid_payload' }

  switch (data.type) {
    case 'ready':
      return { ok: true, message: { type: 'ready', id } }
    case 'size': {
      const height = clampWidgetHeight(payload.height)
      if (height === null) return { ok: false, reason: 'invalid_payload' }
      return { ok: true, message: { type: 'size', id, height } }
    }
    case 'followup': {
      const text = typeof payload.text === 'string' ? payload.text.trim() : ''
      if (!text || text.length > MAX_FOLLOWUP_CHARS) return { ok: false, reason: 'invalid_payload' }
      return { ok: true, message: { type: 'followup', id, text } }
    }
    case 'open_link': {
      if (typeof payload.url !== 'string') return { ok: false, reason: 'invalid_payload' }
      const url = payload.url.trim()
      if (!isSafeLinkHref(url)) return { ok: false, reason: 'unsafe_link' }
      return { ok: true, message: { type: 'open_link', id, url } }
    }
    default:
      return { ok: false, reason: 'unknown_type' }
  }
}

export type TokenBucket = { take(): boolean }

/** `capacity` tokens, refilled evenly over `periodMs`; a take with no whole token left fails. */
export function createTokenBucket(capacity: number, periodMs: number, now: () => number = Date.now): TokenBucket {
  let tokens = capacity
  let last = now()
  return {
    take() {
      const at = now()
      tokens = Math.min(capacity, tokens + ((at - last) * capacity) / periodMs)
      last = at
      if (tokens < 1) return false
      tokens -= 1
      return true
    },
  }
}

/** The follow-up budget all widgets of one answer share. */
export function createAnswerFollowupBudget(now: () => number = Date.now): TokenBucket {
  return createTokenBucket(FOLLOWUPS_PER_ANSWER_PER_MINUTE, 60_000, now)
}

/** What the host does with an accepted message. */
export type WidgetBridgeAction =
  | { kind: 'resize'; height: number }
  | { kind: 'stage_followup'; text: string }
  | { kind: 'confirm_link'; url: string }

export type WidgetBridgeOutcome = { ok: true; action: WidgetBridgeAction | null } | { ok: false; reason: WidgetBridgeDrop }

export type WidgetBridgeHost = {
  handle(event: WidgetBridgeEvent): WidgetBridgeOutcome
  /** The person sent or dismissed the staged chip: the widget may stage another. */
  releaseFollowup(): void
  /** Drops per reason since the frame mounted. */
  drops(): Readonly<Partial<Record<WidgetBridgeDrop, number>>>
}

/**
 * One frame's side of the bridge: the envelope check, one staged follow-up at a
 * time, the answer's shared follow-up budget and the frame's own size budget.
 */
export function createWidgetBridgeHost(options: {
  frameWindow: () => unknown
  followupBudget: TokenBucket
  now?: () => number
}): WidgetBridgeHost {
  const sizeBudget = createTokenBucket(SIZE_MESSAGES_PER_SECOND, 1000, options.now)
  const dropped: Partial<Record<WidgetBridgeDrop, number>> = {}
  let followupStaged = false

  const drop = (reason: WidgetBridgeDrop): WidgetBridgeOutcome => {
    dropped[reason] = (dropped[reason] ?? 0) + 1
    return { ok: false, reason }
  }

  return {
    handle(event) {
      const parsed = parseWidgetBridgeEvent(event, options.frameWindow())
      // A foreign source is another frame's or the page's own traffic, not this widget's to count.
      if (!parsed.ok) return parsed.reason === 'foreign_source' ? parsed : drop(parsed.reason)
      const message = parsed.message
      switch (message.type) {
        case 'ready':
          return { ok: true, action: null }
        case 'size':
          return sizeBudget.take() ? { ok: true, action: { kind: 'resize', height: message.height } } : drop('rate_limited')
        case 'followup':
          // The slot is checked first, so a widget looping on suggest() does not spend the answer's budget.
          if (followupStaged) return drop('followup_already_staged')
          if (!options.followupBudget.take()) return drop('rate_limited')
          followupStaged = true
          return { ok: true, action: { kind: 'stage_followup', text: message.text } }
        case 'open_link':
          return { ok: true, action: { kind: 'confirm_link', url: message.url } }
      }
    },
    releaseFollowup() {
      followupStaged = false
    },
    drops() {
      return { ...dropped }
    },
  }
}
