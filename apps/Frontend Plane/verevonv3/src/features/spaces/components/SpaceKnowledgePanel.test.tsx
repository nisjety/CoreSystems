import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const spacesClient = vi.hoisted(() => ({ getSpaceKnowledge: vi.fn() }))
vi.mock('@/shared/api/spaces-client', () => spacesClient)

const { SpaceKnowledgePanel } = await import('./SpaceKnowledgePanel')

const space = { space_ref: 'space_1', name: 'Leveranse', kind: 'room', lifecycle: 'active' }
const membership = {
  space_ref: 'space_1', org_id: 'org_1', subject_id: 'user_1', kind: 'room', role: 'owner',
  revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
}

const answer = (overrides: Record<string, unknown>) => ({
  space,
  membership,
  binding: null,
  documents: [],
  documents_truncated: false,
  wiki_pages: [],
  unavailable: [],
  ...overrides,
})

beforeEach(() => {
  spacesClient.getSpaceKnowledge.mockReset()
})

afterEach(() => cleanup())

describe('SpaceKnowledgePanel', () => {
  it('lists the room’s documents and wiki pages under their own headings', async () => {
    spacesClient.getSpaceKnowledge.mockResolvedValue(
      answer({
        binding: { workspace_id: 'ws-drift', collection_id: null },
        documents: [
          { document_id: 'd1', title: 'Rutine for mottak', source: 'sharepoint', status: 'completed' },
        ],
        wiki_pages: [{ page_id: 'p1', title: 'Onboarding', path: '/onboarding' }],
      }),
    )
    render(() => <SpaceKnowledgePanel spaceRef="space_1" />)

    expect(await screen.findByText('Rutine for mottak')).toBeTruthy()
    expect(screen.getByText('Onboarding')).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Dokumenter' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Wiki-sider' })).toBeTruthy()
    // Which archive the room reads from is the fact that makes a wrong answer
    // explicable, so it is on screen rather than in a config file.
    expect(screen.getByText('ws-drift')).toBeTruthy()
  })

  // Retrieval is entitled separately from chat, so this is the state most
  // rooms are actually in. It must read as a permission, not a breakage.
  it('says a knowledge read is not authorized in the reader’s language', async () => {
    spacesClient.getSpaceKnowledge.mockResolvedValue(
      answer({
        unavailable: [{
          section: 'knowledge',
          code: 'knowledge_read_not_authorized',
          reason: 'Reading this Space’s knowledge is not authorized.',
        }],
      }),
    )
    render(() => <SpaceKnowledgePanel spaceRef="space_1" />)

    expect(await screen.findByText(/gis separat fra samtale/)).toBeTruthy()
    expect(screen.queryByText(/not authorized/)).toBeNull()
    // A gap is not an empty room: "nothing is connected" would be a claim we
    // cannot make when we could not look.
    expect(screen.queryByText(/Ingen dokumenter eller wiki-sider/)).toBeNull()
  })

  it('shows the documents that resolved beside a wiki gap', async () => {
    spacesClient.getSpaceKnowledge.mockResolvedValue(
      answer({
        documents: [{ document_id: 'd1', title: 'Avviksrapport' }],
        unavailable: [{
          section: 'wiki_pages',
          code: 'space_binding_has_no_wiki_workspace',
          reason: 'This Space’s Data binding names no wiki workspace.',
        }],
      }),
    )
    render(() => <SpaceKnowledgePanel spaceRef="space_1" />)

    expect(await screen.findByText('Avviksrapport')).toBeTruthy()
    expect(screen.getByText(/Wiki: /)).toBeTruthy()
    expect(screen.getByText(/ikke på et wiki-arbeidsområde/)).toBeTruthy()
  })

  // A build that does not know a code must still state the gap, in the
  // server's words, rather than dropping it or inventing a translation.
  it('falls back to the server sentence for an unrecognised code', async () => {
    spacesClient.getSpaceKnowledge.mockResolvedValue(
      answer({
        unavailable: [{ section: 'files', code: 'something_new', reason: 'Files are not published yet.' }],
      }),
    )
    render(() => <SpaceKnowledgePanel spaceRef="space_1" />)

    expect(await screen.findByText(/Files are not published yet/)).toBeTruthy()
    expect(screen.getByText(/files:/)).toBeTruthy()
  })

  it('says plainly when the room genuinely has no knowledge attached', async () => {
    spacesClient.getSpaceKnowledge.mockResolvedValue(answer({}))
    render(() => <SpaceKnowledgePanel spaceRef="space_1" />)

    expect(await screen.findByText(/Ingen dokumenter eller wiki-sider/)).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  // A capped list read as a complete archive is how someone concludes a
  // document is not in the room.
  it('marks a capped list as a page rather than the whole archive', async () => {
    spacesClient.getSpaceKnowledge.mockResolvedValue(
      answer({
        documents: [{ document_id: 'd1', title: 'Rutine' }],
        documents_truncated: true,
      }),
    )
    render(() => <SpaceKnowledgePanel spaceRef="space_1" />)

    expect(await screen.findByText(/ikke hele arkivet/)).toBeTruthy()
  })

  // A document still being processed cannot ground an answer yet.
  it('marks a document that is not ready, and stays quiet when it is', async () => {
    spacesClient.getSpaceKnowledge.mockResolvedValue(
      answer({
        documents: [
          { document_id: 'd1', title: 'Under arbeid', status: 'pending' },
          { document_id: 'd2', title: 'Ferdig', status: 'completed' },
        ],
      }),
    )
    render(() => <SpaceKnowledgePanel spaceRef="space_1" />)

    expect(await screen.findByText('pending')).toBeTruthy()
    expect(screen.queryByText('completed')).toBeNull()
  })

  it('separates “could not load” from “nothing is attached”', async () => {
    spacesClient.getSpaceKnowledge.mockRejectedValue(new Error('down'))
    render(() => <SpaceKnowledgePanel spaceRef="space_1" />)

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByText(/Arkivet er uendret/)).toBeTruthy()
    expect(screen.queryByText(/Ingen dokumenter eller wiki-sider/)).toBeNull()
  })

  // An untitled row is a real document. Dropping it would make the archive
  // look smaller than it is.
  it('names an untitled document instead of dropping it', async () => {
    spacesClient.getSpaceKnowledge.mockResolvedValue(
      answer({ documents: [{ document_id: 'd1', title: '   ' }] }),
    )
    render(() => <SpaceKnowledgePanel spaceRef="space_1" />)

    expect(await screen.findByText('Dokument uten tittel')).toBeTruthy()
  })
})
