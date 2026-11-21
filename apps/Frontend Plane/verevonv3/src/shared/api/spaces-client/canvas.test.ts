import { afterEach, describe, expect, it, vi } from 'vitest'

import { restoreSpaceCanvasVersion, saveSpaceCanvasVersion } from '@/features/spaces/lib/space-canvas-save'
import type { ActionActor } from '@/shared/actions/types'
import { getSpaceCanvas, getSpaceCanvasVersion, listSpaceCanvasVersions } from './canvas'

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
  vi.unstubAllEnvs()
})

const SHA = 'a'.repeat(64)
const actor: ActionActor = { type: 'human', userId: 'user_1', orgId: 'org_1' }

const version = (over: Record<string, unknown> = {}) => ({
  version_id: 'ver_1',
  version_no: 1,
  parent_version_id: null,
  author_kind: 'user',
  author_id: 'user_1',
  on_behalf_of: null,
  run_id: null,
  change_kind: 'create',
  restored_from_version_id: null,
  summary: null,
  content_bytes: 4,
  content_sha256: SHA,
  created_at: '2026-11-01T10:00:00Z',
  ...over,
})

const canvas = {
  canvas_id: 'cnv_1',
  space_ref: 'room 1',
  state: 'active',
  head_version_id: 'ver_2',
  head_version_no: 2,
  created_at: '2026-11-01T09:00:00Z',
  updated_at: '2026-11-01T10:00:00Z',
}

const actionReply = (actionId: string, result: unknown) => ({
  data: { actionId, runId: 'run_1', status: 'completed', auditId: 'aud_1', eventStream: 'stream_1', result },
})

describe('getSpaceCanvas (WS19-S5)', () => {
  it('maps the canvas, its head with the text, and can_write', async () => {
    const calls = stubFetch(200, {
      data: { canvas, head: version({ version_id: 'ver_2', version_no: 2, content: 'Plan' }), can_write: true },
    })
    const read = await getSpaceCanvas('room 1')
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room%201/canvas')).toBe(true)
    expect(read).toMatchObject({
      unchanged: false,
      canWrite: true,
      canvas: { canvasId: 'cnv_1', headVersionId: 'ver_2', headVersionNo: 2 },
      head: { versionId: 'ver_2', versionNo: 2, content: 'Plan' },
    })
  })

  it('answers unchanged with the head id and no text when the caller already holds that head', async () => {
    const calls = stubFetch(200, { data: { unchanged: true, head_version_id: 'ver_2' } })
    const read = await getSpaceCanvas('room 1', 'ver 2')
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room%201/canvas?known_version_id=ver%202')).toBe(true)
    expect(read).toEqual({ unchanged: true, headVersionId: 'ver_2' })
  })

  it('reads a null canvas as the empty room, not as an error', async () => {
    stubFetch(200, { data: { canvas: null, head: null, can_write: true } })
    await expect(getSpaceCanvas('room 1')).resolves.toEqual({ unchanged: false, canvas: null, head: null, canWrite: true })
  })

  it('answers 503 canvas_unavailable for a canvas that has no head', async () => {
    stubFetch(200, { data: { canvas, head: null, can_write: true } })
    await expect(getSpaceCanvas('room 1')).rejects.toMatchObject({ status: 503, code: 'canvas_unavailable' })
  })
})

describe('canvas writes through the action registry (WS19-S5)', () => {
  it('reads a stored version (the 201 answer) and the unchanged head (the 200 answer) from the save action', async () => {
    const stored = stubFetch(
      200,
      actionReply('spaces.save_canvas', {
        canvas_id: 'cnv_1',
        version: version({ version_id: 'ver_2', version_no: 2, summary: 'Plan' }),
        deduplicated: false,
      }),
    )
    const saved = await saveSpaceCanvasVersion('room 1', actor, {
      expectedVersionId: 'ver_1',
      content: 'Plan',
      clientSaveId: 'save-0000000000000001',
    })
    expect(JSON.parse(String(stored[0]?.init?.body))).toEqual({
      actionId: 'spaces.save_canvas',
      idempotencyKey: 'save-0000000000000001',
      input: { spaceRef: 'room 1', expectedVersionId: 'ver_1', content: 'Plan', clientSaveId: 'save-0000000000000001' },
    })
    expect(saved).toMatchObject({
      unchanged: false,
      canvasId: 'cnv_1',
      deduplicated: false,
      version: { versionId: 'ver_2', versionNo: 2 },
    })

    stubFetch(200, actionReply('spaces.save_canvas', { canvas_id: 'cnv_1', head_version_id: 'ver_2', unchanged: true }))
    await expect(
      saveSpaceCanvasVersion('room 1', actor, {
        expectedVersionId: 'ver_2',
        content: 'Plan',
        clientSaveId: 'save-0000000000000002',
      }),
    ).resolves.toEqual({ unchanged: true, canvasId: 'cnv_1', headVersionId: 'ver_2' })
  })

  it('restoreSpaceCanvasVersion sends the expected head and the client save id as its action input', async () => {
    const calls = stubFetch(
      200,
      actionReply('spaces.restore_canvas_version', {
        canvas_id: 'cnv_1',
        version: version({ version_id: 'ver_3', version_no: 3, change_kind: 'restore', restored_from_version_id: 'ver_1' }),
        deduplicated: false,
      }),
    )
    const restored = await restoreSpaceCanvasVersion('room 1', actor, {
      versionId: 'ver_1',
      expectedVersionId: 'ver_2',
      clientSaveId: 'canvas-restore-0000000000000002',
    })
    expect(JSON.parse(String(calls[0]?.init?.body))).toEqual({
      actionId: 'spaces.restore_canvas_version',
      idempotencyKey: 'canvas-restore-0000000000000002',
      input: {
        spaceRef: 'room 1',
        versionId: 'ver_1',
        expectedVersionId: 'ver_2',
        clientSaveId: 'canvas-restore-0000000000000002',
      },
    })
    expect(restored).toMatchObject({
      unchanged: false,
      version: { versionId: 'ver_3', changeKind: 'restore', restoredFromVersionId: 'ver_1' },
    })
  })
})

describe('listSpaceCanvasVersions (WS19-S5, WS19-S7)', () => {
  it('reads a numeric meta.next_cursor as a digit string and null at the end', async () => {
    const calls = stubFetch(200, { data: [version()], meta: { limit: 1, next_cursor: 41 } })
    const page = await listSpaceCanvasVersions('room 1', { limit: 1 })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room%201/canvas/versions?limit=1')).toBe(true)
    expect(page.versions).toMatchObject([{ versionId: 'ver_1', versionNo: 1 }])
    expect(page.nextCursor).toBe('41')

    stubFetch(200, { data: [version()], meta: { limit: 1, next_cursor: null } })
    expect((await listSpaceCanvasVersions('room 1')).nextCursor).toBeNull()
    stubFetch(200, { data: [version()] })
    expect((await listSpaceCanvasVersions('room 1')).nextCursor).toBeNull()
  })

  it('reads meta.window as the member history window (D21m), and null when the history is whole', async () => {
    stubFetch(200, {
      data: [version()],
      meta: { limit: 50, next_cursor: null, window: { starts_at: '2026-11-01T00:00:00Z', hidden_before: true } },
    })
    expect((await listSpaceCanvasVersions('room 1')).window).toEqual({
      startsAt: '2026-11-01T00:00:00Z',
      hiddenBefore: true,
    })
    stubFetch(200, { data: [version()], meta: { limit: 50, next_cursor: null, window: { starts_at: null, hidden_before: false } } })
    expect((await listSpaceCanvasVersions('room 1')).window).toEqual({ startsAt: null, hiddenBefore: false })
    stubFetch(200, { data: [version()], meta: { limit: 50, next_cursor: null } })
    expect((await listSpaceCanvasVersions('room 1')).window).toBeNull()
  })

  it('throws the typed ApiError on 403', async () => {
    stubFetch(403, { error: { code: 'space_read_not_authorized', message: 'no' } })
    await expect(listSpaceCanvasVersions('room 1')).rejects.toMatchObject({
      status: 403,
      code: 'space_read_not_authorized',
    })
  })

  it('sends the dev bypass header through requestJsonPage only while the bypass is on', async () => {
    vi.stubEnv('VITE_ALLOW_DEV_AUTH_BYPASS', 'true')
    const bypassed = stubFetch(200, { data: [], meta: null })
    await listSpaceCanvasVersions('room 1')
    expect(new Headers(bypassed[0]?.init?.headers).get('Authorization')).toBe('Bearer dev-bypass')

    vi.stubEnv('VITE_ALLOW_DEV_AUTH_BYPASS', 'false')
    const plain = stubFetch(200, { data: [], meta: null })
    await listSpaceCanvasVersions('room 1')
    expect(new Headers(plain[0]?.init?.headers).get('Authorization')).toBeNull()
  })

  it('fails the whole page with canvas_unavailable when one row is not a version', async () => {
    stubFetch(200, { data: [version(), { version_id: 'ver_9' }], meta: { next_cursor: null } })
    await expect(listSpaceCanvasVersions('room 1')).rejects.toMatchObject({ status: 503, code: 'canvas_unavailable' })
  })
})

describe('getSpaceCanvasVersion (WS19-S5)', () => {
  it('keeps the version text and encodes the ids in the path', async () => {
    const calls = stubFetch(200, { data: version({ version_id: 'ver/1', content: 'Plan' }) })
    const got = await getSpaceCanvasVersion('room 1', 'ver/1')
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room%201/canvas/versions/ver%2F1')).toBe(true)
    expect(got).toMatchObject({ versionId: 'ver/1', content: 'Plan' })
  })
})
