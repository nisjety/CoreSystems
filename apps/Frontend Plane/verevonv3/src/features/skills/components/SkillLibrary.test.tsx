// @vitest-environment jsdom

// EP-11 S9: the skill library lists what the registry already lets this person see, by where it
// comes from. A tab never widens anything; an admin's publish is the existing promotion copy.

import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { Skill } from '@/shared/api/skills-client'

const reads = vi.hoisted(() => ({
  listSkills: vi.fn(),
  createSkill: vi.fn(),
  session: { current: {} as object },
}))

vi.mock('@/shared/session/session-store', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/session/session-store')>()),
  getSession: () => reads.session.current,
}))

vi.mock('@/shared/api/skills-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/skills-client')>()),
  listSkills: reads.listSkills,
  createSkill: reads.createSkill,
}))

import { canPublishToOrg, libraryTier, SkillLibrary, skillsInTier } from './SkillLibrary'

function skill(id: string, fields: Partial<Skill> = {}): Skill {
  return {
    id,
    org_id: 'org-1',
    name: `Skill ${id}`,
    description: '',
    content: 'body',
    trigger_keywords: [],
    trigger_file_patterns: [],
    tool_restrictions: [],
    enabled: true,
    scope: 'org',
    review_state: 'accepted',
    ...fields,
  }
}

const rows: Skill[] = [
  skill('org-1'),
  skill('mine-1', { scope: 'user', owner_user_id: 'user-1' }),
  skill('shared-1', { scope: 'user', owner_user_id: 'user-2', shared_with: ['user-1'] }),
  skill('space-1', { scope: 'space' as Skill['scope'] }),
  skill('proposal-1', { scope: 'user', owner_user_id: 'user-1', review_state: 'proposed' }),
]

function signIn(role: 'member' | 'admin') {
  reads.session.current = {
    status: 'authenticated',
    user: { id: 'user-1' },
    activeOrg: { id: 'org-1', name: 'Aquatiq', role },
    permissions: [],
  }
}

beforeEach(() => {
  signIn('member')
  reads.listSkills.mockReset()
  reads.listSkills.mockResolvedValue({ skills: rows })
  reads.createSkill.mockReset()
})

describe('libraryTier', () => {
  it('puts each row under exactly one tab for its viewer', () => {
    expect(libraryTier(rows[0]!, 'user-1')).toBe('org')
    expect(libraryTier(rows[1]!, 'user-1')).toBe('mine')
    expect(libraryTier(rows[2]!, 'user-1')).toBe('shared')
    expect(libraryTier(rows[3]!, 'user-1')).toBe('space')
    // Another viewer never sees someone else's skill as their own.
    expect(libraryTier(rows[1]!, 'user-2')).toBe('shared')
    expect(libraryTier(rows[1]!, '')).toBe('shared')
  })

  it('keeps a scope it does not know out of Mine and Organisation', () => {
    expect(libraryTier(skill('x', { scope: 'team' as Skill['scope'], owner_user_id: 'user-1' }), 'user-1')).toBe(
      'shared',
    )
  })

  it('leaves proposals to the Læring inbox and lists no row under Verevon', () => {
    expect(skillsInTier(rows, 'mine', 'user-1').map((s) => s.id)).toEqual(['mine-1'])
    expect(skillsInTier(rows, 'verevon', 'user-1')).toEqual([])
  })
})

describe('canPublishToOrg', () => {
  it('offers the promotion only to an admin, for an accepted skill a member shared', () => {
    expect(canPublishToOrg(rows[2]!, 'user-1', true)).toBe(true)
    expect(canPublishToOrg(rows[2]!, 'user-1', false)).toBe(false)
    expect(canPublishToOrg(rows[1]!, 'user-1', true)).toBe(false)
    expect(canPublishToOrg(rows[0]!, 'user-1', true)).toBe(false)
    expect(canPublishToOrg(rows[3]!, 'user-1', true)).toBe(false)
  })
})

describe('SkillLibrary', () => {
  it('shows the tab a person picks and nothing from the other tabs', async () => {
    const { container, getByText, unmount } = render(() => <SkillLibrary />)
    await waitFor(() => expect(container.querySelector('[data-skill-id="mine-1"]')).not.toBeNull())
    expect(container.querySelector('[data-skill-id="org-1"]')).toBeNull()
    expect(container.querySelector('[data-skill-id="proposal-1"]')).toBeNull()

    fireEvent.click(getByText('Organisasjon'))
    await waitFor(() => expect(container.querySelector('[data-skill-id="org-1"]')).not.toBeNull())
    expect(container.querySelector('[data-skill-id="mine-1"]')).toBeNull()

    fireEvent.click(getByText('Rom'))
    await waitFor(() => expect(container.querySelector('[data-skill-id="space-1"]')).not.toBeNull())
    unmount()
  })

  it('never offers a member the publish button', async () => {
    const { container, getByText, queryByText, unmount } = render(() => <SkillLibrary />)
    fireEvent.click(getByText('Delt med meg'))
    await waitFor(() => expect(container.querySelector('[data-skill-id="shared-1"]')).not.toBeNull())
    expect(queryByText('Publiser til organisasjonen')).toBeNull()
    unmount()
  })

  it("lets an admin publish a shared skill as the existing promotion copy", async () => {
    signIn('admin')
    reads.createSkill.mockResolvedValue({ id: 'skill_new' })
    const { container, getByText, unmount } = render(() => <SkillLibrary />)
    fireEvent.click(getByText('Delt med meg'))
    await waitFor(() => expect(container.querySelector('[data-skill-id="shared-1"]')).not.toBeNull())
    fireEvent.click(getByText('Publiser til organisasjonen'))
    await waitFor(() => expect(reads.createSkill).toHaveBeenCalledTimes(1))
    expect(reads.createSkill.mock.calls[0]).toEqual([
      'org-1',
      {
        name: 'Skill shared-1',
        description: '',
        content: 'body',
        trigger_keywords: [],
        enabled: false,
        scope: 'org',
        promoted_from: 'shared-1',
      },
    ])
    unmount()
  })

  it('says what the Verevon tab holds instead of listing rows', async () => {
    const { container, getByText, unmount } = render(() => <SkillLibrary />)
    await waitFor(() => expect(reads.listSkills).toHaveBeenCalledWith('org-1'))
    fireEvent.click(getByText('Verevon'))
    await waitFor(() => expect(getByText(/Verevons egne ferdigheter er innebygd/)).toBeTruthy())
    expect(container.querySelector('[data-skill-id]')).toBeNull()
    unmount()
  })
})
