import { createSignal, type Accessor } from 'solid-js'

/**
 * Shell toasts (F-t3-078): one shared list of short notices that outlives the panel that raised them. A toast
 * raised from a thread or a room stays up when the person moves on, and the shell's ToastHost draws it.
 *
 * Presentation only. A toast holds its tone, the message string it was given, an optional action and an optional
 * scope (for example a thread id, so `dismissScope` can clear one thread's toasts). Nothing is persisted, logged or
 * sent. A caller that needs more than a line of text links to where that content already lives.
 *
 * Behind `VITE_SHELL_TOASTS=true`, read on each call. Off, `showToast` returns '' and does nothing. Call it from an
 * event handler or from an effect's apply step, never from a memo or a component body: Solid 2 forbids signal
 * writes inside an owned computation.
 */

export type ToastTone = 'info' | 'success' | 'warning' | 'error'

export interface ToastAction {
  readonly label: string
  /** Runs when the action is clicked. The toast is dismissed first. */
  readonly run: () => void
}

export interface ShowToastOptions {
  /** Reusing an id replaces the visible toast in place (dedupe) and restarts its timer. */
  readonly id?: string
  readonly tone: ToastTone
  /** The text the toast shows, and the screen reader says. */
  readonly message: string
  readonly action?: ToastAction
  /** Groups toasts for `dismissScope`, for example a thread id. */
  readonly scope?: string
  /** Milliseconds before auto-dismiss. Defaults to 5000. Ignored when `sticky` is set. */
  readonly ttlMs?: number
  /** Stays until the person dismisses it, or a caller dismisses it by id or scope. */
  readonly sticky?: boolean
}

export interface ShellToast {
  readonly id: string
  readonly tone: ToastTone
  readonly message: string
  readonly action?: ToastAction | undefined
  readonly scope?: string | undefined
  readonly sticky: boolean
}

/** Why a countdown is held. Hovering and focusing hold it independently. */
export type ToastHold = 'hover' | 'focus'

const MAX_VISIBLE = 4
const DEFAULT_TTL_MS = 5000
/** The longest delay setTimeout honours; a longer one would fire at once. */
const MAX_TIMER_MS = 2_147_483_647

/** Read on each call, so the flag can be stubbed per test. */
export function shellToastsEnabled(): boolean {
  return import.meta.env.VITE_SHELL_TOASTS === 'true'
}

const [toastList, setToastList] = createSignal<readonly ShellToast[]>([])

/** The visible toasts, oldest first. Read it inside JSX or a memo. */
export const shellToasts: Accessor<readonly ShellToast[]> = toastList

// Solid 2 reads trail writes until the next flush, so the list is kept here and mirrored into the signal.
let current: readonly ShellToast[] = []
let sequence = 0

interface Countdown {
  remainingMs: number
  startedAt: number
  handle: ReturnType<typeof setTimeout> | undefined
  readonly holds: Set<ToastHold>
}

/** Countdowns exist only for non-sticky toasts. */
const countdowns = new Map<string, Countdown>()

function publish(next: readonly ShellToast[]): void {
  current = next
  setToastList(next)
}

function ttlFor(ttlMs: number | undefined): number {
  if (typeof ttlMs !== 'number' || !Number.isFinite(ttlMs) || ttlMs <= 0) return DEFAULT_TTL_MS
  return Math.min(ttlMs, MAX_TIMER_MS)
}

function startCountdown(id: string, countdown: Countdown): void {
  if (countdown.handle !== undefined || countdown.holds.size > 0) return
  countdown.startedAt = Date.now()
  countdown.handle = setTimeout(() => dismissToast(id), countdown.remainingMs)
}

function haltCountdown(countdown: Countdown): void {
  if (countdown.handle === undefined) return
  clearTimeout(countdown.handle)
  countdown.handle = undefined
  countdown.remainingMs = Math.max(0, countdown.remainingMs - (Date.now() - countdown.startedAt))
}

function forget(id: string): void {
  const countdown = countdowns.get(id)
  if (countdown === undefined) return
  if (countdown.handle !== undefined) clearTimeout(countdown.handle)
  countdowns.delete(id)
}

/** Drops the oldest toasts, other than the one just raised, until the cap holds. Non-sticky toasts go first. */
function enforceCap(list: readonly ShellToast[], keepId: string): readonly ShellToast[] {
  let next = list
  while (next.length > MAX_VISIBLE) {
    const victim =
      next.find((toast) => !toast.sticky && toast.id !== keepId) ?? next.find((toast) => toast.id !== keepId)
    if (victim === undefined) break
    forget(victim.id)
    next = next.filter((toast) => toast !== victim)
  }
  return next
}

/**
 * Raises a toast and returns its id. Returns '' when the flag is off or the message is blank. An id that is already
 * visible replaces that toast in place.
 */
export function showToast(options: ShowToastOptions): string {
  if (!shellToastsEnabled() || options.message.trim() === '') return ''
  const id = options.id ?? `shell-toast-${++sequence}`
  const sticky = options.sticky === true
  const toast: ShellToast = {
    id,
    tone: options.tone,
    message: options.message,
    action: options.action,
    scope: options.scope,
    sticky,
  }
  const index = current.findIndex((entry) => entry.id === id)
  const next =
    index === -1 ? [...current, toast] : current.map((entry, position) => (position === index ? toast : entry))
  if (sticky) {
    forget(id)
  } else {
    const ttl = ttlFor(options.ttlMs)
    const existing = countdowns.get(id)
    const countdown: Countdown =
      existing ?? { remainingMs: ttl, startedAt: 0, handle: undefined, holds: new Set<ToastHold>() }
    if (existing !== undefined) {
      haltCountdown(existing)
      existing.remainingMs = ttl
    }
    countdowns.set(id, countdown)
    startCountdown(id, countdown)
  }
  publish(enforceCap(next, id))
  return id
}

/** Removes a toast and stops its countdown. An unknown id is ignored. */
export function dismissToast(id: string): void {
  forget(id)
  if (!current.some((toast) => toast.id === id)) return
  publish(current.filter((toast) => toast.id !== id))
}

/** Removes every toast raised with this scope. */
export function dismissScope(scope: string): void {
  const doomed = current.filter((toast) => toast.scope === scope)
  if (doomed.length === 0) return
  for (const toast of doomed) forget(toast.id)
  publish(current.filter((toast) => toast.scope !== scope))
}

/** Holds a toast's countdown for one reason, such as the pointer being over it. */
export function pauseToast(id: string, hold: ToastHold): void {
  const countdown = countdowns.get(id)
  if (countdown === undefined) return
  countdown.holds.add(hold)
  haltCountdown(countdown)
}

/** Releases one hold. The countdown restarts once no hold remains. */
export function resumeToast(id: string, hold: ToastHold): void {
  const countdown = countdowns.get(id)
  if (countdown === undefined) return
  countdown.holds.delete(hold)
  startCountdown(id, countdown)
}
