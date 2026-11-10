/**
 * The single hotkey layer (WS13-S6, D9).
 *
 * One capture-phase `keydown` listener on `window` resolves a key press to a
 * registered command. It replaces the Cmd/Ctrl+K and bare `/` handling that
 * lived in CoreNavbar. Rules:
 *   - IME composition never triggers a command.
 *   - A press something else already handled (`defaultPrevented`) is left alone.
 *   - Printable single-key bindings (`/`, `?`) fire only outside editable targets.
 *   - `Alt+1..9` matches `event.code` (`Digit1`..), so it works on any layout and
 *     on macOS where Option rewrites `event.key` (decision D-7).
 *   - A command marked `handledByPage` is fired by its page's own listener, never here.
 * `Esc` is not handled here: each overlay closes itself.
 */

import { isEditableTarget } from '@/features/chat/lib/use-chat-shortcuts'
import { listCommands, type CommandContext, type CommandDescriptor } from './command-registry'

export interface HotkeyEventLike {
  readonly key: string
  readonly code?: string
  readonly metaKey: boolean
  readonly ctrlKey: boolean
  readonly altKey: boolean
  readonly shiftKey: boolean
  readonly isComposing?: boolean
  readonly defaultPrevented?: boolean
}

interface ParsedHotkey {
  readonly mod: boolean
  readonly alt: boolean
  readonly key: string
}

function parseHotkey(spec: string): ParsedHotkey {
  const parts = spec.toLowerCase().split('+')
  // A literal "+" binding would split to ['', ''] ; none is registered, so the last part is the key.
  const key = parts[parts.length - 1] ?? ''
  return { mod: parts.includes('mod'), alt: parts.includes('alt'), key }
}

/** True when the press is exactly the binding (no extra modifiers other than Shift on printable keys). */
export function matchesHotkey(event: HotkeyEventLike, spec: string): boolean {
  const wanted = parseHotkey(spec)
  const mod = event.metaKey || event.ctrlKey
  if (wanted.mod !== mod) return false
  if (wanted.alt !== event.altKey) return false
  if (wanted.alt && /^[0-9]$/.test(wanted.key)) {
    return event.code === `Digit${wanted.key}` && !event.shiftKey
  }
  // `?` is Shift+/ on most layouts: the produced key already encodes Shift.
  const printable = wanted.key.length === 1 && !wanted.mod && !wanted.alt
  if (!printable && event.shiftKey) return false
  return event.key.toLowerCase() === wanted.key
}

function isSingleKeyBinding(spec: string): boolean {
  const parsed = parseHotkey(spec)
  return !parsed.mod && !parsed.alt
}

/** The command a press resolves to, or null. Pure, so it is testable without a DOM. */
export function resolveHotkey(
  event: HotkeyEventLike,
  target: { tagName?: string; isContentEditable?: boolean } | null,
  commands: readonly CommandDescriptor[],
): CommandDescriptor | null {
  if (event.isComposing || event.defaultPrevented) return null
  const typing = isEditableTarget(target)
  for (const command of commands) {
    // A key the page's own listener fires is listed, not fired here (CommandDescriptor.handledByPage).
    if (!command.hotkey || command.handledByPage) continue
    if (typing && isSingleKeyBinding(command.hotkey)) continue
    if (matchesHotkey(event, command.hotkey)) return command
  }
  return null
}

/**
 * Installs the layer. `getContext` is read per press so the command sees the
 * route it fires on. Returns the disposer.
 */
export function installHotkeys(getContext: () => CommandContext): () => void {
  const onKeyDown = (event: KeyboardEvent) => {
    const ctx = getContext()
    if (ctx.offline) return
    const command = resolveHotkey(event, event.target as HTMLElement | null, listCommands(ctx))
    if (!command) return
    event.preventDefault()
    void Promise.resolve(command.run(ctx)).catch(() => undefined)
  }
  window.addEventListener('keydown', onKeyDown, true)
  return () => window.removeEventListener('keydown', onKeyDown, true)
}
