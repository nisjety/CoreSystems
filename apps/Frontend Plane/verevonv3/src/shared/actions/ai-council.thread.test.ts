import { describe, expect, it } from 'vitest'
import { councilInputAccepted, councilRunInput, parseCouncilInput } from './ai-council'

// The council's thread (W4-BYO, DEC-V-R8). A question asked on a connection thread names the thread; the council's models
// are the ones the request names, as before. The thread is optional: a question that names none is the request it was before.

const COUNCIL = { question: 'Hva er forskjellen?', models: ['model-a', 'model-b'] }

describe('council thread', () => {
  it('keeps a question that names no thread as it was', () => {
    const request = parseCouncilInput(COUNCIL)
    expect(request).toEqual({ question: 'Hva er forskjellen?', models: ['model-a', 'model-b'], allowSubscription: false })
    expect(request).not.toHaveProperty('threadId')
  })

  it('takes the thread a question is asked on, trimmed', () => {
    expect(parseCouncilInput({ ...COUNCIL, threadId: ' thread-1 ' }).threadId).toBe('thread-1')
  })

  it('refuses a blank or malformed thread, and accepts a plain one', () => {
    expect(councilInputAccepted({ ...COUNCIL, threadId: '' })).toBe(false)
    expect(councilInputAccepted({ ...COUNCIL, threadId: 'thread 1' })).toBe(false)
    expect(councilInputAccepted({ ...COUNCIL, threadId: 'a'.repeat(201) })).toBe(false)
    expect(councilInputAccepted({ ...COUNCIL, threadId: 'thread-1' })).toBe(true)
  })

  it('still refuses any other field, so a caller cannot name a posture or a route', () => {
    expect(councilRunInput.safeParse({ ...COUNCIL, model: 'model-c' }).success).toBe(false)
    expect(councilRunInput.safeParse({ ...COUNCIL, zdr: true }).success).toBe(false)
  })
})
