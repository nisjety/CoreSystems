import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

import { actionRegistry, getActionDescriptor } from '@/shared/actions/action-registry'
import { buildActionCatalogManifest } from '@/shared/actions/catalog-manifest'
import { spaceActions } from '@/shared/actions/domains/spaces'
import { MODEL_ACCESS } from '@/shared/actions/model-access'
import { isModelExecutableAction } from '@/shared/actions/model-eligibility'

// D18: the action contracts of per-member room history. The new id is human-only (an agent never sets who reads a room's
// past), its input mirrors Control's pair rule, and the invite's choice rides `spaces.add_member` (default true).

const here = dirname(fileURLToPath(import.meta.url))
const ID = 'spaces.set_member_history_window'

const descriptor = () => {
  const found = getActionDescriptor(ID)
  if (!found) throw new Error(`${ID} is not registered`)
  return found
}

const yesterday = () => new Date(Date.now() - 24 * 3600 * 1000).toISOString()
const tomorrow = () => new Date(Date.now() + 24 * 3600 * 1000).toISOString()
const valid = { spaceRef: 'space-1', memberId: 'user-2' }

describe('spaces.set_member_history_window (D18)', () => {
  it('is registered once, as a Control action, last among the Space fragments', () => {
    const ids = actionRegistry.map((action) => action.id)
    expect(ids.filter((id) => id === ID)).toHaveLength(1)
    expect(spaceActions.map((action) => action.id)).toContain(ID)
    // Appended after EP-8: the registry order test pins the first Space entries, so nothing moved.
    const spaceIds = spaceActions.map((action) => action.id as string)
    expect(spaceIds[spaceIds.length - 1]).toBe(ID)
    expect(descriptor().ownerPlane).toBe('control')
    expect(descriptor().risk).toBe('medium')
    expect(descriptor().requiresApproval).toBe(false)
    expect(descriptor().reversible).toBe(true)
  })

  it('is declared in its own fragment file under the id literal the gateway contract test reads', () => {
    const source = readFileSync(resolve(here, 'domains/spaces/d18.ts'), 'utf8')
    const ids = [...source.matchAll(/\bid:\s*'([^']+)'/g)].map((match) => match[1])
    expect(ids).toEqual([ID])
  })

  it('is closed to the model: a human_identity_or_security row that is not eligible, and human-only in the catalog', async () => {
    const row = (MODEL_ACCESS as Record<string, (typeof MODEL_ACCESS)[keyof typeof MODEL_ACCESS]>)[ID]
    expect(row).toMatchObject({ access: 'none', exemptReason: 'human_identity_or_security', eligible: false, executor: {} })
    expect(isModelExecutableAction(ID as Parameters<typeof isModelExecutableAction>[0])).toBe(false)
    const entry = (await buildActionCatalogManifest()).actions.find((action) => action.id === ID)
    expect(entry).toBeDefined()
    expect(entry?.allowedActorTypes).toEqual(['human'])
    expect(entry?.modelAccess).toBe('none')
    expect(entry?.exemptReason).toBe('human_identity_or_security')
  })

  it('is a closed, ineligible row of the generated admission contract that every plane vendors', () => {
    const contract = JSON.parse(readFileSync(resolve(here, '../api/contracts/agent-action-admission.json'), 'utf8')) as {
      rows: Array<{ actionId: string; eligible: boolean; access: string; exemptReason: string | null; zdr_allowed: boolean }>
    }
    const rows = contract.rows.filter((row) => row.actionId === ID)
    expect(rows).toHaveLength(1)
    expect(rows[0]).toMatchObject({ access: 'none', eligible: false, exemptReason: 'human_identity_or_security', zdr_allowed: false })
  })

  it('takes each of the three modes, and the instant only with since_date', () => {
    const input = descriptor().inputSchema
    expect(input.safeParse({ ...valid, mode: 'full' }).success).toBe(true)
    expect(input.safeParse({ ...valid, mode: 'since_join' }).success).toBe(true)
    expect(input.safeParse({ ...valid, mode: 'since_date', sinceAt: yesterday() }).success).toBe(true)
    // An offset is an RFC 3339 instant too; the gateway sends Control one canonical UTC string.
    expect(input.safeParse({ ...valid, mode: 'since_date', sinceAt: '2026-11-01T09:00:00+01:00' }).success).toBe(true)
    expect(input.safeParse({ ...valid, mode: 'since_date' }).success).toBe(false)
    expect(input.safeParse({ ...valid, mode: 'full', sinceAt: yesterday() }).success).toBe(false)
    expect(input.safeParse({ ...valid, mode: 'since_join', sinceAt: yesterday() }).success).toBe(false)
  })

  it('refuses a since_date instant in the future, as Control does', () => {
    const input = descriptor().inputSchema
    const refused = input.safeParse({ ...valid, mode: 'since_date', sinceAt: tomorrow() })
    expect(refused.success).toBe(false)
    expect(refused.success ? [] : refused.error.issues.map((issue) => issue.path.join('.'))).toContain('sinceAt')
  })

  it('refuses a mode an admin may not set, and anything that is not an instant', () => {
    const input = descriptor().inputSchema
    // `room` is written only by an invite, inside Control.
    for (const mode of ['room', 'all', '', 'FULL', undefined, null, 3]) {
      expect(input.safeParse({ ...valid, mode }).success, String(mode)).toBe(false)
    }
    for (const sinceAt of ['yesterday', '2026-11-01', '', '2026-13-45T00:00:00Z']) {
      expect(input.safeParse({ ...valid, mode: 'since_date', sinceAt }).success, sinceAt).toBe(false)
    }
  })

  it('needs a room and a member, and takes a revision of 0 or more', () => {
    const input = descriptor().inputSchema
    expect(input.safeParse({ memberId: 'user-2', mode: 'full' }).success).toBe(false)
    expect(input.safeParse({ spaceRef: 'space-1', mode: 'full' }).success).toBe(false)
    expect(input.safeParse({ spaceRef: ' ', memberId: 'user-2', mode: 'full' }).success).toBe(false)
    expect(input.safeParse({ spaceRef: 'space-1', memberId: '', mode: 'full' }).success).toBe(false)
    expect(input.safeParse({ ...valid, mode: 'full', expectedRevision: 0 }).success).toBe(true)
    expect(input.safeParse({ ...valid, mode: 'full', expectedRevision: 7 }).success).toBe(true)
    expect(input.safeParse({ ...valid, mode: 'full', expectedRevision: -1 }).success).toBe(false)
    expect(input.safeParse({ ...valid, mode: 'full', expectedRevision: 1.5 }).success).toBe(false)
    expect(input.safeParse({ ...valid, mode: 'full', expectedRevision: '2' }).success).toBe(false)
  })

  it('answers with the window Control confirmed: a mode, an instant and a revision, never message text', () => {
    const output = descriptor().outputSchema
    const answer = {
      space_ref: 'space-1',
      subject_id: 'user-2',
      changed: true,
      history_window: { source: 'member', mode: 'since_date', since_at: '2026-11-01T00:00:00Z', revision: 3 },
    }
    expect(output.safeParse(answer).success).toBe(true)
    expect(output.safeParse({ ...answer, history_window: { ...answer.history_window, since_at: null, mode: 'full' } }).success).toBe(true)
    expect(output.safeParse({ ...answer, history_window: { ...answer.history_window, source: 'room', mode: 'since_join' } }).success).toBe(true)
    // `room` is a source, never a mode of a window an admin set.
    expect(output.safeParse({ ...answer, history_window: { ...answer.history_window, mode: 'room' } }).success).toBe(false)
    expect(output.safeParse({ ...answer, changed: undefined }).success).toBe(false)
    expect(output.safeParse({ ...answer, history_window: undefined }).success).toBe(false)
  })

  it('takes the same pairs as the REST client sends, so the contract and the route cannot drift', () => {
    const input = descriptor().inputSchema
    const sent = [
      { mode: 'full', expectedRevision: 3 },
      { mode: 'since_join', expectedRevision: 0 },
      { mode: 'since_date', sinceAt: yesterday(), expectedRevision: 1 },
      { mode: 'since_date', sinceAt: yesterday() },
    ]
    for (const change of sent) expect(input.safeParse({ ...valid, ...change }).success, JSON.stringify(change)).toBe(true)
  })
})

describe('spaces.add_member and the invite’s share past conversation choice (D18)', () => {
  const input = () => {
    const found = getActionDescriptor('spaces.add_member')
    if (!found) throw new Error('spaces.add_member is not registered')
    return found.inputSchema
  }
  const base = { spaceRef: 'space-1', memberId: 'user-2' }

  it('defaults the choice to true, so an invite that says nothing shares the past as it always did', () => {
    const parsed = input().safeParse(base)
    expect(parsed.success).toBe(true)
    expect(parsed.success && (parsed.data as { sharePastConversation: boolean }).sharePastConversation).toBe(true)
    const withRole = input().safeParse({ ...base, role: 'viewer' })
    expect(withRole.success && (withRole.data as { sharePastConversation: boolean }).sharePastConversation).toBe(true)
  })

  it('takes false as the choice to hide what was posted before the join, and true as the choice to share', () => {
    const hidden = input().safeParse({ ...base, sharePastConversation: false })
    expect(hidden.success && (hidden.data as { sharePastConversation: boolean }).sharePastConversation).toBe(false)
    const shared = input().safeParse({ ...base, sharePastConversation: true })
    expect(shared.success && (shared.data as { sharePastConversation: boolean }).sharePastConversation).toBe(true)
  })

  it('never lets something that is not a boolean degrade into the default and share the past', () => {
    for (const value of ['false', 'true', 0, 1, null, 'no', {}, []]) {
      expect(input().safeParse({ ...base, sharePastConversation: value }).success, JSON.stringify(value)).toBe(false)
    }
  })

  it('stays a person-only action the model cannot run, and keeps its earlier fields', () => {
    expect(isModelExecutableAction('spaces.add_member')).toBe(false)
    expect(input().safeParse({ ...base, role: 'manager' }).success).toBe(true)
    expect(input().safeParse({ ...base, role: 'admin' }).success).toBe(false)
    expect(input().safeParse({ spaceRef: 'space-1' }).success).toBe(false)
  })

  it('says in its description what a false choice does', () => {
    expect(getActionDescriptor('spaces.add_member')?.description).toMatch(/sharePastConversation false/)
  })
})
