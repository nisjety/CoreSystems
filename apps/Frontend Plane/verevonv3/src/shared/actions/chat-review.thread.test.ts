import { describe, expect, it } from 'vitest'
import { parseReviewRunInput, reviewRunInput } from './chat-review'

// The review's thread and model (W4-BYO, DEC-V-R8). A review of a connection thread names the thread and the model that
// connection serves. Both are optional: a review that names neither is the request it was before the feature named one.

const REVIEW = {
  target: 'artifact_revision',
  label: 'Tilbud v2',
  material: 'Prisen er 900 kr.',
  brief: '',
  locale: 'no',
} as const

describe('review.run thread and model', () => {
  it('keeps a review that names no thread as it was: the review fields alone', () => {
    const request = parseReviewRunInput({ ...REVIEW })
    expect(request).toEqual({ target: 'artifact_revision', label: 'Tilbud v2', material: 'Prisen er 900 kr.', brief: '', locale: 'no' })
    expect(request).not.toHaveProperty('threadId')
    expect(request).not.toHaveProperty('model')
  })

  it('takes the thread and the model a connection thread names, trimmed', () => {
    const request = parseReviewRunInput({ ...REVIEW, threadId: ' thread-1 ', model: '  gpt-5-codex ' })
    expect(request.threadId).toBe('thread-1')
    expect(request.model).toBe('gpt-5-codex')
  })

  it('refuses a blank thread or model: a caller that names none leaves the key out', () => {
    expect(() => reviewRunInput.parse({ ...REVIEW, threadId: '' })).toThrow()
    expect(() => reviewRunInput.parse({ ...REVIEW, threadId: '   ' })).toThrow()
    expect(() => reviewRunInput.parse({ ...REVIEW, model: '' })).toThrow()
  })

  it('refuses an identifier with a space or a control character in it', () => {
    expect(() => reviewRunInput.parse({ ...REVIEW, threadId: 'thread 1' })).toThrow()
    expect(() => reviewRunInput.parse({ ...REVIEW, threadId: 'thread-1\nx' })).toThrow()
    expect(() => reviewRunInput.parse({ ...REVIEW, model: 'gpt 5' })).toThrow()
    expect(() => reviewRunInput.parse({ ...REVIEW, model: 'gpt\u00075' })).toThrow()
  })

  it('refuses an identifier longer than 200 characters', () => {
    expect(() => reviewRunInput.parse({ ...REVIEW, threadId: 'a'.repeat(201) })).toThrow()
    expect(() => reviewRunInput.parse({ ...REVIEW, model: 'm'.repeat(201) })).toThrow()
    expect(reviewRunInput.parse({ ...REVIEW, threadId: 'a'.repeat(200) }).threadId).toHaveLength(200)
  })

  it('still refuses any other field, so a caller cannot name a connection or a switch', () => {
    expect(() => reviewRunInput.parse({ ...REVIEW, connection: 'conn-a' })).toThrow()
    expect(() => reviewRunInput.parse({ ...REVIEW, zdr: false })).toThrow()
  })
})
