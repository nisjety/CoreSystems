import { beforeEach, describe, expect, it, vi } from 'vitest'
import { anbudState, anbudWorkspace, resetAnbudState } from '@/features/anbud/lib/anbud-state'
import { draftFromProfile, emptyProfileDraft, parseProfile, sourceLabel } from '@/features/anbud/lib/profile'
import { loadWorkspace, markNewTendersSeen, saveProfile, setPipelineStage } from '@/features/anbud/lib/workspace-sync'

const client = vi.hoisted(() => ({
  readWorkspaceProfile: vi.fn(),
  readWorkspacePipeline: vi.fn(),
  writeWorkspaceProfile: vi.fn(),
  writeWorkspaceStage: vi.fn(),
  suggestWorkspaceProfile: vi.fn(),
  readWorkspaceSeen: vi.fn(),
  markWorkspaceSeen: vi.fn(),
}))

vi.mock('@/shared/api/anbud-client', () => client)

const saved = {
  orgNumber: '923456789',
  cpv: ['72212000'],
  nuts: ['NO081'],
  keywords: ['åpen drift', 'IT'],
  valueRange: { currency: 'NOK', min: 100000, max: 900000 },
}

beforeEach(() => {
  resetAnbudState()
  vi.clearAllMocks()
  client.readWorkspaceProfile.mockResolvedValue(null)
  client.readWorkspacePipeline.mockResolvedValue([])
  client.writeWorkspaceProfile.mockResolvedValue(undefined)
  client.writeWorkspaceStage.mockResolvedValue(undefined)
  client.suggestWorkspaceProfile.mockResolvedValue({ applied: false, reason: 'no_wins', suggestion: { orgNumber: '923456783', wins: 0, cpv: [], nuts: [] } })
  client.readWorkspaceSeen.mockResolvedValue('2026-10-01T00:00:00Z')
  client.markWorkspaceSeen.mockResolvedValue('2026-10-28T10:00:00Z')
})

describe('loadWorkspace', () => {
  it('puts a saved profile, its search step and the pipeline stages into the page', async () => {
    client.readWorkspaceProfile.mockResolvedValue({ profile: { ...saved, strictness: 'wide' }, updatedAt: '2026-11-09T08:00:00Z' })
    client.readWorkspacePipeline.mockResolvedValue([{ noticeId: '2026-100001', stage: 'sent', updatedAt: '2026-11-09T08:00:00Z' }])
    await loadWorkspace(new AbortController().signal)
    expect(anbudState.submitted()).toEqual(saved)
    expect(anbudState.strictness()).toBe('wide')
    expect(anbudState.draft().cpv).toBe('72212000')
    expect(anbudWorkspace.stages()).toEqual({ '2026-100001': 'sent' })
  })

  it('clears what the page held when nothing is saved for this organisation', async () => {
    anbudState.setSubmitted(saved)
    anbudState.setStrictness('strict')
    anbudWorkspace.setStages({ '2026-100001': 'working' })
    await loadWorkspace(new AbortController().signal)
    expect(anbudState.submitted()).toBeNull()
    expect(anbudState.strictness()).toBe('balanced')
    expect(anbudState.draft()).toEqual(emptyProfileDraft)
    expect(anbudWorkspace.stages()).toEqual({})
  })
})

describe('saveProfile', () => {
  it('writes the searched profile with the step it was searched at', async () => {
    anbudState.setSubmitted(saved)
    anbudState.setStrictness('strict')
    saveProfile()
    await vi.waitFor(() => expect(client.writeWorkspaceProfile).toHaveBeenCalledTimes(1))
    // Nothing was loaded, so every tracked field is one the user entered.
    expect(client.writeWorkspaceProfile).toHaveBeenCalledWith({
      ...saved,
      strictness: 'strict',
      sources: { orgNumber: 'user', cpv: 'user', nuts: 'user', keywords: 'user', valueRange: 'user' },
      confirmed: true,
    })
  })

  it('writes nothing before a profile has been searched', () => {
    saveProfile()
    expect(client.writeWorkspaceProfile).not.toHaveBeenCalled()
  })
})

describe('setPipelineStage', () => {
  it('puts back a move the server refuses, and says so', async () => {
    anbudWorkspace.setStages({ '2026-100001': 'review' })
    client.writeWorkspaceStage.mockRejectedValue(new Error('refused'))
    setPipelineStage('2026-100001', 'working')
    expect(anbudWorkspace.stages()['2026-100001']).toBe('working')
    await vi.waitFor(() => expect(anbudWorkspace.persistFailed()).toBe(true))
    expect(anbudWorkspace.stages()['2026-100001']).toBe('review')
  })

  it('runs one write at a time, in the order the moves were made', async () => {
    let inFlight = 0
    let peak = 0
    const order: string[] = []
    client.writeWorkspaceStage.mockImplementation(async (_noticeId: string, stage: string) => {
      inFlight += 1
      peak = Math.max(peak, inFlight)
      order.push(stage)
      await new Promise((resolve) => setTimeout(resolve, 5))
      inFlight -= 1
    })
    setPipelineStage('2026-100001', 'review')
    setPipelineStage('2026-100001', 'working')
    setPipelineStage('2026-100002', 'sent')
    await vi.waitFor(() => expect(order).toEqual(['review', 'working', 'sent']))
    expect(peak).toBe(1)
  })
})

describe('a saved profile read back into the form', () => {
  it('parses to the same profile it was saved as', () => {
    expect(parseProfile(draftFromProfile(saved))).toEqual({ profile: saved })
  })
})

describe('a suggested profile', () => {
  it('goes into the editor as a draft when nothing is saved, and is not searched until the organisation saves it', async () => {
    client.suggestWorkspaceProfile.mockResolvedValue({
      applied: true,
      profile: { profile: { ...saved, strictness: 'balanced', confirmed: false, suggestedFrom: { wins: 3 } }, updatedAt: '2026-11-09T08:00:00Z' },
      suggestion: { orgNumber: '923456783', wins: 3, cpv: ['72212000'], nuts: ['NO081'] },
    })
    const applied = await loadWorkspace(new AbortController().signal)
    expect(applied).toBe(false)
    expect(anbudState.submitted()).toBeNull()
    expect(anbudState.draft().cpv).toBe('72212000')
    expect(anbudWorkspace.profileOpen()).toBe(true)
    expect(anbudWorkspace.profileSuggestion()).toEqual({ wins: 3, cpv: 1, nuts: 1 })
  })

  it('keeps an unconfirmed profile as a draft after a reload, and searches with a confirmed one', async () => {
    client.readWorkspaceProfile.mockResolvedValue({ profile: { ...saved, strictness: 'balanced', confirmed: false, suggestedFrom: { wins: 2 } }, updatedAt: '2026-11-09T08:00:00Z' })
    expect(await loadWorkspace(new AbortController().signal)).toBe(false)
    expect(anbudState.submitted()).toBeNull()
    expect(anbudWorkspace.profileOpen()).toBe(true)

    client.readWorkspaceProfile.mockResolvedValue({ profile: { ...saved, strictness: 'balanced', confirmed: true }, updatedAt: '2026-11-09T08:00:00Z' })
    expect(await loadWorkspace(new AbortController().signal)).toBe(true)
    expect(anbudState.submitted()).toEqual(saved)
  })

  it('leaves the page empty when the organisation has no wins to draw from', async () => {
    const applied = await loadWorkspace(new AbortController().signal)
    expect(applied).toBe(false)
    expect(anbudState.submitted()).toBeNull()
    expect(anbudWorkspace.profileSuggestion()).toBeNull()
  })

  it('keeps loading the pipeline when the suggestion fails', async () => {
    client.suggestWorkspaceProfile.mockRejectedValue(new Error('unavailable'))
    client.readWorkspacePipeline.mockResolvedValue([{ noticeId: '2026-100001', stage: 'review', updatedAt: '2026-11-09T08:00:00Z' }])
    const applied = await loadWorkspace(new AbortController().signal)
    expect(applied).toBe(false)
    expect(anbudWorkspace.stages()).toEqual({ '2026-100001': 'review' })
  })

  it('drops the notice once the organisation saves its own profile', () => {
    anbudWorkspace.setProfileSuggestion({ wins: 3, cpv: 1, nuts: 1 })
    anbudState.setSubmitted(saved)
    saveProfile()
    expect(anbudWorkspace.profileSuggestion()).toBeNull()
  })
})

describe('where a field of the profile came from', () => {
  it('marks a suggested field the user changes as theirs, and keeps the others where they came from', async () => {
    client.suggestWorkspaceProfile.mockResolvedValue({
      applied: true,
      profile: {
        profile: { ...saved, strictness: 'balanced', sources: { orgNumber: 'organisation', cpv: 'wins', nuts: 'wins' } },
        updatedAt: '2026-11-09T08:00:00Z',
      },
      suggestion: { orgNumber: '923456783', wins: 3, cpv: ['72212000'], nuts: ['NO081'] },
    })
    await loadWorkspace(new AbortController().signal)
    anbudState.setSubmitted({ ...saved, cpv: ['45000000'] })
    saveProfile()
    expect(anbudWorkspace.profileSources()).toEqual({ orgNumber: 'organisation', cpv: 'user', nuts: 'wins' })
    await vi.waitFor(() => expect(client.writeWorkspaceProfile).toHaveBeenCalledTimes(1))
    expect(client.writeWorkspaceProfile.mock.calls[0]?.[0]?.sources).toEqual({ orgNumber: 'organisation', cpv: 'user', nuts: 'wins' })
  })

  it('words each source for the form, and says nothing for a field with no source', () => {
    expect(sourceLabel('wins')).toEqual({ no: 'Foreslått fra vunne anbud', en: 'Suggested from tenders won' })
    expect(sourceLabel('organisation')?.en).toBe('From the organisation')
    expect(sourceLabel('user')?.no).toBe('Lagt inn av deg')
    expect(sourceLabel(undefined)).toBeUndefined()
  })
})

describe('the mark of when the tenders were last seen', () => {
  it('is set on the first visit, so nothing is new on it', async () => {
    client.readWorkspaceSeen.mockResolvedValue(null)
    await loadWorkspace(new AbortController().signal)
    expect(client.markWorkspaceSeen).toHaveBeenCalledTimes(1)
    expect(anbudWorkspace.lastSeen()).toBe('2026-10-28T10:00:00Z')
  })

  it('is kept on a later visit, which counts what came after it, and is not moved until marked', async () => {
    client.readWorkspaceSeen.mockResolvedValue('2026-10-27T09:00:00Z')
    await loadWorkspace(new AbortController().signal)
    expect(client.markWorkspaceSeen).not.toHaveBeenCalled()
    expect(anbudWorkspace.lastSeen()).toBe('2026-10-27T09:00:00Z')
    markNewTendersSeen()
    await vi.waitFor(() => expect(anbudWorkspace.lastSeen()).toBe('2026-10-28T10:00:00Z'))
  })
})
