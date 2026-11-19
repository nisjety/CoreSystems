// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ComposerCapabilities, ReviewCard } from '@/shared/api/chat-client'
import { ApiError } from '@/shared/api/api-error'
import type { ReviewRunInput } from '@/shared/actions/chat-review'
import { reviewRunFailureKind } from './ReviewRunCard'

// A review of a chat thread names the thread and the model its connection serves (DEC-V-R8). The panel passes both to the
// review it starts, and a review that names no thread is sent as it was before the feature named one. A typed refusal of the
// thread is shown as its own reason, with no retry, and is not a failed review.

const runReviewMock = vi.hoisted(() => vi.fn<(input: ReviewRunInput) => Promise<ReviewCard | null>>())
const capabilitiesMock = vi.hoisted(() => vi.fn<() => Promise<ComposerCapabilities>>())

vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  getComposerCapabilities: () => capabilitiesMock(),
  runReview: (input: ReviewRunInput) => runReviewMock(input),
}))

import { ArtifactsPanel } from './ChatArtifactPanel'
import type { ArtifactPanelItem, ChatArtifact, ChatTurn } from './chat-types'

const REVIEW = 'Gjennomgå'

const CAPABILITIES: ComposerCapabilities = {
  artifactHistory: false,
  artifactEditing: false,
  contextRefs: false,
  threadReferences: false,
  selectionEdit: false,
  artifactPromote: false,
  deepResearchKnowledge: false,
  wideResearch: false,
}

const CARD: ReviewCard = {
  runId: 'run-1',
  target: 'artifact_revision',
  label: 'Tilbud v2',
  status: 'completed',
  verdict: 'clean',
  findings: [],
  sourceChecks: [],
  limitReached: false,
  reviewed: { contentHash: 'hash', lines: 1, bytes: 17 },
  modelUsed: 'gpt-5-codex',
  usage: { inputTokens: 1, outputTokens: 1, costUsd: null, latencyMs: 1 },
}

const offer: ChatArtifact = {
  id: 'offer',
  kind: 'document',
  title: 'Tilbud',
  content: 'Prisen er 900 kr.',
  version: 2,
}

function turn(artifact: ChatArtifact): ChatTurn {
  return {
    attachments: [],
    artifacts: [artifact],
    content: 'Lag et tilbud',
    createdAt: '2026-11-17T10:00:00.000Z',
    id: 'turn-2',
    role: 'assistant',
    streaming: false,
    tools: [],
  }
}

function item(artifact: ChatArtifact = offer): ArtifactPanelItem {
  return { artifact, turn: turn(artifact) }
}

beforeEach(() => {
  vi.stubEnv('VITE_REVIEW_RUN', 'true')
  capabilitiesMock.mockResolvedValue(CAPABILITIES)
  runReviewMock.mockResolvedValue(CARD)
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  capabilitiesMock.mockReset()
  runReviewMock.mockReset()
})

/** Renders the panel, starts a review of the text on screen, and returns the input the panel sent. */
async function startReview(props: { reviewThreadId?: string; connectionModel?: string | null }): Promise<ReviewRunInput> {
  render(() => (
    <ArtifactsPanel
      items={[item()]}
      history={{ threadId: 'thread-1', temporary: false }}
      reviewThreadId={props.reviewThreadId}
      connectionModel={props.connectionModel}
    />
  ))
  const review = await vi.waitFor(() => {
    flush()
    return screen.getByRole('button', { name: REVIEW })
  })
  fireEvent.click(review)
  await vi.waitFor(() => expect(runReviewMock).toHaveBeenCalledTimes(1))
  const input = runReviewMock.mock.calls[0]?.[0]
  if (!input) throw new Error('the review was not started')
  return input
}

describe('ArtifactsPanel review thread', () => {
  it('names the chat thread and the connection model its review runs on', async () => {
    const input = await startReview({ reviewThreadId: 'thread-1', connectionModel: 'gpt-5-codex' })
    expect(input).toMatchObject({ target: 'artifact_revision', threadId: 'thread-1', model: 'gpt-5-codex' })
  })

  it('names no thread and no model when the chat names none, so the review runs as before', async () => {
    const input = await startReview({ reviewThreadId: undefined, connectionModel: null })
    expect(input.threadId).toBeUndefined()
    expect(input.model).toBeUndefined()
    expect(input.target).toBe('artifact_revision')
  })

  it('shows the gateway refusal of the thread as its own reason, with no retry, and not as a failed review', async () => {
    runReviewMock.mockRejectedValueOnce(
      new ApiError('A review is not available for this thread.', 409, 'side_chat_thread_retention_refused'),
    )
    render(() => (
      <ArtifactsPanel
        items={[item()]}
        history={{ threadId: 'thread-1', temporary: false }}
        reviewThreadId="thread-1"
        connectionModel="gpt-5-codex"
      />
    ))
    const review = await vi.waitFor(() => {
      flush()
      return screen.getByRole('button', { name: REVIEW })
    })
    fireEvent.click(review)
    await vi.waitFor(() => {
      flush()
      expect(screen.getByText(/ikke tilgjengelig for denne samtalen/)).toBeTruthy()
    })
    expect(screen.queryByText(/Kunne ikke gjennomgå/)).toBeNull()
    expect(runReviewMock).toHaveBeenCalledTimes(1)
  })

  it('disables Review once the gateway has refused the thread, so the same refused request is not sent again', async () => {
    runReviewMock.mockRejectedValueOnce(
      new ApiError('A review is not available for this thread.', 409, 'side_chat_thread_retention_refused'),
    )
    render(() => (
      <ArtifactsPanel
        items={[item()]}
        history={{ threadId: 'thread-1', temporary: false }}
        reviewThreadId="thread-1"
        connectionModel="gpt-5-codex"
      />
    ))
    fireEvent.click(await vi.waitFor(() => {
      flush()
      return screen.getByRole('button', { name: REVIEW })
    }))
    await vi.waitFor(() => {
      flush()
      expect(screen.getByText(/ikke tilgjengelig for denne samtalen/)).toBeTruthy()
    })
    const review = screen.getByRole('button', { name: REVIEW }) as HTMLButtonElement
    expect(review.disabled).toBe(true)
    fireEvent.click(review)
    flush()
    expect(runReviewMock).toHaveBeenCalledTimes(1)
  })

  it('leaves Review enabled after a failure that is not a refusal of the thread, so the review can be tried again', async () => {
    runReviewMock.mockRejectedValueOnce(new ApiError('Busy.', 503, 'review_unavailable'))
    render(() => (
      <ArtifactsPanel
        items={[item()]}
        history={{ threadId: 'thread-1', temporary: false }}
        reviewThreadId="thread-1"
        connectionModel="gpt-5-codex"
      />
    ))
    fireEvent.click(await vi.waitFor(() => {
      flush()
      return screen.getByRole('button', { name: REVIEW })
    }))
    await vi.waitFor(() => {
      flush()
      expect(screen.getByText(/ikke tilgjengelig akkurat nå/)).toBeTruthy()
    })
    expect((screen.getByRole('button', { name: REVIEW }) as HTMLButtonElement).disabled).toBe(false)
  })
})

describe('reviewRunFailureKind on a thread', () => {
  it('types each refusal of the named thread, and leaves an untyped conflict as a failed review', () => {
    for (const code of [
      'scope_not_available_in_space',
      'side_chat_thread_retention_refused',
      'connection_ambiguous',
      'side_chat_model_required',
    ]) {
      expect(reviewRunFailureKind(new ApiError('m', code === 'side_chat_model_required' ? 400 : 409, code))).toBe(
        'thread_refused',
      )
    }
    expect(reviewRunFailureKind(new ApiError('m', 409, 'review_unavailable'))).toBe('failed')
  })
})
