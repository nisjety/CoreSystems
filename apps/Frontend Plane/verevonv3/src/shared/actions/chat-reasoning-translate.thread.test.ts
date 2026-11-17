import { describe, expect, it } from 'vitest'
import { parseReasoningTranslateInput, reasoningTranslateInput } from './chat-reasoning-translate'

// The translation's thread and model (W4-BYO, DEC-V-R8). A translation of a connection thread's text names the thread and
// the model that connection serves. Both are optional: a translation that names neither is the request it was before.

const TRANSLATE = { text: 'Jeg vurderer to alternativer.', targetLocale: 'en' } as const

describe('reasoning translation thread and model', () => {
  it('keeps a translation that names no thread as it was: the text and the locale alone', () => {
    const request = parseReasoningTranslateInput({ ...TRANSLATE })
    expect(request).toEqual({ text: 'Jeg vurderer to alternativer.', targetLocale: 'en' })
    expect(request).not.toHaveProperty('threadId')
    expect(request).not.toHaveProperty('model')
  })

  it('takes the thread and the model a connection thread names, trimmed', () => {
    const request = parseReasoningTranslateInput({ ...TRANSLATE, threadId: ' thread-1 ', model: '  gpt-5-codex ' })
    expect(request.threadId).toBe('thread-1')
    expect(request.model).toBe('gpt-5-codex')
  })

  it('refuses a blank thread or model: a caller that names none leaves the key out', () => {
    expect(() => reasoningTranslateInput.parse({ ...TRANSLATE, threadId: '' })).toThrow()
    expect(() => reasoningTranslateInput.parse({ ...TRANSLATE, model: '   ' })).toThrow()
  })

  it('refuses an identifier with a space or a control character in it, or one over 200 characters', () => {
    expect(() => reasoningTranslateInput.parse({ ...TRANSLATE, threadId: 'thread 1' })).toThrow()
    expect(() => reasoningTranslateInput.parse({ ...TRANSLATE, model: 'gpt\u00075' })).toThrow()
    expect(() => reasoningTranslateInput.parse({ ...TRANSLATE, threadId: 'a'.repeat(201) })).toThrow()
  })

  it('still refuses any other field, so a caller cannot name a posture, a model it may not use or a switch', () => {
    expect(() => reasoningTranslateInput.parse({ ...TRANSLATE, zdr: true })).toThrow()
    expect(() => reasoningTranslateInput.parse({ ...TRANSLATE, connection: 'conn-a' })).toThrow()
  })
})
