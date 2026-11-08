// @vitest-environment jsdom

// EP-11 S2: a skill shows its version and a read-only history only when the server sends a version,
// which it does only while its version history is on. With it off the page is as before and never
// asks for a history.

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
  }
})

import { listSkills, listSkillVersions, type Skill, type SkillVersion } from '@/shared/api/skills-client'
import { ApiError } from '@/shared/api/api-error'
import { diffSkillLines, SkillsSection } from './SkillsSection'

const mockedList = vi.mocked(listSkills)
const mockedVersions = vi.mocked(listSkillVersions)

function skill(overrides: Partial<Skill> = {}): Skill {
  return {
    id: 'skill-1',
    org_id: 'org-1',
    name: 'Utrulling',
    description: '',
    content: 'step one\nstep two',
    trigger_keywords: [],
    trigger_file_patterns: [],
    tool_restrictions: [],
    enabled: true,
    scope: 'org',
    review_state: 'accepted',
    ...overrides,
  }
}

function version(n: number, content: string): SkillVersion {
  return {
    version: n,
    digest: String(n).repeat(64),
    name: 'Utrulling',
    description: '',
    content,
    trigger_keywords: [],
    trigger_file_patterns: [],
    tool_restrictions: [],
    actor_kind: 'user',
    actor_id: 'user-1',
    created_at: '2026-11-04T10:00:00Z',
  }
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('SkillsSection versions', () => {
  it('shows_version_badge_when_the_server_sends_one', async () => {
    mockedList.mockResolvedValue({ skills: [skill({ version: 3, digest: 'a'.repeat(64) })] })
    const { findByText, getByRole } = render(() => <SkillsSection />)
    expect(await findByText('v3')).toBeTruthy()
    expect(getByRole('button', { name: /Historikk for Utrulling|History of Utrulling/ })).toBeTruthy()
    expect(mockedVersions).not.toHaveBeenCalled()
  })

  it('hides_version_and_history_while_the_history_is_off', async () => {
    mockedList.mockResolvedValue({ skills: [skill()] })
    const { findByText, queryByText, queryByRole } = render(() => <SkillsSection />)
    expect(await findByText('Utrulling', { exact: false })).toBeTruthy()
    expect(queryByText(/^v\d+$/)).toBeNull()
    expect(queryByRole('button', { name: /Historikk for|History of/ })).toBeNull()
    expect(mockedVersions).not.toHaveBeenCalled()
  })

  it('history_drawer_lists_versions_with_actor_digest_and_diff', async () => {
    mockedList.mockResolvedValue({ skills: [skill({ version: 2, digest: '2'.repeat(64) })] })
    mockedVersions.mockResolvedValue({
      versions: [version(2, 'step one\nstep two'), version(1, 'step one')],
      has_more: false,
    })
    const { findByRole, findByText } = render(() => <SkillsSection />)
    fireEvent.click(await findByRole('button', { name: /Historikk for Utrulling|History of Utrulling/ }))
    await waitFor(() => expect(mockedVersions).toHaveBeenCalledWith('org-1', 'skill-1'))
    const dialog = await findByRole('dialog')
    await findByText('+ step two', { exact: false })
    expect(dialog.textContent).toContain('v1')
    expect(dialog.textContent).toContain('2'.repeat(12))
    expect(dialog.textContent).not.toContain('2'.repeat(13))
    expect(dialog.textContent).toContain('user-1')
    expect(dialog.textContent).not.toContain('- step one')
    // Read only: the drawer offers no way to change or restore a version.
    expect(dialog.querySelectorAll('button')).toHaveLength(1)
  })

  it('history_drawer_says_unavailable_on_404', async () => {
    mockedList.mockResolvedValue({ skills: [skill({ version: 1, digest: '1'.repeat(64) })] })
    mockedVersions.mockRejectedValue(new ApiError('not found', 404, 'skill_not_found'))
    const { findByRole } = render(() => <SkillsSection />)
    fireEvent.click(await findByRole('button', { name: /Historikk for Utrulling|History of Utrulling/ }))
    const alert = await findByRole('alert')
    expect(alert.textContent).toMatch(/Historikken er ikke tilgjengelig|The history is not available/)
  })
})

describe('diffSkillLines', () => {
  it('marks_added_and_removed_lines_in_reading_order', () => {
    expect(diffSkillLines('a\nb\nc', 'a\nc\nd')).toEqual([
      { kind: 'same', text: 'a' },
      { kind: 'removed', text: 'b' },
      { kind: 'same', text: 'c' },
      { kind: 'added', text: 'd' },
    ])
  })

  it('an_unchanged_text_has_no_changes', () => {
    expect(diffSkillLines('x\ny', 'x\ny').every((line) => line.kind === 'same')).toBe(true)
  })
})
