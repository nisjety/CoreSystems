import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  exportStudioProjectToSocialDraft,
  listStudioProjects,
  saveStudioProject,
} from './studio-client'

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ data }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('studio API client', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('lists Studio projects through the gateway with org context', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ projects: [] }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await listStudioProjects('org_1')

    expect(result.projects).toEqual([])
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/studio/projects')
    expect(init.credentials).toBe('include')
    expect((init.headers as Headers).get('x-velion-org-id')).toBe('org_1')
  })

  it('saves canvas blocks without mutating caller data', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({
      project: {
        id: 'project_1',
        orgId: 'org_1',
        ownerUserId: 'user_1',
        updatedByUserId: 'user_1',
        title: 'Launch canvas',
        status: 'draft',
        blocks: [],
        createdAt: '2026-06-16T10:00:00Z',
        updatedAt: '2026-06-16T10:00:00Z',
      },
    }))
    vi.stubGlobal('fetch', fetchMock)
    const block = {
      id: 'block_1',
      kind: 'text' as const,
      title: 'Hook',
      body: 'Launch note',
      x: 10,
      y: 20,
      width: 300,
      height: 180,
    }

    await saveStudioProject('org_1', 'project_1', { blocks: [block], selectedBlockId: block.id })

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/studio/projects/project_1')
    expect(init.method).toBe('PUT')
    expect(JSON.parse(init.body as string)).toMatchObject({
      blocks: [block],
      selectedBlockId: 'block_1',
    })
    expect(block.title).toBe('Hook')
  })

  it('exports Studio projects to Social drafts', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({
      project: {
        id: 'project_1',
        orgId: 'org_1',
        ownerUserId: 'user_1',
        updatedByUserId: 'user_1',
        title: 'Launch canvas',
        status: 'draft',
        blocks: [],
        socialDraftId: 'social_studio_1',
        socialExportedAt: '2026-06-16T10:00:00Z',
        createdAt: '2026-06-16T10:00:00Z',
        updatedAt: '2026-06-16T10:00:00Z',
      },
      socialPost: {
        id: 'social_studio_1',
        title: 'Launch canvas',
        body: 'Launch note',
        status: 'draft',
        scheduledAt: '2026-06-17T10:00:00Z',
        platforms: ['linkedin'],
        source: { kind: 'campaign', label: 'Launch canvas' },
        approval: { required: true, state: 'not_requested' },
        media: [],
      },
    }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await exportStudioProjectToSocialDraft('org_1', 'project_1', {
      platforms: ['linkedin'],
    })

    expect(result.socialPost.id).toBe('social_studio_1')
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/studio/projects/project_1/export/social-draft')
    expect(init.method).toBe('POST')
    expect((init.headers as Headers).get('x-velion-org-id')).toBe('org_1')
    expect(JSON.parse(init.body as string)).toEqual({ platforms: ['linkedin'] })
  })
})
