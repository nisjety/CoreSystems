import { describe, expect, it } from 'vitest'
import { streamFailurePresentation } from './chat-normalizers'

// The model gateway refuses a deep-research turn when its thread already runs a research run (Q4 E4.6 062b).
// The refusal comes before any search or model, so no answer is saved, and the reader is told to try again once
// the running run has finished. The server's English text names that run and is never shown.

const en = (_norwegian: string, english: string) => english
const no = (norwegian: string) => norwegian

const CODE = 'research_run_active'

const SERVER_TEXT =
  'This thread already has a research run in progress (research-run-1). Try again when it finishes.'

describe('streamFailurePresentation for the research run refusal', () => {
  it('is a refusal with a retry, saved nowhere, in both languages', () => {
    for (const tr of [no, en]) {
      expect(streamFailurePresentation(CODE, SERVER_TEXT, tr)).toMatchObject({
        unsaved: true,
        unavailable: false,
        action: 'retry',
      })
    }
  })

  it('says to try again once the run has finished and never shows the server text', () => {
    const norwegian = streamFailurePresentation(CODE, SERVER_TEXT, no).message
    const english = streamFailurePresentation(CODE, SERVER_TEXT, en).message
    expect(norwegian).toContain('Prøv igjen når den er ferdig.')
    expect(english).toContain('Try again when it finishes.')
    expect(norwegian).not.toBe(english)
    expect(`${norwegian} ${english}`).not.toContain('research-run-1')
    expect(english).not.toBe(SERVER_TEXT)
  })
})
