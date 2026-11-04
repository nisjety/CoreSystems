import { describe, expect, it } from 'vitest'
import { artifactVersionAt, artifactVersions, mergeArtifactVersion, mergeServerRevisions } from './chat-artifacts'
import type { ChatArtifact, ChatArtifactVersion } from './chat-types'

// EP-3 S3: revisions read from the server's history fold into the streamed history.

function streamed(version: number, content: string): ChatArtifact {
  return { id: 'report', kind: 'document', title: 'Rapport', content, version }
}

function server(version: number, content: string, extra: Partial<ChatArtifactVersion> = {}): ChatArtifactVersion {
  return { content, title: 'Rapport', version, editorKind: 'model', ...extra }
}

describe('mergeServerRevisions', () => {
  it('server_revisions_fill_history', () => {
    // After a reload only the newest turn's revision streamed back.
    const artifact = streamed(3, 'tredje')
    const merged = mergeServerRevisions(artifact, [
      server(3, 'tredje'),
      server(2, 'andre', { editorKind: 'user' }),
      server(1, 'første'),
    ])
    expect(artifactVersions(merged).map((entry) => entry.version)).toEqual([1, 2, 3])
    expect(artifactVersionAt(merged, 2)).toMatchObject({ content: 'andre', editorKind: 'user' })
    expect(merged.version).toBe(3)
    expect(merged.content).toBe('tredje')
  })

  it('equal_version_prefers_server', () => {
    const artifact = mergeArtifactVersion(streamed(1, 'utkast'), streamed(2, 'strømmet tekst'))
    const merged = mergeServerRevisions(artifact, [server(2, 'lagret tekst')])
    const second = artifactVersionAt(merged, 2)
    expect(second.content).toBe('lagret tekst')
    expect(second.editorKind).toBe('model')
    // The streamed-only revision the server has not paged in yet is kept.
    expect(artifactVersionAt(merged, 1).content).toBe('utkast')
  })

  it('a newer server head becomes what the panel shows', () => {
    // Another device revised the document after this tab streamed version 2.
    const merged = mergeServerRevisions(streamed(2, 'gammel'), [
      server(4, 'gjenopprettet', { editorKind: 'restore', restoredFromVersion: 1 }),
    ])
    expect(merged.version).toBe(4)
    expect(merged.content).toBe('gjenopprettet')
    expect(merged.editor).toBe('restore')
    expect(artifactVersionAt(merged, 4).restoredFromVersion).toBe(1)
  })

  it('keeps the server attribution when the newest streamed frame named none', () => {
    const merged = mergeServerRevisions(streamed(2, 'tekst'), [server(2, 'tekst', { editorKind: 'user' })])
    // Folding the same streamed frame again must not erase who made the revision.
    const again = mergeArtifactVersion(merged, streamed(2, 'tekst'))
    expect(artifactVersionAt(again, 2).editorKind).toBe('user')
  })

  it('ignores a malformed server entry', () => {
    const merged = mergeServerRevisions(streamed(1, 'en'), [
      { title: 'Rapport', version: 2 } as unknown as ChatArtifactVersion,
    ])
    expect(artifactVersions(merged).map((entry) => entry.version)).toEqual([1])
  })

  it('an empty page changes nothing a reader sees', () => {
    const artifact = mergeArtifactVersion(streamed(1, 'en'), streamed(2, 'to'))
    const merged = mergeServerRevisions(artifact, [])
    expect(artifactVersions(merged)).toEqual(artifactVersions(artifact))
    expect(merged.editor).toBeUndefined()
  })
})
