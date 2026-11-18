import { afterEach, describe, expect, it, vi } from 'vitest'

import { getActionDescriptor } from '@/shared/actions/action-registry'
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor, ActionExecution } from '@/shared/actions/types'
import { ApiError } from '@/shared/api/http'
import type { SpaceCanvas, SpaceCanvasVersion, SpaceCanvasWriteResult } from '@/shared/api/spaces-client'

import {
  canvasDraftProblem,
  canvasTextBytes,
  CANVAS_MAX_BYTES,
  headAfterSave,
  isAmbiguousSaveFailure,
  isStaleSaveRefusal,
  pendingRestoreFor,
  pendingSaveFor,
  restoreSpaceCanvasVersion,
  saveSpaceCanvasVersion,
} from './space-canvas-save'

vi.mock('@/shared/actions/action-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/actions/action-client')>()),
  executeAction: vi.fn(),
}))

const mockedExecute = vi.mocked(executeAction)
const actor: ActionActor = { type: 'human', userId: 'user-kari', orgId: 'org-a' }

function version(overrides: Partial<SpaceCanvasVersion> = {}): SpaceCanvasVersion {
  return {
    versionId: 'v-4',
    versionNo: 4,
    parentVersionId: 'v-3',
    authorKind: 'user',
    authorId: 'subject-kari',
    onBehalfOf: null,
    runId: null,
    changeKind: 'edit',
    restoredFromVersionId: null,
    summary: null,
    contentBytes: 9,
    contentSha256: 'hash-4',
    createdAt: '2026-11-18T09:00:00.000Z',
    ...overrides,
  }
}

// The gateway's snake_case wire shape, as the action's result carries it.
function versionWire(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    version_id: 'v-4',
    version_no: 4,
    parent_version_id: 'v-3',
    author_kind: 'user',
    author_id: 'subject-kari',
    on_behalf_of: null,
    run_id: null,
    change_kind: 'edit',
    restored_from_version_id: null,
    summary: null,
    content_bytes: 9,
    content_sha256: 'hash-4',
    created_at: '2026-11-18T09:00:00.000Z',
    ...overrides,
  }
}

function execution(actionId: string, result: unknown): ActionExecution {
  return { actionId, runId: 'run-1', status: 'completed', auditId: 'audit-1', eventStream: '/events/run-1', result }
}

afterEach(() => {
  vi.restoreAllMocks()
  mockedExecute.mockReset()
})

describe('WS19-S6 canvas save helpers', () => {
  it('canvasDraftProblem gives empty for the empty string, nul for a NUL byte, too_large for 65537 bytes (multi-byte counted as bytes) and passes 65536 bytes and whitespace-only text', () => {
    expect(canvasDraftProblem('')).toBe('empty')
    expect(canvasDraftProblem('a\u0000b')).toBe('nul')
    expect(canvasDraftProblem('\u0000')).toBe('nul')

    const atLimit = 'a'.repeat(65_536)
    expect(canvasTextBytes(atLimit)).toBe(65_536)
    expect(canvasDraftProblem(atLimit)).toBeNull()
    expect(CANVAS_MAX_BYTES).toBe(65_536)

    const overLimit = 'a'.repeat(65_537)
    expect(canvasTextBytes(overLimit)).toBe(65_537)
    expect(canvasDraftProblem(overLimit)).toBe('too_large')

    // U+00E9 is two bytes in UTF-8: 32,769 characters is 65,538 bytes, so it is over the limit.
    const multiByteAtLimit = 'é'.repeat(32_768)
    expect(canvasTextBytes(multiByteAtLimit)).toBe(65_536)
    expect(canvasDraftProblem(multiByteAtLimit)).toBeNull()
    const multiByteOver = 'é'.repeat(32_769)
    expect(multiByteOver.length).toBeLessThan(65_536)
    expect(canvasDraftProblem(multiByteOver)).toBe('too_large')

    expect(canvasDraftProblem(' ')).toBeNull()
    expect(canvasDraftProblem(' \n\t  \r\n')).toBeNull()
  })

  it('pendingSaveFor reuses the id for the same base and text and mints a new id otherwise', () => {
    const mint = vi.spyOn(crypto, 'randomUUID')
    mint.mockReturnValueOnce('11111111-1111-4111-8111-111111111111')
    const first = pendingSaveFor(null, 'v-3', 'Hei')
    expect(first).toEqual({ clientSaveId: '11111111-1111-4111-8111-111111111111', expectedVersionId: 'v-3', content: 'Hei' })

    // A retry of the same draft over the same head keeps its id, so nothing new is minted.
    expect(pendingSaveFor(first, 'v-3', 'Hei')).toBe(first)
    expect(mint).toHaveBeenCalledTimes(1)

    mint.mockReturnValueOnce('22222222-2222-4222-8222-222222222222')
    expect(pendingSaveFor(first, 'v-4', 'Hei').clientSaveId).toBe('22222222-2222-4222-8222-222222222222')

    mint.mockReturnValueOnce('33333333-3333-4333-8333-333333333333')
    expect(pendingSaveFor(first, 'v-3', 'Hei igjen').clientSaveId).toBe('33333333-3333-4333-8333-333333333333')

    mint.mockReturnValueOnce('44444444-4444-4444-8444-444444444444')
    expect(pendingSaveFor(first, null, 'Hei').clientSaveId).toBe('44444444-4444-4444-8444-444444444444')
  })

  it('isStaleSaveRefusal is true only for canvas_head_moved, space_decision_digest_mismatch and idempotency_key_conflict', () => {
    for (const code of ['canvas_head_moved', 'space_decision_digest_mismatch', 'idempotency_key_conflict']) {
      expect(isStaleSaveRefusal(new ApiError(code, 409, code))).toBe(true)
    }
    for (const code of ['canvas_unavailable', 'canvas_write_not_authorized', 'idempotency_conflict', 'space_decision_required']) {
      expect(isStaleSaveRefusal(new ApiError(code, 409, code))).toBe(false)
    }
    expect(isStaleSaveRefusal(new ApiError('refused', 409, null))).toBe(false)
    expect(isStaleSaveRefusal(Object.assign(new Error('canvas_head_moved'), { code: 'canvas_head_moved' }))).toBe(false)
    expect(isStaleSaveRefusal(new TypeError('Failed to fetch'))).toBe(false)
    expect(isStaleSaveRefusal(undefined)).toBe(false)
  })

  it('isAmbiguousSaveFailure is true for a TypeError, 5xx and 408 and false for 400, 403, 409 and 429', () => {
    expect(isAmbiguousSaveFailure(new TypeError('Failed to fetch'))).toBe(true)
    for (const status of [500, 502, 503, 408]) {
      expect(isAmbiguousSaveFailure(new ApiError('unavailable', status, null))).toBe(true)
    }
    for (const status of [400, 403, 409, 429]) {
      expect(isAmbiguousSaveFailure(new ApiError('refused', status, null))).toBe(false)
    }
    expect(isAmbiguousSaveFailure(new Error('boom'))).toBe(false)
  })

  it('headAfterSave is null for unchanged and returns the head with the saved text', () => {
    const before: SpaceCanvas = {
      canvasId: 'canvas-1',
      spaceRef: 'room-a',
      state: 'active',
      headVersionId: 'v-3',
      headVersionNo: 3,
      createdAt: '2026-11-01T08:00:00.000Z',
      updatedAt: '2026-11-17T12:00:00.000Z',
    }
    const unchanged: SpaceCanvasWriteResult = { unchanged: true, canvasId: 'canvas-1', headVersionId: 'v-3' }
    expect(headAfterSave(unchanged, 'Hei', 'room-a', before)).toBeNull()

    const stored: Extract<SpaceCanvasWriteResult, { unchanged: false }> = {
      unchanged: false,
      canvasId: 'canvas-1',
      version: version({ versionId: 'v-4', versionNo: 4, createdAt: '2026-11-18T09:00:00.000Z' }),
      deduplicated: false,
    }
    const after = headAfterSave(stored, 'Ny tekst', 'room-a', before)
    expect(after).toEqual({
      canvas: {
        canvasId: 'canvas-1',
        spaceRef: 'room-a',
        state: 'active',
        headVersionId: 'v-4',
        headVersionNo: 4,
        createdAt: '2026-11-01T08:00:00.000Z',
        updatedAt: '2026-11-18T09:00:00.000Z',
      },
      head: { ...stored.version, content: 'Ny tekst' },
    })
    expect(after?.head.content).toBe('Ny tekst')

    // With no canvas held yet, the head's own creation time stands in for the canvas's.
    expect(headAfterSave(stored, 'Ny tekst', 'room-a', null)?.canvas.createdAt).toBe('2026-11-18T09:00:00.000Z')
  })

  it('saveSpaceCanvasVersion calls executeAction with spaces.save_canvas, the clientSaveId as idempotencyKey and maps the result', async () => {
    mockedExecute.mockResolvedValueOnce(
      execution('spaces.save_canvas', { canvas_id: 'canvas-1', version: versionWire(), deduplicated: false }),
    )
    const saved = await saveSpaceCanvasVersion('room-a', actor, {
      expectedVersionId: 'v-3',
      content: 'Ny tekst',
      clientSaveId: 'save-attempt-0001',
    })
    expect(mockedExecute).toHaveBeenCalledWith(
      'spaces.save_canvas',
      actor,
      { spaceRef: 'room-a', expectedVersionId: 'v-3', content: 'Ny tekst', clientSaveId: 'save-attempt-0001' },
      { idempotencyKey: 'save-attempt-0001' },
    )
    expect(saved).toEqual({
      unchanged: false,
      canvasId: 'canvas-1',
      version: expect.objectContaining({ versionId: 'v-4', versionNo: 4, parentVersionId: 'v-3', changeKind: 'edit' }),
      deduplicated: false,
    })

    mockedExecute.mockResolvedValueOnce(
      execution('spaces.save_canvas', { canvas_id: 'canvas-1', head_version_id: 'v-3', unchanged: true }),
    )
    const unchanged = await saveSpaceCanvasVersion('room-a', actor, {
      expectedVersionId: 'v-3',
      content: 'Ny tekst',
      clientSaveId: 'save-attempt-0002',
    })
    expect(unchanged).toEqual({ unchanged: true, canvasId: 'canvas-1', headVersionId: 'v-3' })
  })
})

describe('WS19-S7 canvas restore helpers', () => {
  it('pendingRestoreFor keeps the id for the same version and head and mints a new one otherwise', () => {
    const mint = vi.spyOn(crypto, 'randomUUID')
    mint.mockReturnValueOnce('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa')
    const first = pendingRestoreFor(null, 'v-2', 'v-5')
    expect(first).toEqual({
      clientSaveId: 'canvas-restore-aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
      versionId: 'v-2',
      expectedVersionId: 'v-5',
    })

    expect(pendingRestoreFor(first, 'v-2', 'v-5')).toBe(first)
    expect(mint).toHaveBeenCalledTimes(1)

    mint.mockReturnValueOnce('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb')
    expect(pendingRestoreFor(first, 'v-1', 'v-5').clientSaveId).toBe('canvas-restore-bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb')

    mint.mockReturnValueOnce('cccccccc-cccc-4ccc-8ccc-cccccccccccc')
    expect(pendingRestoreFor(first, 'v-2', 'v-6').clientSaveId).toBe('canvas-restore-cccccccc-cccc-4ccc-8ccc-cccccccccccc')
  })

  it('every restore id starts with canvas-restore-, is 16 to 64 characters and matches the action clientSaveId pattern', () => {
    const descriptor = getActionDescriptor('spaces.restore_canvas_version')
    expect(descriptor).toBeDefined()

    for (let attempt = 0; attempt < 5; attempt += 1) {
      const { clientSaveId } = pendingRestoreFor(null, 'v-2', null)
      expect(clientSaveId.startsWith('canvas-restore-')).toBe(true)
      expect(clientSaveId.length).toBeGreaterThanOrEqual(16)
      expect(clientSaveId.length).toBeLessThanOrEqual(64)
      const accepted = descriptor?.inputSchema.safeParse({
        spaceRef: 'room-a',
        versionId: 'v-2',
        expectedVersionId: null,
        clientSaveId,
      })
      expect(accepted?.success).toBe(true)
    }
  })

  it('restoreSpaceCanvasVersion calls executeAction with spaces.restore_canvas_version and the id as idempotencyKey, and maps a 201 and a 200 unchanged', async () => {
    const clientSaveId = 'canvas-restore-restore-0001'

    // The gateway answers 201 with the stored version, which is the new head.
    mockedExecute.mockResolvedValueOnce(
      execution('spaces.restore_canvas_version', {
        canvas_id: 'canvas-1',
        version: versionWire({
          version_id: 'v-6',
          version_no: 6,
          parent_version_id: 'v-5',
          change_kind: 'restore',
          restored_from_version_id: 'v-2',
        }),
        deduplicated: false,
      }),
    )
    const restored = await restoreSpaceCanvasVersion('room-a', actor, {
      versionId: 'v-2',
      expectedVersionId: 'v-5',
      clientSaveId,
    })
    expect(mockedExecute).toHaveBeenCalledWith(
      'spaces.restore_canvas_version',
      actor,
      { spaceRef: 'room-a', versionId: 'v-2', expectedVersionId: 'v-5', clientSaveId },
      { idempotencyKey: clientSaveId },
    )
    expect(restored).toEqual({
      unchanged: false,
      canvasId: 'canvas-1',
      version: expect.objectContaining({ versionId: 'v-6', versionNo: 6, changeKind: 'restore', restoredFromVersionId: 'v-2' }),
      deduplicated: false,
    })

    // The gateway answers 200 with the unchanged head when that head already holds the text.
    mockedExecute.mockResolvedValueOnce(
      execution('spaces.restore_canvas_version', { canvas_id: 'canvas-1', head_version_id: 'v-5', unchanged: true }),
    )
    const unchanged = await restoreSpaceCanvasVersion('room-a', actor, {
      versionId: 'v-2',
      expectedVersionId: 'v-5',
      clientSaveId,
    })
    expect(unchanged).toEqual({ unchanged: true, canvasId: 'canvas-1', headVersionId: 'v-5' })
  })
})
