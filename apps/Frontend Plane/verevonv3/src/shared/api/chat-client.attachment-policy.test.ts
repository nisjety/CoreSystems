import { afterEach, describe, expect, it, vi } from 'vitest'
import { getAttachmentPolicy, listModels, parseAttachmentPolicy } from './chat-client'

// The server declares what a chat turn may attach beside the model catalogue
// (model-gateway `attachment_policy` on /v1/models). The client reads it whole or
// not at all: absent or malformed means today's constants, never a partial policy.

const DECLARED = {
  image_mimes: ['image/png', 'image/jpeg', 'image/webp', 'image/gif'],
  max_images: 4,
  max_image_bytes: 1_000_000,
  max_files: 3,
  max_file_bytes: 1_000_000,
  max_total_bytes: 3_145_728,
  modes: { vision: true, sandbox_files: false },
}

function catalogue(body: unknown) {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  })))
}

describe('the declared attachment policy', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('is read from the catalogue response beside the models', async () => {
    catalogue({ models: [{ id: 'verevon-balance', features: ['vision'] }], attachment_policy: DECLARED })
    const models = await listModels()
    expect(models.map((model) => model.id)).toEqual(['verevon-balance'])
    expect(getAttachmentPolicy()).toEqual({
      imageMimes: DECLARED.image_mimes,
      maxImages: 4,
      maxImageBytes: 1_000_000,
      maxFiles: 3,
      maxFileBytes: 1_000_000,
      maxTotalBytes: 3_145_728,
      modes: { vision: true, sandboxFiles: false },
    })
  })

  it('is absent when the server declares none, so today\'s constants stay', async () => {
    catalogue({ models: [{ id: 'm1' }], attachment_policy: DECLARED })
    await listModels()
    expect(getAttachmentPolicy()).toBeDefined()
    catalogue({ models: [{ id: 'm1' }] })
    await listModels()
    expect(getAttachmentPolicy()).toBeUndefined()
    catalogue([{ id: 'legacy-array-shape' }])
    await listModels()
    expect(getAttachmentPolicy()).toBeUndefined()
  })

  it('is never half-trusted: one malformed field drops the whole policy', () => {
    expect(parseAttachmentPolicy(DECLARED)).toBeDefined()
    expect(parseAttachmentPolicy({ ...DECLARED, max_images: 0 })).toBeUndefined()
    expect(parseAttachmentPolicy({ ...DECLARED, max_images: '4' })).toBeUndefined()
    expect(parseAttachmentPolicy({ ...DECLARED, max_total_bytes: 1.5 })).toBeUndefined()
    expect(parseAttachmentPolicy({ ...DECLARED, image_mimes: ['image/png', 7] })).toBeUndefined()
    expect(parseAttachmentPolicy({ ...DECLARED, modes: { vision: true } })).toBeUndefined()
    expect(parseAttachmentPolicy({ ...DECLARED, modes: undefined })).toBeUndefined()
    expect(parseAttachmentPolicy(null)).toBeUndefined()
    expect(parseAttachmentPolicy('policy')).toBeUndefined()
  })
})
