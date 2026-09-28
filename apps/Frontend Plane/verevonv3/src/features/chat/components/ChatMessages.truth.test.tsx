// @vitest-environment jsdom

/**
 * The chat never claims more than the server did.
 *
 *  - Answer text that streamed before a failure the server saved nothing for
 *    is shown only behind an explicit "not saved" disclosure.
 *  - A model service that did not answer reads as an outage in the Work
 *    panel, not as a failed check.
 *  - "Rediger" on a memory that is not the caller's (session-core now answers
 *    404 for those) saves the correction but does not say the original was
 *    replaced; "Glem" does not drop a row the server did not delete.
 */

import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { RecalledMemory } from '@/shared/api/chat-client'
import { ApiError } from '@/shared/api/http'
import * as memoryClient from '@/shared/api/memory-client'
import { AnswerRegion, MemoryRecallNotice } from './ChatMessages'
import { StepsPanel } from './ChatPanels'

vi.mock('@/shared/api/memory-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/memory-client')>()
  return {
    ...actual,
    deleteMemory: vi.fn(async () => ({ deleted: true, degraded: false, degradationReason: '' })),
    correctMemory: vi.fn(async () => ({
      memoryId: 'mem-1-corrected',
      deleted: false,
      original: 'not_found',
      degraded: false,
      degradationReason: '',
    })),
    listMemories: vi.fn(async () => ({
      memories: [{
        memoryId: 'mem-1',
        topic: 'USER',
        content: 'Foretrekker metriske enheter',
        updatedAt: '2026-01-01T00:00:00.000Z',
        provenance: 'stated',
      }],
      degraded: false,
      degradationReason: '',
    })),
  }
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('an answer the server did not save', () => {
  it('shows the explanation, with the streamed text only behind a closed "not saved" disclosure', () => {
    const { container, unmount } = render(() => (
      <AnswerRegion
        answer={{ state: 'failed', message: 'Svaret ble ikke lagret.', unsaved: 'Hele svaret er her.' }}
        onRegenerate={() => {}}
      />
    ))
    const notice = container.querySelector('.verevon-chat-error-notice')
    expect(notice?.textContent).toContain('Svaret ble ikke lagret.')
    const disclosure = container.querySelector<HTMLDetailsElement>('details.verevon-chat-error-notice__unsaved')
    expect(disclosure).not.toBeNull()
    expect(disclosure?.open).toBe(false)
    expect(disclosure?.querySelector('summary')?.textContent).toMatch(/ikke ble lagret|was not saved/)
    expect(disclosure?.textContent).toContain('Hele svaret er her.')
    // Never rendered as an answer.
    expect(container.querySelector('.verevon-chat-streaming')).toBeNull()
    unmount()
  })

  it('shows no disclosure for an ordinary failure', () => {
    const { container, unmount } = render(() => (
      <AnswerRegion answer={{ state: 'failed', message: 'Utkastet er ikke publisert.' }} onRegenerate={() => {}} />
    ))
    expect(container.querySelector('details')).toBeNull()
    unmount()
  })
})

describe('StepsPanel when the model service did not answer', () => {
  it('names the service, not the check it interrupted', () => {
    const { container, unmount } = render(() => (
      <StepsPanel
        turnStatus="error"
        steps={[
          { id: 'a1:event-draft-1:source-check', title: 'Kontrollerer utkastet mot kildene', detail: 'Svarte ikke.', status: 'stopped', createdAt: '2026-09-24T08:00:00Z', turnId: 'a1' },
          { id: 'a1:service-unavailable', title: 'Modelltjenesten svarte ikke', detail: 'Svarte ikke.', status: 'stopped', createdAt: '2026-09-24T08:00:01Z', turnId: 'a1' },
        ]}
        onStopTask={() => {}}
      />
    ))
    const line = container.querySelector('.verevon-chat-steps-header p')?.textContent ?? ''
    expect(line).toBe('Modelltjenesten svarte ikke. Ingenting ble publisert.')
    expect(line).not.toContain('Feilet')
    unmount()
  })

  it('still names a failed step when the failure came after', () => {
    const { container, unmount } = render(() => (
      <StepsPanel
        turnStatus="error"
        steps={[
          { id: 'a1:service-unavailable', title: 'Modelltjenesten svarte ikke', detail: '', status: 'stopped', createdAt: '2026-09-24T08:00:00Z', turnId: 'a1' },
          { id: 'a2:event-check', title: 'Kildegjennomgang', detail: 'Ikke publisert.', status: 'error', createdAt: '2026-09-24T08:01:00Z', turnId: 'a2' },
        ]}
        onStopTask={() => {}}
      />
    ))
    expect(container.querySelector('.verevon-chat-steps-header p')?.textContent).toContain('Feilet')
    unmount()
  })
})

describe('MemoryRecallNotice tells the truth about the original', () => {
  const memory: RecalledMemory = {
    memoryId: 'mem-1',
    role: 'recall',
    origin: 'stated',
    label: 'USER',
    preview: 'Foretrekker metriske enheter',
  }

  const openList = (container: HTMLElement) => {
    fireEvent.click(container.querySelector('.verevon-chat-memory-recall__toggle')!)
    flush()
  }
  const actionButton = (container: HTMLElement, label: string) =>
    Array.from(container.querySelectorAll('.verevon-chat-memory-recall__action'))
      .find((el) => el.textContent?.includes(label)) as HTMLButtonElement | undefined

  async function correct(container: HTMLElement, content: string) {
    fireEvent.click(actionButton(container, 'Rediger')!)
    flush()
    const textarea = await waitFor(() => {
      const el = container.querySelector<HTMLTextAreaElement>('.verevon-chat-memory-recall__edit-textarea')
      expect(el?.value).toBe('Foretrekker metriske enheter')
      return el!
    })
    fireEvent.input(textarea, { target: { value: content } })
    flush()
    fireEvent.click(actionButton(container, 'Lagre')!)
  }

  it('does not claim the original was replaced when it is not the caller\'s to remove', async () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} threadId="thread-1" memories={[memory]} />
    ))
    openList(container)
    await correct(container, 'Foretrekker metriske enheter, alltid')

    await waitFor(() => expect(container.querySelector('.verevon-chat-memory-recall__notice')).not.toBeNull())
    const notice = container.querySelector('.verevon-chat-memory-recall__notice')?.textContent ?? ''
    expect(notice).toContain('Rettelsen er lagret')
    expect(notice).toContain('ble ikke fjernet')
    // The row is still the original, under its own id: it can still be recalled.
    const preview = container.querySelector('.verevon-chat-memory-recall__preview')?.textContent
    expect(preview).toBe('Foretrekker metriske enheter')
    const forget = actionButton(container, 'Glem')!
    fireEvent.click(forget)
    flush()
    fireEvent.click(forget)
    await waitFor(() => expect(memoryClient.deleteMemory).toHaveBeenCalledWith('mem-1'))
    unmount()
  })

  it('says the original could not be removed when the delete failed', async () => {
    vi.mocked(memoryClient.correctMemory).mockResolvedValueOnce({
      memoryId: 'mem-1-corrected',
      deleted: false,
      original: 'not_removed',
      degraded: false,
      degradationReason: '',
    })
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} threadId="thread-1" memories={[memory]} />
    ))
    openList(container)
    await correct(container, 'Ny tekst')
    await waitFor(() =>
      expect(container.querySelector('.verevon-chat-memory-recall__notice')?.textContent).toContain('kunne ikke fjernes'),
    )
    expect(container.querySelector('.verevon-chat-memory-recall__preview')?.textContent).toBe('Foretrekker metriske enheter')
    unmount()
  })

  it('keeps the row when "Glem" was not carried out', async () => {
    vi.mocked(memoryClient.deleteMemory).mockResolvedValueOnce({ deleted: false, degraded: true, degradationReason: 'zdr' })
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} threadId="thread-1" memories={[memory]} />
    ))
    openList(container)
    const forget = actionButton(container, 'Glem')!
    fireEvent.click(forget)
    flush()
    fireEvent.click(forget)
    await waitFor(() => expect(container.querySelector('.verevon-chat-memory-recall__error')).not.toBeNull())
    expect(container.querySelector('.verevon-chat-memory-recall__error')?.textContent).toContain('ble ikke glemt')
    expect(container.textContent).toContain('Foretrekker metriske enheter')
    unmount()
  })

  it('explains a 404 on "Glem" instead of a generic failure', async () => {
    vi.mocked(memoryClient.deleteMemory).mockRejectedValueOnce(new ApiError('memory not found', 404, 'memory_not_found'))
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} threadId="thread-1" memories={[memory]} />
    ))
    openList(container)
    const forget = actionButton(container, 'Glem')!
    fireEvent.click(forget)
    flush()
    fireEvent.click(forget)
    await waitFor(() =>
      expect(container.querySelector('.verevon-chat-memory-recall__error')?.textContent).toContain('ikke ditt eget'),
    )
    expect(container.textContent).toContain('Foretrekker metriske enheter')
    unmount()
  })
})
