/**
 * The platform's own commands (WS13-S6). Registered by the palette host
 * (CoreNavbar). Other slices register theirs through `registerCommands`:
 * WS13-S5 `mod+f` in-room search, WS16 `org.switch.*`.
 */

import type { CommandDescriptor } from './command-registry'
import { loadRooms, spaceHref } from './palette-providers'

const ROOM_JUMP_KEYS = [1, 2, 3, 4, 5, 6, 7, 8, 9] as const

export function createDefaultCommands(options: {
  /** True while a surface (the Inbox) owns the bare `/` key. */
  readonly deferSlash: () => boolean
}): readonly CommandDescriptor[] {
  const jumps: CommandDescriptor[] = ROOM_JUMP_KEYS.map((n) => ({
    id: `room.jump.${n}`,
    labelNo: `Gå til rom ${n}`,
    labelEn: `Go to room ${n}`,
    group: 'navigate',
    hotkey: `alt+${n}`,
    kind: 'navigate',
    run: async (ctx) => {
      const room = (await loadRooms())[n - 1]
      if (room) ctx.navigate(spaceHref(room.space_ref))
    },
  }))

  return [
    {
      id: 'palette.open',
      labelNo: 'Åpne kommandopaletten',
      labelEn: 'Open the command palette',
      group: 'help',
      hotkey: 'mod+k',
      kind: 'ui',
      run: (ctx) => ctx.openPalette(),
    },
    {
      id: 'palette.open.slash',
      labelNo: 'Søk (skråstrek)',
      labelEn: 'Search (slash)',
      group: 'help',
      hotkey: '/',
      // The support queue owns `/` while its persistent search field is on screen.
      when: () => !options.deferSlash(),
      kind: 'ui',
      run: (ctx) => ctx.openPalette(),
    },
    {
      id: 'help.shortcuts',
      labelNo: 'Vis hurtigtaster',
      labelEn: 'Show keyboard shortcuts',
      group: 'help',
      hotkey: '?',
      kind: 'ui',
      run: (ctx) => ctx.openShortcutSheet(),
    },
    {
      id: 'help.shortcuts.mod',
      labelNo: 'Vis hurtigtaster',
      labelEn: 'Show keyboard shortcuts',
      group: 'help',
      hotkey: 'mod+/',
      kind: 'ui',
      run: (ctx) => ctx.openShortcutSheet(),
    },
    ...jumps,
  ]
}
