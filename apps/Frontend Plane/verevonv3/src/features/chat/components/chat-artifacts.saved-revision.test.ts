import { describe, expect, it } from 'vitest'
import {
  artifactVersions,
  foldSavedRevision,
  latestArtifactVersion,
  mergeArtifactVersion,
  newestTurnCarrying,
  savedRevisionVersion,
  type SavedRevision,
} from './chat-artifacts'
import { collectArtifactItems } from './chat-media-markdown'
import type { ChatArtifact, ChatTurn } from './chat-types'

// EP-3 S4: a revision the person saved or restored in the Output panel folds into the conversation's artifact state.

function documentArtifact(versions: number, id = 'report'): ChatArtifact {
  let artifact: ChatArtifact | undefined
  for (let version = 1; version <= versions; version += 1) {
    artifact = mergeArtifactVersion(artifact, { id, kind: 'document', title: 'Rapport', content: `Utkast ${version}`, version })
  }
  return artifact as ChatArtifact
}

function turnWith(id: string, artifacts: ChatArtifact[]): ChatTurn {
  return {
    attachments: [],
    content: '',
    createdAt: '2026-11-04T10:00:00.000Z',
    id,
    role: 'assistant',
    streaming: false,
    tools: [],
    artifacts,
  }
}

function saved(overrides: Partial<SavedRevision> = {}): SavedRevision {
  return {
    artifactId: 'report',
    content: 'Mitt avsnitt',
    title: 'Rapport',
    version: 3,
    editorKind: 'user',
    ...overrides,
  }
}

describe('a revision the person saved (EP-3 S4)', () => {
  it('becomes the newest version, attributed to the person', () => {
    const artifact = foldSavedRevision([documentArtifact(2)], saved())[0] as ChatArtifact
    expect(artifactVersions(artifact).map((entry) => entry.version)).toEqual([1, 2, 3])
    expect(artifact).toMatchObject({ version: 3, content: 'Mitt avsnitt', editor: 'user' })
    expect(latestArtifactVersion(artifact)).toMatchObject({ version: 3, editorKind: 'user' })
  })

  it('keeps the version a restore brought back, and labels the restore', () => {
    const artifact = foldSavedRevision(
      [documentArtifact(2)],
      saved({ version: 4, editorKind: 'restore', content: 'Utkast 1', restoredFromVersion: 1 }),
    )[0] as ChatArtifact
    expect(latestArtifactVersion(artifact)).toMatchObject({ version: 4, editorKind: 'restore', restoredFromVersion: 1 })
    expect(artifact.editor).toBe('restore')
  })

  it('names no restored version for an edit', () => {
    expect('restoredFromVersion' in savedRevisionVersion(saved())).toBe(false)
    expect(savedRevisionVersion(saved({ editorKind: 'user' }))).toMatchObject({ editorKind: 'user', content: 'Mitt avsnitt' })
  })

  it('leaves every other artifact of the conversation as it was', () => {
    const notes = documentArtifact(1, 'notes')
    const folded = foldSavedRevision([notes, documentArtifact(2)], saved())
    expect(folded[0]).toBe(notes)
    expect(folded[1]).toMatchObject({ id: 'report', version: 3 })
  })

  it('folds into the newest turn that carries the artifact, and nowhere else', () => {
    const turns = [turnWith('turn-a', [documentArtifact(1)]), turnWith('turn-b', [documentArtifact(2)]), turnWith('turn-c', [])]
    expect(newestTurnCarrying(turns, 'report')).toBe(1)
    expect(newestTurnCarrying(turns, 'missing')).toBe(-1)
    expect(newestTurnCarrying([{ ...turnWith('turn-a', []), artifacts: undefined }], 'report')).toBe(-1)
  })

  it('is the one panel item, on the turn that carries it', () => {
    const turns = [turnWith('turn-a', [documentArtifact(1)]), turnWith('turn-b', [documentArtifact(2)])]
    const index = newestTurnCarrying(turns, 'report')
    const folded = turns.map((turn, position) => (
      position === index ? { ...turn, artifacts: foldSavedRevision(turn.artifacts ?? [], saved()) } : turn
    ))
    const items = collectArtifactItems(folded)
    expect(items).toHaveLength(1)
    expect(items[0]?.artifact).toMatchObject({ version: 3, content: 'Mitt avsnitt', editor: 'user' })
    expect(items[0]?.turn.id).toBe('turn-b')
  })

  it('reaches the page through the editing prop, from the gateway capability', async () => {
    const { readFileSync } = await import('node:fs')
    const { resolve } = await import('node:path')
    const page = readFileSync(resolve(process.cwd(), 'src/features/chat/components/ChatPage.tsx'), 'utf8')
    expect(page).toContain('enabled: composerCapabilities()?.artifactEditing === true')
    expect(page).toContain('spaceThread: spaceScoped()')
    expect(page).toContain('onSaved: foldSavedArtifactRevision')
  })
})
