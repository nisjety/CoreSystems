// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import KnowledgePage from '@/features/knowledge/components/KnowledgePage'

function makeFetchResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

const knowledgePayload = {
  data: {
    generatedAt: '2026-06-06T12:00:00.000Z',
    orgId: 'org-1',
    collections: [
      { id: 'all', label: 'General Knowledge', count: 5 },
      { id: 'provider:microsoft', label: 'Microsoft 365', count: 2 },
      { id: 'web', label: 'Web sources', count: 1 },
    ],
    dataPlane: {
      available: true,
      documentCount: 24,
      indexedCount: 22,
    },
    graph: {
      available: true,
      edgeCount: 9,
      groups: ['policy', 'workspace'],
      nodeCount: 6,
      truncated: false,
      nodes: [
        {
          id: 'entity-shipping',
          label: 'Shipping policy',
          group: 'policy',
          tone: 'policy',
          x: 140,
          y: 120,
          radius: 22,
          sourceRefs: ['kid-1'],
          sourceIds: ['doc-shipping'],
        },
        {
          id: 'entity-support',
          label: 'Support workspace',
          group: 'workspace',
          tone: 'support',
          x: 320,
          y: 210,
          radius: 20,
          sourceRefs: ['kid-2'],
          sourceIds: ['doc-returns'],
        },
      ],
      links: [
        {
          from: 'entity-shipping',
          to: 'entity-support',
          label: 'supports',
          strength: 2,
          sourceRefs: ['kid-1'],
        },
      ],
    },
    metrics: {
      connected: 3,
      failed: 0,
      syncing: 1,
    },
    metricCards: [
      { label: 'Indexed documents', value: '22', delta: '2 pending', tone: 'warn' },
      { label: 'Connected integrations', value: '3', delta: '1 syncing', tone: 'good' },
      { label: 'Graph entities', value: '6', delta: '9 edges', tone: 'good' },
      { label: 'Reclaim opportunities', value: '0', delta: 'No cleanup queued', tone: 'good' },
    ],
    folders: [
      {
        id: 'folder-ms',
        title: 'Support knowledge',
        subtitle: 'SharePoint / OneDrive',
        providerKey: 'microsoft',
        primaryValue: '18',
        primaryLabel: 'Files',
        secondaryValue: '128 MB',
        secondaryLabel: 'Stored',
        connections: ['Microsoft 365', 'Finspo'],
        tone: 'warm',
      },
    ],
    integrations: [
      {
        id: 'conn-microsoft',
        name: 'Microsoft 365',
        providerKey: 'microsoft',
        status: 'Syncing',
        documents: '2 drives · 18 docs',
        freshness: '5m ago',
        detail: 'Microsoft 365',
      },
      {
        id: 'conn-notion',
        name: 'Notion',
        providerKey: 'notion',
        status: 'Connected',
        documents: '6 docs',
        freshness: 'Live',
        detail: 'Notion',
      },
    ],
    files: [
      {
        id: 'doc-shipping',
        name: 'Shipping FAQ',
        addedBy: 'System',
        source: 'Notion',
        providerKey: 'notion',
        updated: '5m ago',
        type: 'Notion',
      },
      {
        id: 'doc-returns',
        name: 'Returns Policy',
        addedBy: 'System',
        source: 'Microsoft 365',
        providerKey: 'microsoft',
        updated: '12m ago',
        type: 'Docs',
      },
    ],
    sources: [
      {
        id: 'doc-shipping',
        title: 'Shipping FAQ',
        description: 'Notion · Indexed',
        type: 'Notion',
        provider: 'Notion',
        providerKey: 'notion',
        category: 'policy',
        owner: 'System',
        updated: '5m ago',
        size: '12 KB',
        status: 'Indexed',
        chunks: 3,
        hitRate: '88%',
        coverage: '100%',
        similarity: '0.88',
        tags: ['Notion', 'policy'],
        related: ['Shipping policy'],
        chunksPreview: [
          { id: 'kid-1', title: 'Chunk 1', score: '#1', text: 'Escalate delayed parcels after the promised delivery window closes.' },
        ],
      },
      {
        id: 'doc-returns',
        title: 'Returns Policy',
        description: 'Microsoft 365 · Indexed',
        type: 'Docs',
        provider: 'Microsoft 365',
        providerKey: 'microsoft',
        category: 'policy',
        owner: 'System',
        updated: '12m ago',
        size: '18 KB',
        status: 'Indexed',
        chunks: 2,
        hitRate: '91%',
        coverage: '100%',
        similarity: '0.91',
        tags: ['Microsoft 365', 'policy'],
        related: ['Support workspace'],
        chunksPreview: [
          { id: 'kid-2', title: 'Chunk 1', score: '#1', text: 'Items must be returned within 30 days of delivery.' },
        ],
      },
    ],
    webSources: [
      {
        id: 'web-docs',
        name: 'Velion docs',
        url: 'https://docs.velion.ai',
        kind: 'crawl',
        status: 'active',
        providerKey: 'web',
        updated: '8m ago',
      },
    ],
    diagnostics: {
      available: true,
      sparseBackend: 'quickwit-with-postgres-fallback',
      vectorCollections: ['dataplane_knowledge', 'wiki_block_embeddings', 'entity_summary_embeddings'],
      quickwitIndexes: ['dataplane-corpus'],
      services: [
        {
          id: 'retrieval-engine',
          label: 'Retrieval engine',
          status: 'Ready',
          tone: 'good',
          detail: 'Sparse backend quickwit-with-postgres-fallback is serving live retrieval.',
          meta: 'retrieval-engine-rs',
        },
      ],
      storage: [
        {
          id: 'qdrant',
          label: 'Qdrant vectors',
          status: 'Ready',
          tone: 'good',
          detail: 'Collections include dataplane_knowledge and wiki_block_embeddings.',
          meta: '3 collections',
        },
      ],
      capabilities: [
        {
          id: 'embeddings',
          label: 'Embedding system',
          status: 'Live',
          tone: 'good',
          detail: 'embedding-engine and Qdrant collections are ready.',
          meta: 'wiki_block_embeddings',
        },
        {
          id: 'context-mode',
          label: 'Context mode',
          status: 'Not wired',
          tone: 'warn',
          detail: 'No current Data Plane v2 implementation was detected.',
        },
      ],
    },
    finspo: {
      available: true,
      sourceCount: 2,
      largestCount: 3,
      inactiveCount: 1,
      duplicateGroups: 0,
      recommendationCount: 0,
      reclaimableBytes: 0,
    },
  },
}

function renderKnowledgePage() {
  window.history.pushState(null, '', '/knowledge')
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/*all" component={KnowledgePage} />
    </Router>
  ))
}

describe('KnowledgePage', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn(async () => makeFetchResponse(knowledgePayload)))
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    window.history.pushState(null, '', '/')
  })

  it('renders the live overview payload', async () => {
    renderKnowledgePage()

    expect(await screen.findByRole('heading', { name: /^folders$/i })).toBeTruthy()
    expect((screen.getByRole('combobox', { name: /select knowledge collection/i }) as HTMLSelectElement).value).toBe('all')
    expect(screen.getByRole('heading', { name: /^integrations$/i, level: 2 })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /^files$/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /tracked web sources/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /data plane status/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /support knowledge/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /microsoft 365/i })).toBeTruthy()
    expect(screen.getByText(/docs\.velion\.ai/i)).toBeTruthy()
    expect(screen.getAllByText(/quickwit-with-postgres-fallback/i).length).toBeGreaterThan(0)
    expect(screen.getByText(/context mode/i)).toBeTruthy()
    expect(screen.getAllByText(/shipping faq/i)[0]).toBeTruthy()
  })

  it('switches from overview to graph and chunks using live data', async () => {
    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^folders$/i })
    fireEvent.click(screen.getByRole('button', { name: /graph/i }))

    expect(screen.getByRole('region', { name: /raggraph relationship map/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /shipping policy/i, level: 2 })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /chunks/i }))

    expect(screen.getByRole('heading', { name: /^sources$/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /returns policy/i })).toBeTruthy()
  })

  it('opens the add source modal', async () => {
    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^folders$/i })
    fireEvent.click(screen.getByRole('button', { name: /add source/i }))

    expect(screen.getByRole('dialog', { name: /add knowledge source/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /upload files/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /connect a workspace/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /crawl a website/i })).toBeTruthy()
  })

  it('filters live knowledge by collection and search query', async () => {
    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^folders$/i })
    fireEvent.change(screen.getByRole('combobox', { name: /select knowledge collection/i }), {
      target: { value: 'provider:microsoft' },
    })

    expect(screen.getByRole('heading', { name: /support knowledge/i })).toBeTruthy()
    expect(screen.queryByText(/velion docs/i)).toBeNull()
    expect(screen.getAllByText(/returns policy/i)[0]).toBeTruthy()
    expect(screen.queryAllByText(/shipping faq/i)).toHaveLength(0)

    const search = screen.getByRole('textbox', { name: /search files and sources/i })
    fireEvent.input(search, { target: { value: 'returns' } })

    expect(screen.getAllByText(/returns policy/i)[0]).toBeTruthy()
    expect(screen.queryAllByText(/shipping faq/i)).toHaveLength(0)
  })

  it('renders safely when diagnostics are missing from the payload', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => makeFetchResponse({
      data: {
        ...knowledgePayload.data,
        diagnostics: undefined,
      },
    })))

    renderKnowledgePage()

    expect(await screen.findByRole('heading', { name: /data plane status/i })).toBeTruthy()
    expect(screen.getByText(/sparse backend: unknown/i)).toBeTruthy()
    expect(screen.getAllByText(/no live diagnostics were returned for this group yet/i).length).toBeGreaterThan(0)
  })

  it('starts a website crawl from the add source modal', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url === '/api/v1/knowledge/crawl') {
        return makeFetchResponse({
          data: {
            id: 'crawl-1',
            status: 'queued',
            target: 'https://docs.velion.ai',
            createdAt: '2026-06-06T12:05:00.000Z',
          },
        }, 202)
      }
      return makeFetchResponse(knowledgePayload)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^folders$/i })
    fireEvent.click(screen.getByRole('button', { name: /add source/i }))
    fireEvent.input(screen.getByLabelText(/website url/i), {
      target: { value: 'https://docs.velion.ai' },
    })
    fireEvent.input(screen.getByLabelText(/max pages/i), {
      target: { value: '16' },
    })
    fireEvent.click(screen.getByRole('button', { name: /start crawl/i }))

    await waitFor(() => expect(screen.getByText(/started a website crawl for https:\/\/docs\.velion\.ai/i)).toBeTruthy())
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/knowledge/crawl',
      expect.objectContaining({
        method: 'POST',
      }),
    )
  })
})
