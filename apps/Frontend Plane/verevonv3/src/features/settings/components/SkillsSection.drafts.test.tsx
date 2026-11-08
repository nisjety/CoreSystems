// @vitest-environment jsdom

// EP-11 S5: a skill's draft shows a badge with Publish and Discard draft, and the history offers
// "Restore as draft", only while the server's listing says drafts are on. Off, the page is as before.

import { cleanup, fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({
    status: 'authenticated',
    user: { id: 'user-1' },
    activeOrg: { id: 'org-1', name: 'Test', role: 'owner' },
    permissions: [],
  }),
}))

vi.mock('@/shared/api/skills-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/skills-client')>()
  return {
    ...actual,
    listSkills: vi.fn(),
    listSkillVersions: vi.fn(),
    memberSkillAuthoring: vi.fn(async () => false),
    publishSkillDraft: vi.fn(async () => ({ id: 'skill-1', version: 4, published: true })),
    discardSkillDraft: vi.fn(async () => undefined),
    restoreSkillVersion: vi.fn(async () => ({ id: 'skill-1', draft: true, restored_from: 1, bundle_restored: false })),
  }
})

import {
  discardSkillDraft,
  listSkills,
  listSkillVersions,
  publishSkillDraft,
  restoreSkillVersion,
  type Skill,
  type SkillVersion,
} from '@/shared/api/skills-client'
import { SkillsSection } from './SkillsSection'

const mockedList = vi.mocked(listSkills)
const mockedVersions = vi.mocked(listSkillVersions)

function skill(overrides: Partial<Skill> = {}): Skill {
  return {
    id: 'skill-1',
    org_id: 'org-1',
    name: 'Utrulling',
    description: '',
    content: 'step one',
    trigger_keywords: [],
    trigger_file_patterns: [],
    tool_restrictions: [],
    enabled: true,
    scope: 'org',
    review_state: 'accepted',
    version: 3,
    digest: '3'.repeat(64),
    ...overrides,
  }
}

function version(n: number): SkillVersion {
  return {
    version: n,
    digest: String(n).repeat(64),
    name: 'Utrulling',
    description: '',
    content: `step ${n}`,
    trigger_keywords: [],
    trigger_file_patterns: [],
    tool_restrictions: [],
    actor_kind: 'user',
    actor_id: 'user-1',
    created_at: '2026-11-05T10:00:00Z',
  }
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('SkillsSection drafts', () => {
  it('shows_draft_badge_and_publishes_it', async () => {
    mockedList.mockResolvedValue({ skills: [skill({ draft: { content: 'step four' } })], drafts_enabled: true })
    const { findByText, getByRole } = render(() => <SkillsSection />)
    expect(await findByText(/^(Utkast|Draft)$/)).toBeTruthy()
    fireEvent.click(getByRole('button', { name: /Publiser utkastet til Utrulling|Publish the draft of Utrulling/ }))
    await waitFor(() => expect(publishSkillDraft).toHaveBeenCalledWith('org-1', 'skill-1'))
    await waitFor(() => expect(mockedList).toHaveBeenCalledTimes(2))
  })

  it('discards_a_draft_after_confirming', async () => {
    mockedList.mockResolvedValue({ skills: [skill({ draft: { content: 'step four' } })], drafts_enabled: true })
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    const { findByRole } = render(() => <SkillsSection />)
    fireEvent.click(await findByRole('button', { name: /Forkast utkastet til Utrulling|Discard the draft of Utrulling/ }))
    await waitFor(() => expect(discardSkillDraft).toHaveBeenCalledWith('org-1', 'skill-1'))
  })

  it('hides_draft_controls_while_drafts_are_off', async () => {
    // A stale draft field without the listing's switch is not shown: off means as before.
    mockedList.mockResolvedValue({ skills: [skill({ draft: { content: 'step four' } })] })
    const { findByText, queryByText, queryByRole } = render(() => <SkillsSection />)
    expect(await findByText('v3')).toBeTruthy()
    expect(queryByText(/^(Utkast|Draft)$/)).toBeNull()
    expect(queryByRole('button', { name: /Publiser utkastet|Publish the draft/ })).toBeNull()
  })

  it('history_offers_restore_as_draft_for_older_versions', async () => {
    mockedList.mockResolvedValue({ skills: [skill()], drafts_enabled: true })
    mockedVersions.mockResolvedValue({ versions: [version(3), version(2), version(1)], has_more: false })
    const { findByRole, findAllByRole } = render(() => <SkillsSection />)
    fireEvent.click(await findByRole('button', { name: /Historikk for Utrulling|History of Utrulling/ }))
    const restores = await findAllByRole('button', { name: /Gjenopprett v\d+ som utkast|Restore v\d+ as a draft/ })
    // The live version is not offered: restoring it would change nothing.
    expect(restores).toHaveLength(2)
    fireEvent.click(await findByRole('button', { name: /Gjenopprett v1 som utkast|Restore v1 as a draft/ }))
    await waitFor(() => expect(restoreSkillVersion).toHaveBeenCalledWith('org-1', 'skill-1', 1))
  })

  it('history_stays_read_only_while_drafts_are_off', async () => {
    mockedList.mockResolvedValue({ skills: [skill()] })
    mockedVersions.mockResolvedValue({ versions: [version(3), version(2)], has_more: false })
    const { findByRole } = render(() => <SkillsSection />)
    fireEvent.click(await findByRole('button', { name: /Historikk for Utrulling|History of Utrulling/ }))
    const dialog = await findByRole('dialog')
    await waitFor(() => expect(dialog.textContent).toContain('v2'))
    expect(dialog.querySelectorAll('button')).toHaveLength(1)
    expect(restoreSkillVersion).not.toHaveBeenCalled()
  })
})
