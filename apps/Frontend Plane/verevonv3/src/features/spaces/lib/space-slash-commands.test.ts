import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { setCommandPaletteFeature, setMessageSearchOpener } from '@/features/command/command-registry'

import { DM_SPACE_FEATURE } from './space-kind'
import { primeSpaceRoomFeatures } from './space-room-features'
import {
  currentSearchOpener,
  matchPlatformCommands,
  parseSlashCommandText,
  type SlashCommandContext,
} from './space-slash-commands'

let disposeOpener: (() => void) | undefined

// The composer builds its context the same way: the search opener is whatever is registered right now.
function context(overrides: Partial<SlashCommandContext> = {}): SlashCommandContext {
  return {
    spaceRef: 'room-a',
    openTab: vi.fn(),
    openSearch: currentSearchOpener(),
    openShortcutSheet: vi.fn(),
    startNewThread: vi.fn(),
    ...overrides,
  }
}

const ids = (commands: readonly { readonly id: string }[]) => commands.map((command) => command.id)

beforeEach(() => {
  setCommandPaletteFeature(true)
  primeSpaceRoomFeatures(undefined)
  disposeOpener = setMessageSearchOpener(() => undefined)
})

afterEach(() => {
  disposeOpener?.()
  disposeOpener = undefined
  setCommandPaletteFeature(undefined)
  primeSpaceRoomFeatures(undefined)
})

// WS13-S7: a platform command runs locally on Enter and never becomes a message.
describe('platform slash commands (WS13-S7)', () => {
  it('parseSlashCommandText accepts exact names and the Norwegian aliases', () => {
    const c = context()
    expect(parseSlashCommandText('/search', c)?.command.id).toBe('search')
    expect(parseSlashCommandText('/sok', c)?.command.id).toBe('search')
    expect(parseSlashCommandText('/søk', c)?.command.id).toBe('search')
    expect(parseSlashCommandText('/read', c)?.command.id).toBe('read')
    expect(parseSlashCommandText('/les', c)?.command.id).toBe('read')
    expect(parseSlashCommandText('/help', c)?.command.id).toBe('help')
    expect(parseSlashCommandText('/hjelp', c)?.command.id).toBe('help')
    expect(parseSlashCommandText('  /HJELP  ', c)?.command.id).toBe('help')
  })

  it('parseSlashCommandText rejects an unknown word, a bare slash, and an argument on a command that takes none', () => {
    const c = context()
    expect(parseSlashCommandText('/nope', c)).toBeNull()
    expect(parseSlashCommandText('/redo', c)).toBeNull()
    expect(parseSlashCommandText('/', c)).toBeNull()
    expect(parseSlashCommandText('/read please', c)).toBeNull()
    expect(parseSlashCommandText('/les nå', c)).toBeNull()
    expect(parseSlashCommandText('Hei /read', c)).toBeNull()
  })

  it('allows text after /search, which becomes the search argument', () => {
    expect(parseSlashCommandText('/search kvartals rapport', context())).toMatchObject({
      command: { id: 'search' },
      arg: 'kvartals rapport',
    })
    expect(parseSlashCommandText('/sok   kari ', context())?.arg).toBe('kari')
    expect(parseSlashCommandText('/search', context())?.arg).toBe('')
  })

  it('matchPlatformCommands matches by prefix, and an empty query lists every available command in table order', () => {
    const c = context()
    expect(ids(matchPlatformCommands('', c))).toEqual([
      'search',
      'read',
      'work',
      'agents',
      'members',
      'activity',
      'new',
      'help',
    ])
    expect(ids(matchPlatformCommands('se', c))).toEqual(['search'])
    expect(ids(matchPlatformCommands('les', c))).toEqual(['read'])
    expect(ids(matchPlatformCommands('HJ', c))).toEqual(['help'])
    // A prefix of an alias finds the command too.
    expect(ids(matchPlatformCommands('ar', c))).toEqual(['work'])
    // A substring is not a prefix.
    expect(matchPlatformCommands('ok', c)).toEqual([])
  })

  it('with the command palette flag off, no command is offered', () => {
    setCommandPaletteFeature(false)
    const c = context()
    expect(matchPlatformCommands('', c)).toEqual([])
    expect(parseSlashCommandText('/help', c)).toBeNull()
    expect(parseSlashCommandText('/search kari', c)).toBeNull()
  })

  it('/search is hidden while no search opener is registered, and offered again once one is', () => {
    disposeOpener?.()
    disposeOpener = undefined
    const without = context()
    expect(without.openSearch).toBeUndefined()
    expect(parseSlashCommandText('/search kari', without)).toBeNull()
    expect(ids(matchPlatformCommands('', without))).not.toContain('search')
    expect(parseSlashCommandText('/help', without)?.command.id).toBe('help')

    disposeOpener = setMessageSearchOpener(() => undefined)
    expect(parseSlashCommandText('/search kari', context())?.command.id).toBe('search')
  })

  it('the tab commands are hidden while their tab is not visible, and offered again in a room', () => {
    primeSpaceRoomFeatures({ [DM_SPACE_FEATURE]: true })
    const c = context()
    const listed = ids(matchPlatformCommands('', c))
    expect(listed).not.toContain('agents')
    expect(listed).not.toContain('members')
    expect(listed).toEqual(expect.arrayContaining(['work', 'activity']))
    expect(parseSlashCommandText('/agenter', c)).toBeNull()
    expect(parseSlashCommandText('/members', c)).toBeNull()
    expect(parseSlashCommandText('/work', c)?.command.id).toBe('work')

    primeSpaceRoomFeatures({ [DM_SPACE_FEATURE]: false })
    expect(parseSlashCommandText('/medlemmer', c)?.command.id).toBe('members')
  })
})
