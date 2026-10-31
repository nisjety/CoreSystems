import { beforeEach, describe, expect, it, vi } from 'vitest'

import { getAttachmentPolicy } from '@/shared/api/chat-client'

import { toStreamAttachments } from './chat-normalizers'

// A document's bytes go to the server only when it declares the sandbox staging
// (`attachment_policy.modes.sandbox_files`). Its text is inlined into the turn
// separately, so a server without the switch receives exactly what it always did.

vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  getAttachmentPolicy: vi.fn(() => undefined),
}))

const policy = (sandboxFiles: boolean) => ({
  imageMimes: ['image/png'],
  maxImages: 4,
  maxImageBytes: 1_000_000,
  maxFiles: 3,
  maxFileBytes: 1_000_000,
  maxTotalBytes: 3_145_728,
  modes: { vision: true, sandboxFiles },
})

const csv = (name = 'sales.csv') => ({
  id: 'a1',
  name,
  size: 8,
  type: 'text/csv',
  // "month,rev" as base64.
  url: 'data:text/csv;base64,bW9udGgscmV2',
})

const png = {
  id: 'i1',
  name: 'chart.png',
  size: 4,
  type: 'image/png',
  url: 'data:image/png;base64,aGk=',
}

describe('toStreamAttachments with the sandbox staging', () => {
  beforeEach(() => {
    vi.mocked(getAttachmentPolicy).mockReset()
  })

  it('sends a document as bytes when the server declares sandbox files', async () => {
    vi.mocked(getAttachmentPolicy).mockReturnValue(policy(true))
    expect(await toStreamAttachments([csv()])).toEqual([
      { kind: 'file', name: 'sales.csv', data_base64: 'bW9udGgscmV2', mime_type: 'text/csv' },
    ])
  })

  it('sends nothing for a document when the server declares none or declines', async () => {
    vi.mocked(getAttachmentPolicy).mockReturnValue(undefined)
    expect(await toStreamAttachments([csv()])).toEqual([])
    vi.mocked(getAttachmentPolicy).mockReturnValue(policy(false))
    expect(await toStreamAttachments([csv()])).toEqual([])
  })

  it('stages only the document kinds a program reads, and keeps images as before', async () => {
    vi.mocked(getAttachmentPolicy).mockReturnValue(policy(true))
    const out = await toStreamAttachments([csv('notes.txt'), png, csv('model.xlsx')])
    expect(out.map((entry) => entry.kind)).toEqual(['image', 'file'])
    expect(out[1]).toMatchObject({ kind: 'file', name: 'model.xlsx' })
  })

  it('refuses a file it cannot read instead of dropping it', async () => {
    vi.mocked(getAttachmentPolicy).mockReturnValue(policy(true))
    await expect(toStreamAttachments([{ ...csv(), url: '' }])).rejects.toThrow('sales.csv')
  })
})
