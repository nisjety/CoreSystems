import { describe, expect, it } from 'vitest'
import { streamFailurePresentation } from '@/features/chat/components/chat-normalizers'
import { translateApiError } from '@/shared/i18n/errors'
import {
  ATTACHMENT_ACCEPT,
  ATTACHMENT_MAX_FILE_BYTES,
  ATTACHMENT_MAX_IMAGES,
  attachmentRefusalMessage,
  evaluateAttachment,
  evaluateAttachments,
  type AttachmentPolicyContext,
} from './attachment-policy'

const ask: AttachmentPolicyContext = {
  subscriptionBacked: false,
  planMode: false,
  zdr: false,
  currentImageCount: 0,
  currentTotalBytes: 0,
}

const png = { name: 'chart.png', size: 20_000, type: 'image/png' }
const sheet = { name: 'sales.xlsx', size: 40_000, type: 'application/octet-stream' }

const norwegian = (no: string) => no
const english = (_no: string, en: string) => en

describe('evaluateAttachment', () => {
  it('lets an image and a document through an ordinary Ask turn', () => {
    expect(evaluateAttachment(png, ask)).toEqual({ ok: true })
    expect(evaluateAttachment(sheet, ask)).toEqual({ ok: true })
  })

  it('a_subscription_turn_refuses_every_attachment', () => {
    const ctx = { ...ask, subscriptionBacked: true }
    expect(evaluateAttachment(png, ctx)).toEqual({ ok: false, reason: 'route_unsupported' })
    expect(evaluateAttachment({ ...png, type: 'image/webp' }, ctx)).toEqual({ ok: false, reason: 'route_unsupported' })
    // A document is read in the browser and travels as message text, which the
    // subscription route does carry: it is not an attachment on that wire.
    expect(evaluateAttachment(sheet, ctx)).toEqual({ ok: true })
  })

  it('a_do_turn_refuses_images', () => {
    expect(evaluateAttachment(png, { ...ask, planMode: true })).toEqual({ ok: false, reason: 'do_images_unsupported' })
    expect(evaluateAttachment(sheet, { ...ask, planMode: true })).toEqual({ ok: true })
  })

  it('a_second_image_is_refused_not_ignored', () => {
    expect(ATTACHMENT_MAX_IMAGES).toBe(1)
    expect(evaluateAttachment(png, { ...ask, currentImageCount: 1 })).toEqual({ ok: false, reason: 'too_many_images' })

    const { accepted, refused } = evaluateAttachments([png, { ...png, name: 'second.png' }], ask)
    expect(accepted.map((file) => file.name)).toEqual(['chart.png'])
    expect(refused).toEqual([{ file: expect.objectContaining({ name: 'second.png' }), reason: 'too_many_images' }])
  })

  it('a_zdr_turn_refuses_attachments_before_send', () => {
    const ctx = { ...ask, zdr: true }
    expect(evaluateAttachment(png, ctx)).toEqual({ ok: false, reason: 'zdr_refused' })
    // Zero retention outranks the other image gates.
    expect(evaluateAttachment(png, { ...ctx, subscriptionBacked: true, planMode: true, currentImageCount: 3 })).toEqual({
      ok: false,
      reason: 'zdr_refused',
    })
  })

  it('an_oversize_file_names_the_file', () => {
    const big = { name: 'annual-report.pdf', size: ATTACHMENT_MAX_FILE_BYTES + 1, type: 'application/pdf' }
    const verdict = evaluateAttachment(big, ask)
    expect(verdict).toEqual({ ok: false, reason: 'too_large' })
    const message = attachmentRefusalMessage({ file: big, reason: 'too_large' }, norwegian)
    expect(message).toContain('«annual-report.pdf»')
    expect(attachmentRefusalMessage({ file: big, reason: 'too_large' }, english)).toContain('"annual-report.pdf"')
  })

  it('refuses a file that pushes the turn over its byte total', () => {
    expect(evaluateAttachment(sheet, { ...ask, currentTotalBytes: 3_145_728 - 39_999 })).toEqual({
      ok: false,
      reason: 'too_large',
    })
  })

  it('an_unknown_extension_is_refused_with_its_name', () => {
    const exe = { name: 'setup.exe', size: 1_000, type: 'application/x-msdownload' }
    expect(evaluateAttachment(exe, ask)).toEqual({ ok: false, reason: 'unsupported_type' })
    expect(attachmentRefusalMessage({ file: exe, reason: 'unsupported_type' }, norwegian)).toContain('«setup.exe»')
    // An image type the providers do not read is refused too, not passed along.
    expect(evaluateAttachment({ name: 'logo.svg', size: 1_000, type: 'image/svg+xml' }, ask)).toEqual({
      ok: false,
      reason: 'unsupported_type',
    })
  })

  it('trusts the extension over an empty or generic MIME type for a document', () => {
    expect(evaluateAttachment({ name: 'notes.MD', size: 100, type: '' }, ask)).toEqual({ ok: true })
  })
})

describe('the picker and the policy', () => {
  it('share one accept list', () => {
    expect(ATTACHMENT_ACCEPT).toContain('image/png')
    expect(ATTACHMENT_ACCEPT).toContain('.xlsx')
    expect(ATTACHMENT_ACCEPT).not.toContain('image/*')
  })

  it('words every reason in both languages and names the file', () => {
    const reasons = [
      'unsupported_type',
      'too_large',
      'route_unsupported',
      'do_images_unsupported',
      'too_many_images',
      'zdr_refused',
    ] as const
    for (const reason of reasons) {
      const no = attachmentRefusalMessage({ file: png, reason }, norwegian)
      const en = attachmentRefusalMessage({ file: png, reason }, english)
      expect(no).toContain('chart.png')
      expect(en).toContain('chart.png')
      expect(no).not.toBe(en)
    }
  })
})

describe('attachments_not_sent', () => {
  it('is worded, not retried, and says nothing was sent', () => {
    const shown = streamFailurePresentation('attachments_not_sent', 'attachments_not_sent', norwegian)
    expect(shown).toMatchObject({ unsaved: true, unavailable: false, action: 'none' })
    expect(shown.message).toContain('Ingenting ble sendt')
    expect(translateApiError({ code: 'attachments_not_sent' }, english)).toContain('Nothing was sent')
  })
})
