// @vitest-environment jsdom

// EP-11 S8: "Import from file" shows only while the server's listing says import is on, and an
// import says the skill arrived as a switched-off draft. Off, the page is as before.

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
    memberSkillAuthoring: vi.fn(async () => false),
    importSkillFile: vi.fn(async () => ({
      id: 'skill_new',
      name: 'Utrulling',
      scope: 'org',
      enabled: false,
      draft: true,
    })),
  }
})

import { importSkillFile, listSkills } from '@/shared/api/skills-client'
import { SkillsSection } from './SkillsSection'

const mockedList = vi.mocked(listSkills)
const importButton = /^(Importer fra fil|Import from file)$/

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('SkillsSection import', () => {
  it('hides_import_while_the_server_has_it_off', async () => {
    mockedList.mockResolvedValue({ skills: [], drafts_enabled: true })
    const { findByText, queryByRole } = render(() => <SkillsSection />)
    expect(await findByText(/Ingen ferdigheter er opprettet/)).toBeTruthy()
    expect(queryByRole('button', { name: importButton })).toBeNull()
  })

  it('imports_a_file_as_a_switched_off_draft', async () => {
    mockedList.mockResolvedValue({ skills: [], drafts_enabled: true, import_enabled: true })
    const { findByRole, getByLabelText, findByText } = render(() => <SkillsSection />)
    expect(await findByRole('button', { name: importButton })).toBeTruthy()
    const file = new File(['---\nname: Utrulling\ndescription: Steps\n---\nDeploy.\n'], 'SKILL.md', {
      type: 'text/markdown',
    })
    const input = getByLabelText(/Velg en SKILL.md eller zip|Choose a SKILL.md or zip/) as HTMLInputElement
    fireEvent.change(input, { target: { files: [file] } })
    await waitFor(() => expect(importSkillFile).toHaveBeenCalledWith('org-1', file, 'org'))
    expect(await findByText(/importert som et utkast|imported as a draft/)).toBeTruthy()
    await waitFor(() => expect(mockedList).toHaveBeenCalledTimes(2))
  })
})
