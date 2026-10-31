import { afterEach, describe, expect, it, vi } from 'vitest'
import { doAttachmentsEnabled, visionAsPartsEnabled } from '@/features/chat/lib/attachment-policy'
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
  modes: { vision: true, sandbox_files: false, vision_as_parts: false, do_attachments: false },
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
      modes: { vision: true, sandboxFiles: false, visionAsParts: false, doAttachments: false },
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
    const stringSwitch = { ...DECLARED.modes, vision_as_parts: 'true' }
    expect(parseAttachmentPolicy({ ...DECLARED, modes: stringSwitch })).toBeUndefined()
    expect(parseAttachmentPolicy({ ...DECLARED, modes: { ...DECLARED.modes, do_attachments: 1 } })).toBeUndefined()
    expect(parseAttachmentPolicy(null)).toBeUndefined()
    expect(parseAttachmentPolicy('policy')).toBeUndefined()
  })

  it('reads the switches for images as parts and Do attachments, and an older server as off', () => {
    const declared = parseAttachmentPolicy({
      ...DECLARED,
      modes: { vision: true, sandbox_files: false, vision_as_parts: true, do_attachments: true },
    })
    expect(declared?.modes).toEqual({ vision: true, sandboxFiles: false, visionAsParts: true, doAttachments: true })
    const older = parseAttachmentPolicy({ ...DECLARED, modes: { vision: true, sandbox_files: false } })
    expect(older?.modes).toEqual({ vision: true, sandboxFiles: false, visionAsParts: false, doAttachments: false })
  })
})

describe('the gates follow the declared policy before the build flag', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.unstubAllEnvs()
  })

  it('uses the server declaration once a policy is loaded, whatever the build flag says', async () => {
    vi.stubEnv('VITE_CHAT_VISION_AS_PARTS_ENABLED', 'true')
    vi.stubEnv('VITE_CHAT_DO_ATTACHMENTS_ENABLED', 'true')
    catalogue({ models: [{ id: 'm1' }], attachment_policy: DECLARED })
    await listModels()
    expect(visionAsPartsEnabled()).toBe(false)
    expect(doAttachmentsEnabled()).toBe(false)

    vi.stubEnv('VITE_CHAT_VISION_AS_PARTS_ENABLED', 'false')
    vi.stubEnv('VITE_CHAT_DO_ATTACHMENTS_ENABLED', 'false')
    catalogue({
      models: [{ id: 'm1' }],
      attachment_policy: {
        ...DECLARED,
        modes: { vision: true, sandbox_files: false, vision_as_parts: true, do_attachments: true },
      },
    })
    await listModels()
    expect(visionAsPartsEnabled()).toBe(true)
    expect(doAttachmentsEnabled()).toBe(true)
  })

  it('falls back to the build flag when no policy is loaded', async () => {
    catalogue({ models: [{ id: 'm1' }] })
    await listModels()
    expect(getAttachmentPolicy()).toBeUndefined()
    vi.stubEnv('VITE_CHAT_VISION_AS_PARTS_ENABLED', 'true')
    vi.stubEnv('VITE_CHAT_DO_ATTACHMENTS_ENABLED', '')
    expect(visionAsPartsEnabled()).toBe(true)
    expect(doAttachmentsEnabled()).toBe(false)
  })
})
