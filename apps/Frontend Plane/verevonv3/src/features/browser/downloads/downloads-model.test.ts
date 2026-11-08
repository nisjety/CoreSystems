import { describe, expect, it, vi } from 'vitest'
import type { ActionActor, ActionExecution } from '@/shared/actions/types'
import { ApiError } from '@/shared/api/api-error'
import {
  DOWNLOADS_DRAWER_MAX,
  downloadKind,
  formatDownloadSize,
  readSessionDownloads,
  sendDownloadToKnowledge,
  sendToKnowledgeKey,
  type BrowserSessionDownload,
} from './downloads-model'

const actor: ActionActor = { type: 'human', userId: 'user_1', orgId: 'org_1' } as ActionActor

const pdf: BrowserSessionDownload = {
  artifactId: 'art_1',
  filename: 'report.pdf',
  mimeType: 'application/pdf',
  sizeBytes: 2048,
  kind: 'pdf',
}

function execution(result: unknown): ActionExecution {
  return {
    actionId: 'browser_download.send_to_knowledge',
    runId: 'run_x',
    status: 'completed',
    auditId: 'audit_x',
    eventStream: '',
    result,
  } as ActionExecution
}

describe('readSessionDownloads', () => {
  it('gives nothing for a session without downloads', () => {
    expect(readSessionDownloads(null)).toEqual([])
    expect(readSessionDownloads({})).toEqual([])
    expect(readSessionDownloads({ downloads: 'x' })).toEqual([])
  })

  it('reads both spellings, skips entries without an id and duplicates', () => {
    const list = readSessionDownloads({
      downloads: [
        { artifactId: 'a1', filename: 'notes.md', mimeType: 'text/markdown; charset=utf-8', sizeBytes: 10 },
        { artifact_id: 'a2', file_name: 'data.csv', content_type: 'text/csv', size_bytes: 20 },
        { filename: 'no-id.pdf' },
        { artifactId: 'a1', filename: 'again.md' },
        { artifactId: 'a3', mimeType: 'application/zip' },
      ],
    })
    expect(list.map((d) => d.artifactId)).toEqual(['a1', 'a2', 'a3'])
    expect(list[0]).toMatchObject({ kind: 'markdown', mimeType: 'text/markdown', sizeBytes: 10 })
    expect(list[1]).toMatchObject({ kind: 'csv', filename: 'data.csv', sizeBytes: 20 })
    expect(list[2]).toMatchObject({ kind: 'other', filename: 'a3', sizeBytes: undefined })
  })

  it('caps the list', () => {
    const downloads = Array.from({ length: DOWNLOADS_DRAWER_MAX + 5 }, (_, i) => ({ artifactId: `a${i}` }))
    expect(readSessionDownloads({ downloads })).toHaveLength(DOWNLOADS_DRAWER_MAX)
  })
})

describe('downloadKind', () => {
  it('uses the MIME type first and the extension only for a missing or generic type', () => {
    expect(downloadKind('application/pdf', 'x.bin')).toBe('pdf')
    expect(downloadKind('', 'x.TXT')).toBe('text')
    expect(downloadKind('application/octet-stream', 'x.pdf')).toBe('pdf')
    expect(downloadKind('application/zip', 'x.pdf')).toBe('other')
    expect(downloadKind('', 'noext')).toBe('other')
  })
})

describe('formatDownloadSize', () => {
  it('formats bytes', () => {
    expect(formatDownloadSize(undefined, 'en')).toBe('')
    expect(formatDownloadSize(512, 'en')).toBe('512 B')
    expect(formatDownloadSize(12_000, 'en')).toBe('12 kB')
    expect(formatDownloadSize(3_400_000, 'en')).toBe('3.4 MB')
    expect(formatDownloadSize(3_400_000, 'no')).toBe('3,4 MB')
  })
})

describe('sendDownloadToKnowledge', () => {
  it('refuses before any call under ZDR, for other kinds and without an actor', async () => {
    const execute = vi.fn()
    expect(await sendDownloadToKnowledge({ sessionId: 's1', download: pdf, zdr: true, actor, execute })).toEqual({
      outcome: 'refused',
      refusal: 'zdr',
    })
    expect(
      await sendDownloadToKnowledge({ sessionId: 's1', download: { ...pdf, kind: 'other' }, zdr: false, actor, execute }),
    ).toEqual({ outcome: 'refused', refusal: 'unsupported_type' })
    expect(await sendDownloadToKnowledge({ sessionId: 's1', download: pdf, zdr: false, actor: undefined, execute })).toEqual({
      outcome: 'refused',
      refusal: 'signed_out',
    })
    expect(execute).not.toHaveBeenCalled()
  })

  it('sends the contract input with a stable idempotency key and reads the answer', async () => {
    const execute = vi.fn().mockResolvedValue(execution({ documentId: 'doc_1', indexStatus: 'pending' }))
    const outcome = await sendDownloadToKnowledge({ sessionId: 's1', download: pdf, zdr: false, actor, execute })
    expect(outcome).toEqual({ outcome: 'sent', documentId: 'doc_1', indexStatus: 'pending' })
    expect(execute).toHaveBeenCalledWith(
      'browser_download.send_to_knowledge',
      actor,
      { sessionId: 's1', artifactId: 'art_1' },
      { idempotencyKey: sendToKnowledgeKey('s1', 'art_1') },
    )
  })

  it('treats a malformed answer and a thrown error as a refusal', async () => {
    const malformed = vi.fn().mockResolvedValue(execution({ documentId: '' }))
    expect(await sendDownloadToKnowledge({ sessionId: 's1', download: pdf, zdr: false, actor, execute: malformed })).toEqual({
      outcome: 'refused',
      refusal: 'failed',
    })
    const thrown = vi.fn().mockRejectedValue(new Error('network'))
    expect(await sendDownloadToKnowledge({ sessionId: 's1', download: pdf, zdr: false, actor, execute: thrown })).toEqual({
      outcome: 'refused',
      refusal: 'failed',
    })
  })

  it('maps an owner 404 to not_found', async () => {
    const missing = Object.assign(Object.create(ApiError.prototype) as ApiError, { status: 404, feature: null })
    const execute = vi.fn().mockRejectedValue(missing)
    expect(await sendDownloadToKnowledge({ sessionId: 's1', download: pdf, zdr: false, actor, execute })).toEqual({
      outcome: 'refused',
      refusal: 'not_found',
    })
  })
})
