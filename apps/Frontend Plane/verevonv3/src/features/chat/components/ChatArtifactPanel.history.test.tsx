// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ArtifactRevision, ArtifactRevisionPage, ComposerCapabilities } from '@/shared/api/chat-client'

// EP-3 S3: the panel reads the server's revision history for a chat thread when the gateway says it is on.

const capabilitiesMock = vi.hoisted(() => vi.fn<() => Promise<ComposerCapabilities>>())
const revisionsMock = vi.hoisted(() =>
  vi.fn<(threadId: string, artifactId: string, options?: { beforeVersion?: number }) => Promise<ArtifactRevisionPage>>(),
)
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  getComposerCapabilities: () => capabilitiesMock(),
  listArtifactRevisions: (threadId: string, artifactId: string, options?: { beforeVersion?: number }) =>
    revisionsMock(threadId, artifactId, options),
}))

import { ArtifactsPanel } from './ChatArtifactPanel'
import type { ArtifactPanelItem, ChatArtifact, ChatTurn } from './chat-types'

function turn(artifact: ChatArtifact): ChatTurn {
  return {
    attachments: [],
    artifacts: [artifact],
    content: 'Lag et dokument',
    createdAt: '2026-11-04T10:00:00.000Z',
    id: 'turn-3',
    role: 'assistant',
    streaming: false,
    tools: [],
  }
}

const streamed: ChatArtifact = { id: 'report', kind: 'document', title: 'Rapport', content: 'tredje', version: 3 }

function item(artifact: ChatArtifact = streamed): ArtifactPanelItem {
  return { artifact, turn: turn(artifact) }
}

function revision(version: number, editorKind: ArtifactRevision['editorKind'], extra: Partial<ArtifactRevision> = {}): ArtifactRevision {
  return {
    id: `rev-${version}`,
    artifactId: 'report',
    version,
    kind: 'document',
    title: 'Rapport',
    content: `tekst ${version}`,
    editorKind,
    createdAt: '2026-11-04T10:00:00Z',
    ...extra,
  }
}

const ON: ComposerCapabilities = {
  artifactHistory: true,
  artifactEditing: false,
  contextRefs: false,
  threadReferences: false,
  selectionEdit: false,
  artifactPromote: false,
}

beforeEach(() => {
  capabilitiesMock.mockResolvedValue(ON)
  revisionsMock.mockResolvedValue({
    revisions: [revision(3, 'model'), revision(2, 'user'), revision(1, 'model')],
    hasMore: false,
  })
})

afterEach(() => {
  cleanup()
  capabilitiesMock.mockReset()
  revisionsMock.mockReset()
})

/** Let the capability read and the revisions read resolve, then run Solid's queued updates. */
async function settle() {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve()
    flush()
  }
}

describe('ArtifactsPanel server history', () => {
  it('stepper_labels_editor', async () => {
    const { container } = render(() => (
      <ArtifactsPanel items={[item()]} history={{ threadId: 'thread-1', temporary: false }} />
    ))
    await vi.waitFor(() => {
      flush()
      expect(container.textContent).toContain('v3 · Verevon')
    })
    expect(revisionsMock).toHaveBeenCalledWith('thread-1', 'report', {})

    fireEvent.click(screen.getByRole('button', { name: 'Forrige versjon' }))
    flush()
    expect(container.textContent).toContain('v2 · deg')
    expect(container.textContent).toContain('tekst 2')
  })

  it('names a restore with the version it brought back', async () => {
    revisionsMock.mockResolvedValue({
      revisions: [revision(4, 'restore', { restoredFromVersion: 2 }), revision(3, 'model')],
      hasMore: false,
    })
    const { container } = render(() => (
      <ArtifactsPanel items={[item()]} history={{ threadId: 'thread-1', temporary: false }} />
    ))
    await vi.waitFor(() => {
      flush()
      expect(container.textContent).toContain('v4 · gjenopprettet fra v2')
    })
  })

  it('loads earlier versions below the oldest one shown', async () => {
    revisionsMock.mockResolvedValueOnce({ revisions: [revision(3, 'model'), revision(2, 'user')], hasMore: true })
    revisionsMock.mockResolvedValueOnce({ revisions: [revision(1, 'model')], hasMore: false })
    render(() => <ArtifactsPanel items={[item()]} history={{ threadId: 'thread-1', temporary: false }} />)
    const earlier = await vi.waitFor(() => {
      flush()
      return screen.getByRole('button', { name: 'Tidligere versjoner' })
    })
    fireEvent.click(earlier)
    await settle()
    expect(revisionsMock).toHaveBeenLastCalledWith('thread-1', 'report', { beforeVersion: 2 })
    expect(screen.queryByRole('button', { name: 'Tidligere versjoner' })).toBeNull()
  })

  it('room_surface_never_fetches', async () => {
    render(() => (
      <ArtifactsPanel items={[item()]} surface="room" history={{ threadId: 'thread-1', temporary: false }} />
    ))
    await settle()
    expect(capabilitiesMock).not.toHaveBeenCalled()
    expect(revisionsMock).not.toHaveBeenCalled()
  })

  it('temporary_thread_never_fetches', async () => {
    render(() => <ArtifactsPanel items={[item()]} history={{ threadId: 'thread-1', temporary: true }} />)
    await settle()
    expect(capabilitiesMock).not.toHaveBeenCalled()
    expect(revisionsMock).not.toHaveBeenCalled()
  })

  it('reads nothing while the gateway says history is off, and keeps the old label', async () => {
    capabilitiesMock.mockResolvedValue({ ...ON, artifactHistory: false })
    const two = { ...streamed, version: 2, history: [{ content: 'en', title: 'Rapport', version: 1 }] }
    const { container } = render(() => (
      <ArtifactsPanel items={[item(two)]} history={{ threadId: 'thread-1', temporary: false }} />
    ))
    await settle()
    expect(capabilitiesMock).toHaveBeenCalledTimes(1)
    expect(revisionsMock).not.toHaveBeenCalled()
    expect(container.textContent).toContain('v2 · 2/2')
  })

  it('never fetches without a thread, as every existing mount is', async () => {
    render(() => <ArtifactsPanel items={[item()]} />)
    await settle()
    expect(capabilitiesMock).not.toHaveBeenCalled()
  })

  it('a failed history read keeps the streamed revision on screen', async () => {
    revisionsMock.mockRejectedValue(new Error('artifact_history_unavailable'))
    const { container } = render(() => (
      <ArtifactsPanel items={[item()]} history={{ threadId: 'thread-1', temporary: false }} />
    ))
    await settle()
    expect(revisionsMock).toHaveBeenCalled()
    expect(container.textContent).toContain('tredje')
  })
})
