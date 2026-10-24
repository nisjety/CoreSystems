import { describe, expect, it } from 'vitest'
import { streamFailurePresentation } from './chat-normalizers'

// The five codes of the server's attachment contract (model-gateway
// `attachment_policy`): each is a refusal made before any model ran, so the
// reader gets words in their language, no retry, and never the server's English.

const en = (_norwegian: string, english: string) => english
const no = (norwegian: string) => norwegian
const SERVER_TEXT = "'sales.png': it is 2000000 bytes; the limit for one image is 1000000 bytes."

const CODES = [
  'attachment_media_unsupported',
  'attachment_too_large',
  'attachment_count_exceeded',
  'attachment_route_unsupported',
  'attachment_encoding_invalid',
] as const

describe('streamFailurePresentation for the attachment contract', () => {
  it.each(CODES)('%s is a refusal with no retry, saved nowhere, in both languages', (code) => {
    for (const tr of [no, en]) {
      expect(streamFailurePresentation(code, SERVER_TEXT, tr)).toMatchObject({
        unsaved: true,
        unavailable: false,
        action: 'none',
      })
    }
    const norwegian = streamFailurePresentation(code, SERVER_TEXT, no).message
    const english = streamFailurePresentation(code, SERVER_TEXT, en).message
    expect(norwegian).not.toBe(english)
    expect(english).toContain('Nothing was sent')
    expect(norwegian).toContain('Ingenting ble sendt')
    expect(`${norwegian} ${english}`).not.toContain('bytes')
  })

  it('says something different for each code', () => {
    const messages = new Set(CODES.map((code) => streamFailurePresentation(code, '', en).message))
    expect(messages.size).toBe(CODES.length)
  })

  it('names the subscription for the route refusal', () => {
    expect(streamFailurePresentation('attachment_route_unsupported', '', en).message).toContain('ChatGPT subscription')
  })
})
