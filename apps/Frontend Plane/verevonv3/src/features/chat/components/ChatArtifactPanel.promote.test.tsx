// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatDocumentUpload, ComposerCapabilities } from '@/shared/api/chat-client'

// EP-3 S8: a document artifact on the chat surface can be saved to knowledge when the BFF says promote is on.

type UploadOptions = { origin?: string; sourceRef?: { threadId: string; artifactId: string; version: number } }

const capabilitiesMock = vi.hoisted(() => vi.fn<() => Promise<ComposerCapabilities>>())
const uploadMock = vi.hoisted(() =>
  vi.fn<(title: string, content: string, options?: UploadOptions) => Promise<ChatDocumentUpload>>(),
)
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  getComposerCapabilities: () => capabilitiesMock(),
  uploadChatDocument: (title: string, content: string, options?: UploadOptions) => uploadMock(title, content, options),
}))

import { ArtifactsPanel } from './ChatArtifactPanel'
import type { ArtifactPanelItem, ChatArtifact, ChatTurn } from './chat-types'

const SAVE = 'Lagre kopi i kunnskapsbasen'

function turn(artifact: ChatArtifact): ChatTurn {
  return {
    attachments: [],
    artifacts: [artifact],
    content: 'Lag et dokument',
    createdAt: '2026-11-04T10:00:00.000Z',
    id: 'turn-2',
    role: 'assistant',
    streaming: false,
    tools: [],
  }
}

const report: ChatArtifact = {
  id: 'report',
  kind: 'document',
  title: 'Rapport',
  content: 'andre utkast',
  version: 2,
  history: [{ content: 'første utkast', title: 'Rapport', version: 1 }],
}

function item(artifact: ChatArtifact = report): ArtifactPanelItem {
  return { artifact, turn: turn(artifact) }
}

const PROMOTE_ON: ComposerCapabilities = {
  artifactHistory: false,
  artifactEditing: false,
  contextRefs: false,
  threadReferences: false,
  selectionEdit: false,
  artifactPromote: true,
  deepResearchKnowledge: false,
  wideResearch: false,
}

beforeEach(() => {
  capabilitiesMock.mockResolvedValue(PROMOTE_ON)
  uploadMock.mockResolvedValue({ documentId: 'doc-9', status: 'queued' })
})

afterEach(() => {
  cleanup()
  capabilitiesMock.mockReset()
  uploadMock.mockReset()
})

/** Let the capability read and the upload resolve, then run Solid's queued updates. */
async function settle() {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve()
    flush()
  }
}

describe('ArtifactsPanel save to knowledge', () => {
  it('save_to_knowledge_sends_revision_on_screen', async () => {
    const { container } = render(() => (
      <ArtifactsPanel items={[item()]} history={{ threadId: 'thread-1', temporary: false }} />
    ))
    await vi.waitFor(() => {
      flush()
      expect(screen.getByRole('button', { name: SAVE })).toBeTruthy()
    })

    // Step back to v1: the copy is the revision the person is reading, not the newest one.
    fireEvent.click(screen.getByRole('button', { name: 'Forrige versjon' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: SAVE }))
    await settle()

    expect(uploadMock).toHaveBeenCalledTimes(1)
    expect(uploadMock).toHaveBeenCalledWith('Rapport', 'første utkast', {
      origin: 'chat-artifact',
      sourceRef: { threadId: 'thread-1', artifactId: 'report', version: 1 },
    })
    // The confirmation says the copy outlives the chat, and links it.
    expect(container.textContent).toContain('blir værende hvis samtalen slettes')
    expect(screen.getByRole('link', { name: 'Åpne kopien' }).getAttribute('href')).toBe('/knowledge?source=doc-9')
  })

  it('a refused save says so and links nothing', async () => {
    uploadMock.mockRejectedValue(new Error('zdr_durable_mutation_forbidden'))
    render(() => <ArtifactsPanel items={[item()]} history={{ threadId: 'thread-1', temporary: false }} />)
    const save = await vi.waitFor(() => {
      flush()
      return screen.getByRole('button', { name: SAVE })
    })
    fireEvent.click(save)
    await settle()
    expect(screen.getByRole('alert').textContent).toContain('Kunne ikke lagre kopien')
    expect(screen.queryByRole('link', { name: 'Åpne kopien' })).toBeNull()
  })

  it('hidden_for_non_document_kinds', async () => {
    const code: ChatArtifact = { id: 'script', kind: 'code', title: 'script.ts', content: 'const a = 1', version: 1 }
    const html: ChatArtifact = { id: 'page', kind: 'html', title: 'Side', content: '<p>hei</p>', version: 1 }
    for (const artifact of [code, html]) {
      render(() => <ArtifactsPanel items={[item(artifact)]} history={{ threadId: 'thread-1', temporary: false }} />)
      await settle()
      expect(capabilitiesMock).toHaveBeenCalled()
      expect(screen.queryByRole('button', { name: SAVE })).toBeNull()
      cleanup()
    }
  })

  it('hidden while the BFF says promote is off', async () => {
    capabilitiesMock.mockResolvedValue({ ...PROMOTE_ON, artifactPromote: false })
    render(() => <ArtifactsPanel items={[item()]} history={{ threadId: 'thread-1', temporary: false }} />)
    await settle()
    expect(capabilitiesMock).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('button', { name: SAVE })).toBeNull()
  })

  it('a temporary thread and a room never offer it', async () => {
    render(() => <ArtifactsPanel items={[item()]} history={{ threadId: 'thread-1', temporary: true }} />)
    await settle()
    expect(screen.queryByRole('button', { name: SAVE })).toBeNull()
    cleanup()

    render(() => <ArtifactsPanel items={[item()]} surface="room" history={{ threadId: 'thread-1', temporary: false }} />)
    await settle()
    expect(screen.queryByRole('button', { name: SAVE })).toBeNull()
    expect(capabilitiesMock).not.toHaveBeenCalled()
    expect(uploadMock).not.toHaveBeenCalled()
  })
})
