import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { SpaceMergedToolPolicy, SpaceToolPolicyEntry } from '@/shared/api/spaces-client'

import { clearSpaceRoomFeatures, primeSpaceRoomFeatures } from './space-room-features'
import {
  TOOL_POLICY_MAX_ENTRIES,
  draftMatchesStored,
  draftProblem,
  mergedLevelFor,
  mergedRows,
  rowOverriddenByCeiling,
  toolPolicyEditorEnabled,
  type ToolPolicyDraftRow,
} from './space-tool-policy'

// WS10-S5b: the pure half of the per-tool policy editor. Control writes and decides; these functions only
// say what the editor shows and whether a draft is worth sending. Fixtures use neutral tool names.

let nextRow = 1
const row = (toolKey: string, level: ToolPolicyDraftRow['level'] = 'ask'): ToolPolicyDraftRow => ({
  id: nextRow++,
  toolKey,
  level,
})

const merged = (tools: SpaceMergedToolPolicy['tools']): SpaceMergedToolPolicy => ({
  tools,
  ceilingRevision: 1,
  agentRevision: 1,
  digest: 'digest-a',
})

describe('draftProblem: what would make Control refuse the write (WS10-S5b)', () => {
  it('accepts distinct, well-formed keys, the wildcard included', () => {
    expect(draftProblem([row('*'), row('read_file'), row('mail.send')])).toBeNull()
  })

  it('names an empty key, whether blank or only spaces', () => {
    expect(draftProblem([row('read'), row('')])).toBe('empty_key')
    expect(draftProblem([row('read'), row('   ')])).toBe('empty_key')
  })

  it('names an invalid key: a space or a control character inside it, or more than 200 characters', () => {
    expect(draftProblem([row('read file')])).toBe('invalid_key')
    expect(draftProblem([row('read\tfile')])).toBe('invalid_key')
    expect(draftProblem([row('read\u0007file')])).toBe('invalid_key')
    expect(draftProblem([row('a'.repeat(200))])).toBeNull()
    expect(draftProblem([row('a'.repeat(201))])).toBe('invalid_key')
  })

  it('names a key the list already holds, once the spaces around it are trimmed', () => {
    expect(draftProblem([row('read'), row(' read ')])).toBe('duplicate_key')
  })

  it('names too many rules past the cap, and allows exactly the cap', () => {
    const atCap = Array.from({ length: TOOL_POLICY_MAX_ENTRIES }, (_, index) => row(`tool-${index}`))
    expect(draftProblem(atCap)).toBeNull()
    expect(draftProblem([...atCap, row('one-more')])).toBe('too_many')
  })
})

describe('draftMatchesStored (WS10-S5b)', () => {
  const stored: SpaceToolPolicyEntry[] = [
    { toolKey: 'read', level: 'allow' },
    { toolKey: '*', level: 'block' },
  ]

  it('matches a draft that holds the same rules in another order', () => {
    expect(draftMatchesStored([row('*', 'block'), row('read', 'allow')], stored)).toBe(true)
    expect(draftMatchesStored([row('read', 'allow'), row('*', 'block')], stored)).toBe(true)
  })

  it('does not match when a level changed, or a rule was added or removed', () => {
    expect(draftMatchesStored([row('*', 'ask'), row('read', 'allow')], stored)).toBe(false)
    expect(draftMatchesStored([row('*', 'block'), row('read', 'allow'), row('write', 'ask')], stored)).toBe(false)
    expect(draftMatchesStored([row('*', 'block')], stored)).toBe(false)
  })
})

describe('mergedLevelFor: the level a tool gets in the merged view (WS10-S5b)', () => {
  const view = merged({
    '*': { level: 'ask', source: 'space_ceiling' },
    read: { level: 'allow', source: 'agent' },
  })

  it('uses the tool’s own entry first', () => {
    expect(mergedLevelFor(view, 'read')).toEqual({ level: 'allow', source: 'agent' })
  })

  it('falls back to the wildcard entry for a tool the view does not name', () => {
    expect(mergedLevelFor(view, 'write')).toEqual({ level: 'ask', source: 'space_ceiling' })
  })

  it('falls back to allow with no rule when there is no wildcard either', () => {
    expect(mergedLevelFor(merged({ read: { level: 'block', source: 'agent' } }), 'write')).toEqual({
      level: 'allow',
      source: 'default',
    })
  })
})

describe('rowOverriddenByCeiling: a row the room ceiling holds down (WS10-S5b)', () => {
  const ceilingBlocks = merged({ '*': { level: 'block', source: 'space_ceiling' } })

  it('is true when the ceiling is stricter than the row and decided the level', () => {
    expect(rowOverriddenByCeiling(ceilingBlocks, row('write', 'allow'))).toBe(true)
    expect(rowOverriddenByCeiling(ceilingBlocks, row('write', 'ask'))).toBe(true)
  })

  it('is false when the row already says what the ceiling says, or the agent layer decided it', () => {
    expect(rowOverriddenByCeiling(ceilingBlocks, row('write', 'block'))).toBe(false)
    const agentDecided = merged({ read: { level: 'block', source: 'agent' } })
    expect(rowOverriddenByCeiling(agentDecided, row('read', 'allow'))).toBe(false)
    const looseCeiling = merged({ '*': { level: 'allow', source: 'space_ceiling' } })
    expect(rowOverriddenByCeiling(looseCeiling, row('write', 'block'))).toBe(false)
  })

  it('matches the row by its key with the spaces trimmed', () => {
    expect(rowOverriddenByCeiling(ceilingBlocks, row(' write ', 'allow'))).toBe(true)
  })
})

describe('mergedRows: the table the editor shows (WS10-S5b)', () => {
  it('lists the wildcard first, then the other tools alphabetically', () => {
    const view = merged({
      write: { level: 'ask', source: 'agent' },
      '*': { level: 'block', source: 'space_ceiling' },
      read: { level: 'allow', source: 'agent' },
      delete: { level: 'ask', source: 'space_ceiling' },
    })
    expect(mergedRows(view).map((entry) => entry.toolKey)).toEqual(['*', 'delete', 'read', 'write'])
    expect(mergedRows(view)[0]).toEqual({ toolKey: '*', level: 'block', source: 'space_ceiling' })
  })
})

describe('toolPolicyEditorEnabled: the Agent tab offers the editor when the room says so (WS10-S5b)', () => {
  beforeEach(() => {
    clearSpaceRoomFeatures()
    vi.stubEnv('DEV', true)
  })

  afterEach(() => {
    clearSpaceRoomFeatures()
    vi.unstubAllEnvs()
  })

  it('follows the room’s feature flag, and is off before the room has been read', () => {
    expect(toolPolicyEditorEnabled()).toBe(false)
    primeSpaceRoomFeatures({ tool_policy_editor: true })
    expect(toolPolicyEditorEnabled()).toBe(true)
    primeSpaceRoomFeatures({ tool_policy_editor: false })
    expect(toolPolicyEditorEnabled()).toBe(false)
    primeSpaceRoomFeatures({})
    expect(toolPolicyEditorEnabled()).toBe(false)
  })

  it('lets a development override win over the room, both ways', () => {
    vi.stubEnv('VITE_SPACE_TOOL_POLICY_EDITOR', 'false')
    primeSpaceRoomFeatures({ tool_policy_editor: true })
    expect(toolPolicyEditorEnabled()).toBe(false)

    vi.stubEnv('VITE_SPACE_TOOL_POLICY_EDITOR', 'true')
    primeSpaceRoomFeatures({ tool_policy_editor: false })
    expect(toolPolicyEditorEnabled()).toBe(true)
  })

  it('never reads the override in a production build: the room alone decides there', () => {
    vi.stubEnv('DEV', false)
    vi.stubEnv('VITE_SPACE_TOOL_POLICY_EDITOR', 'true')
    primeSpaceRoomFeatures({ tool_policy_editor: false })
    expect(toolPolicyEditorEnabled()).toBe(false)

    vi.stubEnv('VITE_SPACE_TOOL_POLICY_EDITOR', 'false')
    primeSpaceRoomFeatures({ tool_policy_editor: true })
    expect(toolPolicyEditorEnabled()).toBe(true)
  })
})
