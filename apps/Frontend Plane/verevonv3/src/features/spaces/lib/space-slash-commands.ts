/**
 * Platform slash commands in the room composer (WS13-S7, D10).
 *
 * A platform command is a thing the composer does locally: open a tab, mark the
 * room read, open message search, show the shortcut sheet. Slack's ephemeral
 * rule applies: a command runs on Enter, clears its `/token`, never becomes a
 * message, never reaches the model, and reports through a transient status
 * line rather than a timeline entry. "In-channel" commands (ones that post)
 * are not built.
 *
 * `/word` that names no command stays plain text, exactly as before, and skill
 * selection is untouched: commands only add rows above the skills.
 *
 * Nothing here is an authorization input. `/read` runs the shared
 * `spaces.mark_read` action through the same preview-then-execute path the
 * palette uses (low risk, no approval); the owning plane decides. Flag:
 * `space_command_palette_ui`, read through `commandPaletteEnabled()`.
 */

import { commandPaletteEnabled, currentMessageSearchOpener } from '@/features/command/command-registry'
import { runPaletteAction, type PaletteActionOutcome } from '@/features/command/palette-providers'
import { visibleSpaceTabs, type SpaceTabId } from './space-tab-registry'

export type PlatformCommandId =
  | 'search'
  | 'read'
  | 'work'
  | 'agents'
  | 'members'
  | 'activity'
  | 'new'
  | 'help'

/** What a command may touch. Supplied by the composer; none of it is authority. */
export interface SlashCommandContext {
  readonly spaceRef: string
  /** Opens a room tab by registry id (hash navigation). */
  readonly openTab: (tab: SpaceTabId) => void
  /** Opens in-room message search (WS13-S5); absent until that surface is mounted. */
  readonly openSearch: ((query: string) => void) | undefined
  readonly openShortcutSheet: () => void
  /** The next message starts a fresh conversation instead of replying to a thread. */
  readonly startNewThread: () => void
}

/** A bilingual line for the transient `role="status"` text. */
export interface SlashStatus {
  readonly no: string
  readonly en: string
}

export interface PlatformCommand {
  readonly id: PlatformCommandId
  /** Primary name first, then aliases (`sok`, `les`, `hjelp`). Lower case, no slash. */
  readonly names: readonly string[]
  readonly labelNo: string
  readonly labelEn: string
  readonly hintNo: string
  readonly hintEn: string
  /** True when text after the name is an argument (`/search kvartals`). */
  readonly takesText: boolean
  /** Hidden while false (a tab that is not on, a surface that is not mounted). Presentation only. */
  readonly available: (ctx: SlashCommandContext) => boolean
  readonly run: (ctx: SlashCommandContext, arg: string) => Promise<SlashStatus | undefined>
}

const tabVisible = (id: SpaceTabId) => visibleSpaceTabs().some((tab) => tab.id === id)

function tabCommand(
  id: PlatformCommandId,
  names: readonly string[],
  tab: SpaceTabId,
  labelNo: string,
  labelEn: string,
): PlatformCommand {
  return {
    id,
    names,
    labelNo,
    labelEn,
    hintNo: 'Åpner fanen',
    hintEn: 'Opens the tab',
    takesText: false,
    available: () => tabVisible(tab),
    run: async (ctx) => {
      ctx.openTab(tab)
      return undefined
    },
  }
}

const READ_OUTCOME: Record<PaletteActionOutcome, SlashStatus> = {
  done: { no: 'Rommet er merket som lest.', en: 'Room marked as read.' },
  needs_input: { no: 'Kunne ikke merke rommet som lest.', en: 'Could not mark the room as read.' },
  needs_approval: { no: 'Kunne ikke merke rommet som lest.', en: 'Could not mark the room as read.' },
  failed: { no: 'Kunne ikke merke rommet som lest.', en: 'Could not mark the room as read.' },
}

export const PLATFORM_COMMANDS: readonly PlatformCommand[] = [
  {
    id: 'search',
    names: ['search', 'sok', 'søk'],
    labelNo: 'Søk i meldinger',
    labelEn: 'Search messages',
    hintNo: 'Skriv /search og ordene du leter etter',
    hintEn: 'Type /search and the words to look for',
    takesText: true,
    // Search (WS13-S5) registers its opener when mounted; without it there is nothing to open.
    available: (ctx) => ctx.openSearch !== undefined,
    run: async (ctx, arg) => {
      ctx.openSearch?.(arg.trim())
      return undefined
    },
  },
  {
    id: 'read',
    names: ['read', 'les'],
    labelNo: 'Merk rommet som lest',
    labelEn: 'Mark this room read',
    hintNo: 'Kjører handlingen «Merk lest»',
    hintEn: 'Runs the "mark read" action',
    takesText: false,
    available: () => true,
    run: async (ctx) => READ_OUTCOME[await runPaletteAction('spaces.mark_read', { spaceRef: ctx.spaceRef })],
  },
  tabCommand('work', ['work', 'arbeid'], 'arbeid', 'Gå til Arbeid', 'Go to Work'),
  tabCommand('agents', ['agents', 'agent', 'agenter'], 'agent', 'Gå til Agent', 'Go to Agent'),
  tabCommand('members', ['members', 'medlemmer'], 'medlemmer', 'Gå til Medlemmer', 'Go to Members'),
  tabCommand('activity', ['activity', 'aktivitet'], 'aktivitet', 'Gå til Aktivitet', 'Go to Activity'),
  {
    id: 'new',
    names: ['new', 'ny'],
    labelNo: 'Ny samtale',
    labelEn: 'New conversation',
    hintNo: 'Neste melding starter en ny samtale',
    hintEn: 'The next message starts a new conversation',
    takesText: false,
    available: () => true,
    run: async (ctx) => {
      ctx.startNewThread()
      return { no: 'Neste melding starter en ny samtale.', en: 'Your next message starts a new conversation.' }
    },
  },
  {
    id: 'help',
    names: ['help', 'hjelp'],
    labelNo: 'Vis hurtigtaster',
    labelEn: 'Show keyboard shortcuts',
    hintNo: 'Åpner hurtigtastoversikten',
    hintEn: 'Opens the shortcut sheet',
    takesText: false,
    available: () => true,
    run: async (ctx) => {
      ctx.openShortcutSheet()
      return undefined
    },
  },
]

/** The slash commands are on with the command layer (`space_command_palette_ui`). */
export function platformSlashCommandsEnabled(): boolean {
  return commandPaletteEnabled()
}

export function availablePlatformCommands(ctx: SlashCommandContext): readonly PlatformCommand[] {
  if (!platformSlashCommandsEnabled()) return []
  return PLATFORM_COMMANDS.filter((command) => {
    try {
      return command.available(ctx)
    } catch {
      return false
    }
  })
}

/** Commands whose name starts with what was typed after `/`, in table order. Empty query lists all. */
export function matchPlatformCommands(query: string, ctx: SlashCommandContext): readonly PlatformCommand[] {
  const needle = query.trim().toLocaleLowerCase()
  return availablePlatformCommands(ctx).filter(
    (command) => needle === '' || command.names.some((name) => name.startsWith(needle)),
  )
}

export interface ParsedSlashCommand {
  readonly command: PlatformCommand
  readonly arg: string
}

/**
 * Reads a whole composer message as a command. Only an exact command name counts, and an
 * argument only where the command takes one: `/read please` and `/redo` stay plain text.
 */
export function parseSlashCommandText(text: string, ctx: SlashCommandContext): ParsedSlashCommand | null {
  const match = /^\/([^\s/]+)(?:\s+([\s\S]*))?$/.exec(text.trim())
  if (!match) return null
  const name = (match[1] ?? '').toLocaleLowerCase()
  const arg = (match[2] ?? '').trim()
  const command = availablePlatformCommands(ctx).find((candidate) => candidate.names.includes(name))
  if (!command) return null
  if (!command.takesText && arg !== '') return null
  return { command, arg }
}

/** Mounted search opener from the command layer, or undefined while the surface is off. */
export function currentSearchOpener(): ((query: string) => void) | undefined {
  return currentMessageSearchOpener()
}

/** Opens the shortcut sheet through the one hotkey layer (`mod+/`), so the composer holds no shell state. */
export function openShortcutSheetViaHotkeyLayer(): void {
  if (typeof window === 'undefined') return
  window.dispatchEvent(new KeyboardEvent('keydown', { key: '/', ctrlKey: true, bubbles: true, cancelable: true }))
}

/** Switches the room tab by hash; the page listens to `hashchange`. */
export function openRoomTabByHash(tab: SpaceTabId): void {
  if (typeof window === 'undefined') return
  window.location.hash = tab
}
