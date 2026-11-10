/**
 * The command layer's registry (WS13-S6, D9).
 *
 * A command is a thing a person can do from the keyboard: jump to a room, open
 * the palette, mark a Space read. This registry WRAPS the shared action
 * registry (`@/shared/actions`), it never edits it: `ActionDescriptor` has no
 * hotkey or `when` field, and the strict dispatcher-parity test must not move.
 * A command that runs an action names it in `actionId`; everything else is
 * navigation or UI state.
 *
 * Nothing here is an authorization input. A command only calls what the
 * owning plane already allows; `when` hides a command that cannot apply, it
 * does not grant anything.
 */

import { createSignal } from 'solid-js'
import type { ActionId } from '@/shared/actions/action-registry'

export type CommandGroup = 'navigate' | 'room' | 'action' | 'help'
export type CommandKind = 'navigate' | 'ui' | 'action'

/** What a command may read and call. Built by the palette host (CoreNavbar). */
export interface CommandContext {
  /** The room in scope (`/spaces/:ref`), or null outside a room. */
  readonly spaceRef: string | null
  /** The active server thread on `/chat` (`readActiveChatThreadId`), or null elsewhere. Presentation input, never authority. */
  readonly chatThreadId: string | null
  /** True on isolated local surfaces where global chrome stays inert. */
  readonly offline: boolean
  readonly navigate: (href: string) => void
  readonly openPalette: (query?: string) => void
  readonly openShortcutSheet: () => void
}

export interface CommandDescriptor {
  readonly id: string
  readonly labelNo: string
  readonly labelEn: string
  readonly group: CommandGroup
  /** `mod+k`, `/`, `?`, `alt+1`. `mod` is Cmd on macOS and Ctrl elsewhere. */
  readonly hotkey?: string
  /** Hidden and inert while this returns false. Presentation only. */
  readonly when?: (ctx: CommandContext) => boolean
  readonly kind: CommandKind
  readonly run: (ctx: CommandContext) => void | Promise<void>
  /** The shared action this command executes, when `kind === 'action'`. */
  readonly actionId?: ActionId
  /**
   * A page's own keydown listener fires this command's key (the chat page's use-chat-shortcuts).
   * The hotkey layer still lists the binding on the shortcut sheet but never fires it, so one press
   * runs the command once. The palette and the sheet run or show it as usual.
   */
  readonly handledByPage?: boolean
}

const registered = new Map<string, CommandDescriptor>()
const [version, setVersion] = createSignal(0)

function isDev(): boolean {
  try {
    return import.meta.env.DEV === true
  } catch {
    return false
  }
}

/** Two commands on one hotkey would make the second silently dead. */
function assertNoDuplicateHotkey(candidate: CommandDescriptor): void {
  if (!isDev() || !candidate.hotkey) return
  for (const other of registered.values()) {
    if (other.id !== candidate.id && other.hotkey === candidate.hotkey) {
      throw new Error(`Duplicate command hotkey "${candidate.hotkey}": ${other.id} and ${candidate.id}`)
    }
  }
}

/** Registers commands; returns the disposer that removes exactly these. Re-registering an id replaces it. */
export function registerCommands(list: readonly CommandDescriptor[]): () => void {
  for (const command of list) {
    assertNoDuplicateHotkey(command)
    registered.set(command.id, command)
  }
  setVersion((value) => value + 1)
  return () => {
    for (const command of list) {
      if (registered.get(command.id) === command) registered.delete(command.id)
    }
    setVersion((value) => value + 1)
  }
}

/** Commands whose `when` holds in `ctx`, in registration order. Reactive on registration changes. */
export function listCommands(ctx: CommandContext): readonly CommandDescriptor[] {
  version()
  return [...registered.values()].filter((command) => {
    try {
      return command.when ? command.when(ctx) : true
    } catch {
      return false
    }
  })
}

/** Every registered command regardless of `when` (the shortcut sheet lists bindings, not availability). */
export function allCommands(): readonly CommandDescriptor[] {
  version()
  return [...registered.values()]
}

/**
 * The in-room message search (WS13-S5) hands the palette a way to open it, so
 * the palette can show its pinned "Search messages" row. Absent while that
 * surface is not mounted or its flag is off, and the row is then not shown.
 */
let messageSearchOpener: ((query: string) => void) | undefined
const [openerVersion, setOpenerVersion] = createSignal(0)

export function setMessageSearchOpener(opener: (query: string) => void): () => void {
  messageSearchOpener = opener
  setOpenerVersion((value) => value + 1)
  return () => {
    if (messageSearchOpener === opener) messageSearchOpener = undefined
    setOpenerVersion((value) => value + 1)
  }
}

export function currentMessageSearchOpener(): ((query: string) => void) | undefined {
  openerVersion()
  return messageSearchOpener
}

export function __resetCommandsForTests(): void {
  registered.clear()
  messageSearchOpener = undefined
  setVersion((value) => value + 1)
}

/**
 * Rollout switch for the palette (`space_command_palette_ui`). Presentation
 * only, never an authorization input. Until the gateway's
 * `GET /api/v1/spaces/features` carries `command_palette` (WS13-S3), the build
 * flag `VITE_SPACE_COMMAND_PALETTE_UI` decides: '1' on, '0' off, unset means on
 * in dev and off in production. `setCommandPaletteFeature` lets the features
 * route override it later without another change here.
 */
let featureOverride: boolean | undefined

export function setCommandPaletteFeature(enabled: boolean | undefined): void {
  featureOverride = enabled
}

export function commandPaletteEnabled(): boolean {
  if (featureOverride !== undefined) return featureOverride
  const raw = import.meta.env.VITE_SPACE_COMMAND_PALETTE_UI
  if (raw === '1') return true
  if (raw === '0') return false
  return isDev()
}
