// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import * as chatClient from '@/shared/api/chat-client'
import { ArtifactsPanel, type ArtifactEditing } from './ChatArtifactPanel'
import { mergeArtifactVersion } from './chat-artifacts'
import type { ArtifactPanelItem, ChatArtifact, ChatTurn } from './chat-types'

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, saveArtifactRevision: vi.fn(), restoreArtifactRevision: vi.fn() }
})

afterEach(() => {
  cleanup()
  vi.mocked(chatClient.saveArtifactRevision).mockReset()
  vi.mocked(chatClient.restoreArtifactRevision).mockReset()
})

function turn(artifact: ChatArtifact): ChatTurn {
  return {
    attachments: [],
    content: 'Lag et dokument',
    createdAt: '2026-11-04T10:00:00.000Z',
    id: 'turn-1',
    role: 'assistant',
    streaming: false,
    tools: [],
    artifacts: [artifact],
  }
}

function item(artifact: ChatArtifact): ArtifactPanelItem {
  return { artifact, turn: turn(artifact) }
}

function documentArtifact(versions: number): ChatArtifact {
  let artifact: ChatArtifact | undefined
  for (let version = 1; version <= versions; version += 1) {
    artifact = mergeArtifactVersion(artifact, {
      id: 'report',
      kind: 'document',
      title: 'Rapport',
      content: `Utkast ${version}`,
      version,
    })
  }
  return artifact as ChatArtifact
}

function editing(overrides: Partial<ArtifactEditing> = {}): ArtifactEditing {
  return { enabled: true, threadId: 'thread-a', temporary: false, spaceThread: false, ...overrides }
}

const EDIT = { name: 'Rediger' }
const RESTORE = { name: 'Gjenopprett denne versjonen' }

describe('ArtifactsPanel editing (EP-3 S4)', () => {
  it('offers Edit on the newest revision of a chat document and saves through the editor', async () => {
    vi.mocked(chatClient.saveArtifactRevision).mockResolvedValue({
      id: 'rev-3',
      artifactId: 'report',
      version: 3,
      kind: 'document',
      title: 'Rapport',
      content: 'Mitt avsnitt',
      editorKind: 'user',
      createdAt: '2026-11-04T10:05:00Z',
    })
    const onSaved = vi.fn()
    render(() => <ArtifactsPanel items={[item(documentArtifact(2))]} editing={editing({ onSaved })} />)

    fireEvent.click(screen.getByRole('button', EDIT))
    flush()
    fireEvent.input(screen.getByRole('textbox', { name: 'Rediger Rapport' }), { target: { value: 'Mitt avsnitt' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1))
    expect(vi.mocked(chatClient.saveArtifactRevision).mock.calls[0]?.[0]).toMatchObject({
      threadId: 'thread-a',
      artifactId: 'report',
      expectedVersion: 2,
    })
    flush()
    // The panel shows the person's version at once, as the newest.
    expect(screen.getByText('Mitt avsnitt')).toBeTruthy()
    expect(screen.getByText('v3 · 3/3')).toBeTruthy()
    expect(screen.queryByRole('textbox')).toBeNull()
  })

  it('offers Restore on an older revision, against the newest version as the head', async () => {
    vi.mocked(chatClient.restoreArtifactRevision).mockResolvedValue({
      id: 'rev-3',
      artifactId: 'report',
      version: 3,
      kind: 'document',
      title: 'Rapport',
      content: 'Utkast 1',
      editorKind: 'restore',
      restoredFromVersion: 1,
      createdAt: '2026-11-04T10:05:00Z',
    })
    render(() => <ArtifactsPanel items={[item(documentArtifact(2))]} editing={editing()} />)

    expect(screen.queryByRole('button', RESTORE)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Forrige versjon' }))
    flush()
    expect(screen.queryByRole('button', EDIT)).toBeNull()
    fireEvent.click(screen.getByRole('button', RESTORE))

    await waitFor(() => expect(screen.getByText('v3 · 3/3')).toBeTruthy())
    expect(chatClient.restoreArtifactRevision).toHaveBeenCalledWith({
      threadId: 'thread-a',
      artifactId: 'report',
      version: 1,
      expectedVersion: 2,
    })
  })

  it('shows neither control without the editing capability', () => {
    render(() => <ArtifactsPanel items={[item(documentArtifact(2))]} />)
    expect(screen.queryByRole('button', EDIT)).toBeNull()
    render(() => <ArtifactsPanel items={[item(documentArtifact(2))]} editing={editing({ enabled: false })} />)
    expect(screen.queryByRole('button', EDIT)).toBeNull()
  })

  it('no_edit_on_room_surface', () => {
    render(() => <ArtifactsPanel items={[item(documentArtifact(2))]} surface="room" editing={editing()} />)
    expect(screen.queryByRole('button', EDIT)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Forrige versjon' }))
    flush()
    expect(screen.queryByRole('button', RESTORE)).toBeNull()
  })

  it('no_edit_for_pdf_image_binary', () => {
    const kinds: ChatArtifact[] = [
      { id: 'pdf', kind: 'file', title: 'Rapport.pdf', content: 'data:application/pdf;base64,JVBERi0=', version: 1 },
      { id: 'image', kind: 'image', title: 'Bilde', content: 'data:image/png;base64,iVBORw0KGgo=', version: 1 },
      { id: 'sheet', kind: 'spreadsheet', title: 'Ark.xlsx', content: 'data:application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;base64,UEsDBA==', version: 1 },
      { id: 'deck', kind: 'deck', title: 'Presentasjon', content: '{"slides":[]}', version: 1 },
    ]
    for (const artifact of kinds) {
      render(() => <ArtifactsPanel items={[item(artifact)]} editing={editing()} />)
      expect(screen.queryByRole('button', EDIT), artifact.kind).toBeNull()
      cleanup()
    }
  })

  it('no_edit_in_temporary_thread', () => {
    render(() => <ArtifactsPanel items={[item(documentArtifact(2))]} editing={editing({ temporary: true })} />)
    expect(screen.queryByRole('button', EDIT)).toBeNull()
    cleanup()
    render(() => <ArtifactsPanel items={[item(documentArtifact(2))]} editing={editing({ spaceThread: true })} />)
    expect(screen.queryByRole('button', EDIT)).toBeNull()
  })

  it('locks the artifact list while the editor is open', () => {
    const notes: ChatArtifact = { id: 'notes', kind: 'document', title: 'Notater', content: 'Notat', version: 1 }
    render(() => <ArtifactsPanel items={[item(notes), item(documentArtifact(1))]} editing={editing()} />)

    fireEvent.click(screen.getByRole('button', EDIT))
    flush()
    expect((screen.getByRole('tab', { name: /Notater/ }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt' }))
    flush()
    expect((screen.getByRole('tab', { name: /Notater/ }) as HTMLButtonElement).disabled).toBe(false)
  })
})
