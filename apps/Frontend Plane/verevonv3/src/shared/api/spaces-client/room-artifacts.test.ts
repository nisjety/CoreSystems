import { afterEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '../http'
import { getSpaceArtifacts, SPACE_ARTIFACT_PAGE_SIZE } from './room-artifacts'

type FetchCall = { url: string; init: RequestInit | undefined }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), init })
      return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
    }),
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const row = (over: Record<string, unknown> = {}) => ({
  thread_id: 't1',
  message_id: 'm1',
  artifact_id: 'a1',
  kind: 'document',
  title: 'Plan',
  version: 2,
  version_count: 3,
  size_chars: 120,
  agent_name: 'Lagerhjelpen',
  created_at: '2026-10-01T10:00:00Z',
  ...over,
})

describe('getSpaceArtifacts (WS07-S9.spa, WS07-S12)', () => {
  it('GETs /api/v1/spaces/{ref}/artifacts with the default limit and no cursor', async () => {
    const calls = stubFetch(200, { data: { artifacts: [] } })
    await getSpaceArtifacts('room 1')
    expect(calls).toHaveLength(1)
    const url = new URL(calls[0]!.url, 'http://localhost')
    expect(url.pathname).toBe('/api/v1/spaces/room%201/artifacts')
    expect(url.searchParams.get('limit')).toBe(String(SPACE_ARTIFACT_PAGE_SIZE))
    expect(SPACE_ARTIFACT_PAGE_SIZE).toBe(50)
    expect(url.searchParams.has('cursor')).toBe(false)
    expect((calls[0]!.init?.method ?? 'GET').toUpperCase()).toBe('GET')
  })

  it('encodes limit and cursor into the query', async () => {
    const calls = stubFetch(200, { data: { artifacts: [] } })
    await getSpaceArtifacts('room', { limit: 10, cursor: 'a b/c+d=' })
    const url = new URL(calls[0]!.url, 'http://localhost')
    expect(url.searchParams.get('limit')).toBe('10')
    expect(url.searchParams.get('cursor')).toBe('a b/c+d=')
  })

  it('maps the summary fields', async () => {
    stubFetch(200, {
      data: { artifacts: [row()], next_cursor: 'c2', shared_read: { available: true }, retention: { zdr: false } },
    })
    const page = await getSpaceArtifacts('room')
    expect(page.artifacts).toEqual([
      {
        thread_id: 't1',
        message_id: 'm1',
        artifact_id: 'a1',
        kind: 'document',
        title: 'Plan',
        version: 2,
        version_count: 3,
        size_chars: 120,
        agent_name: 'Lagerhjelpen',
        created_at: '2026-10-01T10:00:00Z',
      },
    ])
    expect(page.nextCursor).toBe('c2')
    expect(page.sharedReadAvailable).toBe(true)
    expect(page.zdr).toBe(false)
  })

  it('drops kinds outside document/code/html, entries without ids and versions below 1', async () => {
    stubFetch(200, {
      data: {
        artifacts: [
          row({ artifact_id: 'pdf', kind: 'pdf' }),
          row({ artifact_id: 'img', kind: 'image' }),
          row({ artifact_id: 'bin', kind: 'binary' }),
          row({ artifact_id: undefined }),
          row({ thread_id: '  ' }),
          row({ artifact_id: 'v0', version: 0 }),
          row({ artifact_id: 'vfrac', version: 1.5 }),
          row({ artifact_id: 'vstr', version: '2' }),
          row({ artifact_id: 'long', title: 'x'.repeat(201) }),
          row({ artifact_id: 'code', kind: 'code' }),
          row({ artifact_id: 'html', kind: 'html' }),
          null,
          'junk',
        ],
      },
    })
    const page = await getSpaceArtifacts('room')
    expect(page.artifacts.map((a) => a.artifact_id)).toEqual(['code', 'html'])
  })

  it('defaults optional counts and omits blank agent name and created_at', async () => {
    stubFetch(200, {
      data: { artifacts: [{ thread_id: 't', artifact_id: 'a', kind: 'code', version: 1, agent_name: '  ' }] },
    })
    const [a] = (await getSpaceArtifacts('room')).artifacts
    expect(a).toMatchObject({ message_id: '', title: '', version_count: 1, size_chars: 0 })
    expect(a).not.toHaveProperty('agent_name')
    expect(a).not.toHaveProperty('created_at')
  })

  it('returns next_cursor null on the last page, including an empty string or missing field', async () => {
    stubFetch(200, { data: { artifacts: [row()], next_cursor: null } })
    expect((await getSpaceArtifacts('room')).nextCursor).toBeNull()
    stubFetch(200, { data: { artifacts: [row()], next_cursor: '' } })
    expect((await getSpaceArtifacts('room')).nextCursor).toBeNull()
    stubFetch(200, { data: { artifacts: [row()] } })
    expect((await getSpaceArtifacts('room')).nextCursor).toBeNull()
  })

  it('maps shared_read.available=false only on an explicit false', async () => {
    stubFetch(200, { data: { artifacts: [], shared_read: { available: false } } })
    expect((await getSpaceArtifacts('room')).sharedReadAvailable).toBe(false)
    stubFetch(200, { data: { artifacts: [] } })
    expect((await getSpaceArtifacts('room')).sharedReadAvailable).toBe(true)
    stubFetch(200, { data: { artifacts: [], shared_read: null } })
    expect((await getSpaceArtifacts('room')).sharedReadAvailable).toBe(true)
    stubFetch(200, { data: { artifacts: [], shared_read: { available: 'no' } } })
    expect((await getSpaceArtifacts('room')).sharedReadAvailable).toBe(true)
  })

  it('maps retention.zdr true, false and anything else to null', async () => {
    stubFetch(200, { data: { artifacts: [], retention: { zdr: true } } })
    expect((await getSpaceArtifacts('room')).zdr).toBe(true)
    stubFetch(200, { data: { artifacts: [], retention: { zdr: false } } })
    expect((await getSpaceArtifacts('room')).zdr).toBe(false)
    stubFetch(200, { data: { artifacts: [] } })
    expect((await getSpaceArtifacts('room')).zdr).toBeNull()
    stubFetch(200, { data: { artifacts: [], retention: { zdr: 'yes' } } })
    expect((await getSpaceArtifacts('room')).zdr).toBeNull()
  })

  it('tolerates a missing or malformed artifacts field', async () => {
    stubFetch(200, { data: {} })
    expect((await getSpaceArtifacts('room')).artifacts).toEqual([])
    stubFetch(200, { data: { artifacts: 'nope' } })
    expect((await getSpaceArtifacts('room')).artifacts).toEqual([])
  })

  it('surfaces a 403 space_read_not_authorized as an ApiError with the code', async () => {
    stubFetch(403, { error: { code: 'space_read_not_authorized', message: 'no', details: {} } })
    const error = await getSpaceArtifacts('room').catch((e: unknown) => e)
    expect(error).toBeInstanceOf(ApiError)
    expect((error as ApiError).status).toBe(403)
    expect((error as ApiError).code).toBe('space_read_not_authorized')
  })

  it('surfaces a 404 space_room_artifacts_disabled as an ApiError with the code', async () => {
    stubFetch(404, { error: { code: 'space_room_artifacts_disabled', message: 'off', details: {} } })
    const error = await getSpaceArtifacts('room').catch((e: unknown) => e)
    expect(error).toBeInstanceOf(ApiError)
    expect((error as ApiError).status).toBe(404)
    expect((error as ApiError).code).toBe('space_room_artifacts_disabled')
  })
})
