import { describe, expect, it } from 'vitest'
import { streamFailurePresentation } from './chat-normalizers'

// The two codes the model gateway's browser page context (QF-10, `browser_page_context`) refuses a turn
// with, before any run or model: the reader gets words in their language, nothing is saved, no retry is
// offered, and the server's English operator text is never shown.

const en = (_norwegian: string, english: string) => english
const no = (norwegian: string) => norwegian

const CODES = ['browser_page_context_unsupported', 'browser_page_context_invalid'] as const

const SERVER_TEXT: Record<(typeof CODES)[number], string> = {
  browser_page_context_unsupported:
    'The picked model cannot read images, so the attached browser regions cannot be used. Pick a model that reads images, or remove the regions.',
  browser_page_context_invalid:
    'The attached browser tabs or regions could not be read. Remove them and attach them again.',
}

describe('streamFailurePresentation for the browser page context refusals', () => {
  it.each(CODES)('%s is a refusal with no retry, saved nowhere, in both languages', (code) => {
    for (const tr of [no, en]) {
      expect(streamFailurePresentation(code, SERVER_TEXT[code], tr)).toMatchObject({
        unsaved: true,
        unavailable: false,
        action: 'none',
      })
    }
  })

  it.each(CODES)('%s says the draft is kept and never shows the server text', (code) => {
    const norwegian = streamFailurePresentation(code, SERVER_TEXT[code], no).message
    const english = streamFailurePresentation(code, SERVER_TEXT[code], en).message
    expect(norwegian).toContain('Utkastet er beholdt.')
    expect(english).toContain('Your draft is kept.')
    expect(norwegian).not.toBe(english)
    expect(`${norwegian} ${english}`).not.toContain(SERVER_TEXT[code])
  })

  it('says something different for each code', () => {
    const messages = new Set(CODES.map((code) => streamFailurePresentation(code, '', en).message))
    expect(messages.size).toBe(CODES.length)
  })
})
