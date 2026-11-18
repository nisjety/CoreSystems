import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// WS11-S3: the palette runs the real preview (it validates the input) and only the execution is stubbed.
const actions = vi.hoisted(() => ({ executeAction: vi.fn() }))
vi.mock('@/shared/actions/action-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/actions/action-client')>()),
  executeAction: actions.executeAction,
}))
vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } }),
}))

import type { CommandContext } from './command-registry'
import { actionItems, runPaletteAction } from './palette-providers'

const tr = (_no: string, english: string) => english

function context(over: Partial<CommandContext> = {}): CommandContext {
  return {
    spaceRef: null,
    chatThreadId: null,
    offline: false,
    navigate: vi.fn(),
    openPalette: vi.fn(),
    openShortcutSheet: vi.fn(),
    ...over,
  }
}

beforeEach(() => {
  actions.executeAction.mockReset()
  actions.executeAction.mockResolvedValue({})
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('actionItems', () => {
  it('offers Mark all rooms read with empty input outside a room', () => {
    const items = actionItems(context({ spaceRef: null }), '', tr, () => undefined)
    const markAll = items.find((item) => item.id === 'action:spaces.mark_all_read')
    expect(markAll?.label).toBe('Mark all rooms read')
    expect(markAll?.group).toBe('actions')
  })
})

describe('runPaletteAction', () => {
  it('runs spaces.mark_all_read as the signed-in human, with no input', async () => {
    await expect(runPaletteAction('spaces.mark_all_read', {})).resolves.toBe('done')
    expect(actions.executeAction).toHaveBeenCalledTimes(1)
    expect(actions.executeAction).toHaveBeenCalledWith(
      'spaces.mark_all_read',
      { type: 'human', userId: 'user-a', orgId: 'org-a' },
      {},
    )
  })
})
