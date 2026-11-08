import { createSignal } from 'solid-js'

import { stripMarkdownPreview } from '@/shared/lib/markdown-plain-text'
import { readClientValue, writeClientValue } from '@/shared/session/client-storage'

/**
 * The shared screen-reader announcer (WS16-S4).
 *
 * One pair of visually hidden live regions per politeness, mounted once in the
 * shell (`<AnnouncerRegions/>`). Anything on the page can say something through
 * `announce()`; nothing is stored, logged or sent anywhere. The text is already
 * on screen, so it lives only in the DOM and needs no ZDR handling.
 *
 * - Two regions per politeness (A and B) are written alternately and the idle
 *   one is cleared shortly after, so the same string twice in a row still
 *   announces (a live region only speaks when its text changes).
 * - Polite messages queue (max 5, oldest dropped) and go out at most one per
 *   1.2 s. The same `key` inside `coalesceMs` collapses to one message. Three
 *   or more queued messages that share a `group` become one summary.
 * - Assertive messages bypass the queue; they are for a run that needs approval
 *   or a hard failure.
 * - The text is plain (markdown stripped), at most 140 characters, no ids.
 */

export type AnnouncePoliteness = 'polite' | 'assertive'

/** What a message is about, for the per-viewer preference. Unset is always spoken. */
export type AnnounceKind = 'post' | 'mention' | 'status'

export interface AnnounceOptions {
  readonly politeness?: AnnouncePoliteness
  /** Messages with the same key inside `coalesceMs` collapse to the first. */
  readonly key?: string
  readonly coalesceMs?: number
  readonly kind?: AnnounceKind
  /** Queued polite messages sharing a group may be said as one summary. */
  readonly group?: string
  /** The summary line for a group of `count` (three or more) queued messages. */
  readonly summarise?: (count: number) => string
}

export const ANNOUNCE_POSTS_KEY = 'verevon.a11y.announce-posts'
export type AnnouncePostsPreference = 'all' | 'mentions' | 'off'

const MAX_LENGTH = 140
const QUEUE_CAP = 5
const MIN_GAP_MS = 1200
const CLEAR_AFTER_MS = 150
const DEFAULT_COALESCE_MS = 1500
const BURST_AT = 3

export function readAnnouncePostsPreference(): AnnouncePostsPreference {
  const value = readClientValue(ANNOUNCE_POSTS_KEY)
  return value === 'mentions' || value === 'off' ? value : 'all'
}

export function writeAnnouncePostsPreference(value: AnnouncePostsPreference): void {
  writeClientValue(ANNOUNCE_POSTS_KEY, value)
}

function allowedByPreference(kind: AnnounceKind | undefined): boolean {
  if (kind !== 'post' && kind !== 'mention') return true
  const preference = readAnnouncePostsPreference()
  if (preference === 'off') return false
  if (preference === 'mentions') return kind === 'mention'
  return true
}

function plain(message: string): string {
  const text = stripMarkdownPreview(message)
  return text.length > MAX_LENGTH ? `${text.slice(0, MAX_LENGTH - 1).trimEnd()}…` : text
}

type Region = readonly [() => string, (value: string) => void]

function region(): Region {
  const [get, set] = createSignal('')
  return [get, set]
}

const regions = {
  polite: [region(), region()] as const,
  assertive: [region(), region()] as const,
}
const nextSlot = { polite: 0, assertive: 0 }
const clearTimers: Array<ReturnType<typeof setTimeout> | undefined> = [undefined, undefined, undefined, undefined]

interface Queued {
  readonly text: string
  readonly group?: string
  readonly summarise?: (count: number) => string
}

let queue: Queued[] = []
let drainTimer: ReturnType<typeof setTimeout> | undefined
let lastPoliteAt = 0
const recentKeys = new Map<string, number>()

function write(politeness: AnnouncePoliteness, text: string): void {
  const slot = nextSlot[politeness]
  nextSlot[politeness] = slot === 0 ? 1 : 0
  const other = slot === 0 ? 1 : 0
  const pair = regions[politeness]
  const setActive = pair[slot]![1]
  const setIdle = pair[other]![1]
  setActive(text)
  const timerIndex = (politeness === 'polite' ? 0 : 2) + other
  clearTimeout(clearTimers[timerIndex])
  clearTimers[timerIndex] = setTimeout(() => setIdle(''), CLEAR_AFTER_MS)
}

function takeNext(): string | undefined {
  if (queue.length === 0) return undefined
  const head = queue[0] as Queued
  if (head.group !== undefined && head.summarise) {
    const same = queue.filter((entry) => entry.group === head.group)
    if (same.length >= BURST_AT) {
      queue = queue.filter((entry) => entry.group !== head.group)
      return plain(head.summarise(same.length))
    }
  }
  queue = queue.slice(1)
  return head.text
}

function drain(): void {
  drainTimer = undefined
  const text = takeNext()
  if (text === undefined) return
  lastPoliteAt = Date.now()
  write('polite', text)
  if (queue.length > 0) drainTimer = setTimeout(drain, MIN_GAP_MS)
}

function schedulePolite(): void {
  if (drainTimer !== undefined) return
  const wait = Math.max(0, lastPoliteAt + MIN_GAP_MS - Date.now())
  drainTimer = setTimeout(drain, wait)
}

function recentlySaid(key: string, coalesceMs: number): boolean {
  const now = Date.now()
  for (const [seen, at] of recentKeys) {
    if (now - at > 60_000) recentKeys.delete(seen)
  }
  const previous = recentKeys.get(key)
  if (previous !== undefined && now - previous < coalesceMs) return true
  recentKeys.set(key, now)
  return false
}

/** Say something to screen-reader users. Safe to call from anywhere; never throws. */
export function announce(message: string, options: AnnounceOptions = {}): void {
  try {
    const text = plain(message)
    if (!text) return
    if (!allowedByPreference(options.kind)) return
    if (options.key && recentlySaid(options.key, options.coalesceMs ?? DEFAULT_COALESCE_MS)) return
    const politeness = options.politeness ?? 'polite'
    if (politeness === 'assertive') {
      write('assertive', text)
      return
    }
    queue.push({ text, group: options.group, summarise: options.summarise })
    if (queue.length > QUEUE_CAP) queue = queue.slice(queue.length - QUEUE_CAP)
    schedulePolite()
  } catch {
    // An announcement is a courtesy; it must never break the caller.
  }
}

/** Test hook: forget queued messages, timers, coalescing memory and region text. */
export function resetAnnouncerForTests(): void {
  clearTimeout(drainTimer)
  drainTimer = undefined
  for (const timer of clearTimers) clearTimeout(timer)
  clearTimers.fill(undefined)
  queue = []
  lastPoliteAt = 0
  recentKeys.clear()
  nextSlot.polite = 0
  nextSlot.assertive = 0
  for (const pair of [regions.polite, regions.assertive]) {
    for (const [, set] of pair) set('')
  }
}

/** Mount once in the shell. Visually hidden; the screen reader hears the text, nobody sees it. */
export function AnnouncerRegions() {
  const [politeA] = regions.polite[0]
  const [politeB] = regions.polite[1]
  const [alertA] = regions.assertive[0]
  const [alertB] = regions.assertive[1]
  return (
    <div class="sr-only" data-announcer="">
      <div role="status" aria-live="polite" aria-atomic="true" data-announcer-region="polite-a">{politeA()}</div>
      <div role="status" aria-live="polite" aria-atomic="true" data-announcer-region="polite-b">{politeB()}</div>
      <div role="alert" aria-live="assertive" aria-atomic="true" data-announcer-region="assertive-a">{alertA()}</div>
      <div role="alert" aria-live="assertive" aria-atomic="true" data-announcer-region="assertive-b">{alertB()}</div>
    </div>
  )
}
