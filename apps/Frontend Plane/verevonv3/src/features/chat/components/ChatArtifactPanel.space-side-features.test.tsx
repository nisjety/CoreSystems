// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ComposerCapabilities, ReviewCard } from '@/shared/api/chat-client'
import type { ReviewRunInput } from '@/shared/actions/chat-review'

// A review of a Space thread is not offered while the BYO gateway switch is on: the thread's turns may be written under a
// customer gateway or a subscription, and a review there would run on the platform chain (DEC-V-R8). With the switch off,
// and on any thread that is not a Space thread, the review is offered as before.

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
  chatProjects: false,
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

/**
 * Renders the panel on a chat thread and waits until its artifact viewer is mounted. The review, when it is offered, is
 * drawn inside that viewer, so an absent review is only meaningful once the viewer is on screen.
 */
async function renderPanel(props: { spaceThread?: boolean }): Promise<void> {
  render(() => (
    <ArtifactsPanel
      items={[item()]}
      history={{ threadId: 'thread-1', temporary: false }}
      spaceThread={props.spaceThread}
    />
  ))
  await vi.waitFor(() => {
    flush()
    expect(capabilitiesMock).toHaveBeenCalled()
  })
  await vi.waitFor(() => {
    flush()
    expect(document.querySelector('section.verevon-chat-artifact-view')).not.toBeNull()
  })
}

describe('ArtifactsPanel review on a Space thread', () => {
  it('withholds the review on a Space thread while the BYO gateway switch is on', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    await renderPanel({ spaceThread: true })
    flush()
    expect(screen.queryByRole('button', { name: REVIEW })).toBeNull()
    expect(runReviewMock).not.toHaveBeenCalled()
  })

  it('still offers the review on a Space thread while the switch is off, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    await renderPanel({ spaceThread: true })
    const review = await vi.waitFor(() => {
      flush()
      return screen.getByRole('button', { name: REVIEW })
    })
    fireEvent.click(review)
    await vi.waitFor(() => expect(runReviewMock).toHaveBeenCalledTimes(1))
    expect(runReviewMock.mock.calls[0]?.[0]?.threadId).toBeUndefined()
  })

  it('still offers the review on a thread that is not a Space thread while the switch is on', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    await renderPanel({ spaceThread: false })
    await vi.waitFor(() => {
      flush()
      return screen.getByRole('button', { name: REVIEW })
    })
  })
})
