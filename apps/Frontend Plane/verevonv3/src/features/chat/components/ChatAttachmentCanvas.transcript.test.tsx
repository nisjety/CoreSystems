import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/api-error'
import { REFUSAL_TEXT } from '@/shared/api/zdr-refusals'
import { ChatAttachmentCanvas } from './ChatAttachmentCanvas'
import type { ChatTurnAttachment } from './chat-types'

// A recording whose transcript was not saved. The note under it says why. The retryable zero-retention outage (the
// organisation's settings could not be read, so nothing was sent) reads the catalogue's own words, and does not say that
// Zero Data Retention applies to the recording, which is what a refusal of the recording says.

const { transcribeMock } = vi.hoisted(() => ({ transcribeMock: vi.fn() }))
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  mediaTranscriptsEnabled: () => true,
  transcribeToDocument: transcribeMock,
}))
vi.mock('@/shared/i18n', () => ({
  useI18n: () => ({
    tr: (_noText: string, enText: string) => enText,
    locale: () => 'en',
  }),
}))

const recording: ChatTurnAttachment = {
  id: 'att-1',
  name: 'meeting.webm',
  size: 4,
  type: 'audio/webm',
  url: 'blob:meeting',
  previewUrl: 'data:audio/webm;base64,AAAA',
}

afterEach(() => {
  cleanup()
  transcribeMock.mockReset()
})

/** Renders the recording, asks for its transcript, and returns the note that the failure leaves under it. */
async function transcriptNoteAfter(error: unknown): Promise<string | null> {
  transcribeMock.mockRejectedValue(error)
  render(() => <ChatAttachmentCanvas attachments={[recording]} transcriptThreadId="thread-1" />)
  fireEvent.click(screen.getByRole('button', { name: 'Transcribe to document' }))
  const note = await screen.findByRole('alert')
  return note.textContent
}

describe('the note under a recording whose transcript was not saved', () => {
  it('reads the retryable zero-retention outage in the catalogue words, not as a refusal of the recording', async () => {
    const note = await transcriptNoteAfter(new ApiError('Server text', 503, 'zdr_decision_unavailable'))

    expect(note).toBe(REFUSAL_TEXT.zdr_decision_unavailable.en)
    expect(note).not.toMatch(/applies/)
  })

  it('still says Zero Data Retention applies for a refusal of the recording itself', async () => {
    const note = await transcriptNoteAfter(new ApiError('Server text', 412, 'zdr_durable_mutation_forbidden'))

    expect(note).toBe('Zero Data Retention applies, so this recording cannot be saved as a document.')
  })

  it('keeps the retry wording for a failure the route did not type', async () => {
    const note = await transcriptNoteAfter(new ApiError('Server text', 500, null))

    expect(note).toBe('The transcript could not be saved. Try again.')
  })
})
