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
        name: 'Verevon docs',
        url: 'https://docs.verevon.ai',
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

const emptyOperatingMapPayload = {
  data: {
    map: null,
    current_version: null,
    proposals: [],
    blueprint_suggestions: [],
  },
}

const operatingMapVersionPayload = {
  version_id: 'version-1',
  operating_map_id: 'map-1',
  org_id: 'org-1',
  summary: 'Generated from Knowledge evidence.',
  confidence: 0.72,
  departments: [
    { id: 'support', name: 'Support', confidence: 0.8, evidence_refs: ['doc-returns'] },
  ],
  workflows: [
    {
      id: 'support-triage',
      department_id: 'support',
      name: 'Support triage',
      phase: 'Assist',
      risk: 'medium',
      evidence_refs: ['doc-returns'],
    },
  ],
  agent_blueprints: [
    {
      id: 'service-agent',
      name: 'Service agent',
      role: 'service',
      source_workflow_id: 'support-triage',
      requires_approval: true,
    },
  ],
  rollout_phases: [
    { id: 'assist', name: 'Assist', description: 'Human copilots and low-risk productivity support.' },
    { id: 'ground', name: 'Ground', description: 'Shared knowledge and workflow memory.' },
    { id: 'act', name: 'Act', description: 'Approved autonomous or semi-autonomous agents.' },
  ],
  risk_overlays: [
    { id: 'human-review', label: 'Human review required', severity: 'medium' },
  ],
  learning_modules: [
    { id: 'approval-patterns', title: 'Approval patterns', audience: 'operators' },
  ],
  roi_notes: [],
  evidence_refs: ['doc-returns'],
}

const generatedOperatingMapPayload = {
  data: {
    proposal: {
      proposal_id: 'proposal-1',
      operating_map_id: 'map-1',
      org_id: 'org-1',
      proposal_status: 'pending',
      generated_by_run_id: 'run-1',
      evidence_refs: [],
      created_at: '2026-06-18T12:00:00.000Z',
      proposed_version: {
        ...operatingMapVersionPayload,
        version_id: '',
      },
    },
    run_id: 'run-1',
    status: 'proposal_created',
  },
}

function acceptedOperatingMapPayload(suggested = false) {
  return {
    data: {
      map: {
        operating_map_id: 'map-1',
        org_id: 'org-1',
        status: 'published',
        current_version_id: 'version-1',
        generated_from: {},
        created_at: '2026-06-18T12:00:00.000Z',
        updated_at: '2026-06-18T12:00:00.000Z',
      },
      current_version: operatingMapVersionPayload,
      proposals: [],
      blueprint_suggestions: suggested
        ? [
            {
              suggestion_id: 'suggestion-1',
              operating_map_id: 'map-1',
              version_id: 'version-1',
              org_id: 'org-1',
              blueprint_id: 'service-agent',
              role: 'service',
              source_workflow_id: 'support-triage',
              name: 'Service agent',
              suggestion_status: 'suggested',
              payload: {},
              created_at: '2026-06-18T12:01:00.000Z',
              updated_at: '2026-06-18T12:01:00.000Z',
            },
          ]
        : [],
    },
  }
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

    expect(await screen.findByRole('heading', { name: /^mapper$/i })).toBeTruthy()
    expect((screen.getByRole('combobox', { name: /velg kunnskapssamling/i }) as HTMLSelectElement).value).toBe('all')
    expect(screen.getByRole('heading', { name: /^integrasjoner$/i, level: 2 })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /^filer$/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /sporede nettkilder/i })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: /data plane status/i })).toBeNull()
    expect(screen.getByRole('heading', { name: /support knowledge/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /microsoft 365/i })).toBeTruthy()
    expect(screen.getByText(/docs\.verevon\.ai/i)).toBeTruthy()
    expect(screen.getAllByText(/shipping faq/i)[0]).toBeTruthy()
  })

  it('exposes the Docs toolbar and switches the overview layout accessibly', async () => {
    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^mapper$/i })

    expect(screen.getByRole('textbox', { name: /søk i kunnskapsbasen/i })).toBeTruthy()
    const gridView = screen.getByRole('button', { name: /rutenettvisning/i })
    const listView = screen.getByRole('button', { name: /listevisning/i })
    expect(gridView.getAttribute('aria-pressed')).toBe('true')
    expect(listView.getAttribute('aria-pressed')).toBe('false')

    fireEvent.click(listView)

    expect(listView.getAttribute('aria-pressed')).toBe('true')
    expect(gridView.getAttribute('aria-pressed')).toBe('false')
    expect(screen.getByRole('button', { name: /filtrer kunnskap/i })).toBeTruthy()
  })

  it('switches from overview to graph and chunks using live data', async () => {
    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^mapper$/i })
    fireEvent.click(screen.getByRole('button', { name: /graf/i }))

    expect(screen.getByRole('region', { name: /raggraph-relasjonskart/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /shipping policy/i, level: 2 })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /utdrag/i }))

    expect(screen.getByRole('heading', { name: /^kilder$/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /returns policy/i })).toBeTruthy()
  })

  it('opens a navbar knowledge result on its exact chunk-backed source', async () => {
    window.history.pushState(null, '', '/knowledge?source=doc-returns')
    render(() => (
      <Router root={(props) => <>{props.children}</>}>
        <Route path="/*all" component={KnowledgePage} />
      </Router>
    ))

    const source = await screen.findByRole('button', { name: /returns policy/i })
    expect(source.getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('heading', { name: /returns policy/i, level: 2 })).toBeTruthy()
  })

  it('renders the Operating Map tab and generates a reviewable proposal', async () => {
    let generated = false
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url === '/api/v1/knowledge/operating-map') {
        if (!generated) return makeFetchResponse(emptyOperatingMapPayload)
        return makeFetchResponse({
          data: {
            map: {
              operating_map_id: 'map-1',
              org_id: 'org-1',
              status: 'draft',
              current_version_id: null,
              generated_from: {},
              created_at: '2026-06-18T12:00:00.000Z',
              updated_at: '2026-06-18T12:00:00.000Z',
            },
            current_version: null,
            proposals: [generatedOperatingMapPayload.data.proposal],
          },
        })
      }
      if (url === '/api/v1/knowledge/operating-map/generate') {
        generated = true
        return makeFetchResponse(generatedOperatingMapPayload, 202)
      }
      if (url === '/api/v1/knowledge/operating-map/runs/run-1/events') {
        return new Response('event: status\ndata: {"status":"completed","detail":"Operating Map proposal is ready."}\n\n', {
          headers: { 'Content-Type': 'text/event-stream' },
          status: 200,
        })
      }
      return makeFetchResponse(knowledgePayload)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^mapper$/i })
    fireEvent.click(screen.getByRole('button', { name: /ai-kart/i }))

    expect(screen.getByRole('heading', { name: /evidence-grounded ai rollout map/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /no operating map yet/i })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /generate map/i }))

    await waitFor(() => expect(screen.getByRole('heading', { name: /proposal awaiting review/i })).toBeTruthy())
    expect(screen.getByRole('heading', { name: /support triage/i })).toBeTruthy()
    expect(screen.getByText(/returns policy/i)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /view evidence/i }))
    expect(screen.getByRole('region', { name: /evidence for support triage/i })).toBeTruthy()
    expect(screen.getByText(/graph: support workspace/i)).toBeTruthy()
    expect(screen.getByRole('button', { name: /accept/i })).toBeTruthy()
  })

  it('creates a durable blueprint suggestion from an accepted Operating Map', async () => {
    let suggested = false
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/v1/knowledge/operating-map') {
        return makeFetchResponse(acceptedOperatingMapPayload(suggested))
      }
      if (url === '/api/v1/actions/execute') {
        const body = JSON.parse(String(init?.body))
        expect(body.actionId).toBe('operating_map.create_agent_blueprint')
        expect(body.input).toMatchObject({
          versionId: 'version-1',
          blueprintId: 'service-agent',
          role: 'service',
          sourceWorkflowId: 'support-triage',
          name: 'Service agent',
        })
        suggested = true
        return makeFetchResponse({
          data: {
            actionId: 'operating_map.create_agent_blueprint',
            runId: 'agent_blueprint_service-agent_user-1',
            status: 'completed',
            auditId: 'audit-agent-blueprint',
            result: {
              suggestion: acceptedOperatingMapPayload(true).data.blueprint_suggestions[0],
            },
          },
        })
      }
      return makeFetchResponse(knowledgePayload)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^mapper$/i })
    fireEvent.click(screen.getByRole('button', { name: /ai-kart/i }))

    expect(await screen.findByRole('heading', { name: /accepted operating map/i })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /create blueprint/i }))

    await waitFor(() => expect(screen.getByText(/agentmal-forslag lagret for gjennomgang i agenter/i)).toBeTruthy())
    expect((screen.getByRole('button', { name: /suggested/i }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('opens the add source modal', async () => {
    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^mapper$/i })
    fireEvent.click(screen.getByRole('button', { name: /legg til kilde/i }))

    expect(screen.getByRole('dialog', { name: /legg til kunnskapskilde/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /last opp filer/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /koble til et arbeidsområde/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /gjennomsøk et nettsted/i })).toBeTruthy()
  })

  it('filters live knowledge by collection and search query', async () => {
    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^mapper$/i })
    fireEvent.change(screen.getByRole('combobox', { name: /velg kunnskapssamling/i }), {
      target: { value: 'provider:microsoft' },
    })

    expect(screen.getByRole('heading', { name: /support knowledge/i })).toBeTruthy()
    expect(screen.queryByText(/verevon docs/i)).toBeNull()
    expect(screen.getAllByText(/returns policy/i)[0]).toBeTruthy()
    expect(screen.queryAllByText(/shipping faq/i)).toHaveLength(0)

    const search = screen.getByRole('textbox', { name: /søk i filer og kilder/i })
    fireEvent.input(search, { target: { value: 'returns' } })

    expect(screen.getAllByText(/returns policy/i)[0]).toBeTruthy()
    expect(screen.queryAllByText(/shipping faq/i)).toHaveLength(0)
  })

  it('keeps the overview usable when diagnostics are missing from the payload', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => makeFetchResponse({
      data: {
        ...knowledgePayload.data,
        diagnostics: undefined,
      },
    })))

    renderKnowledgePage()

    expect(await screen.findByRole('heading', { name: /^mapper$/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /^filer$/i })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: /data plane status/i })).toBeNull()
  })

  it('starts a website crawl from the add source modal', async () => {
    // The mocked response intentionally mirrors the REAL gateway contract
    // (apps/gateway/src/domains/knowledge/quarry.rs::start_crawl) which never
    // echoes the submitted URL back — only { id, jobId, runId, kind, status,
    // acceptedAt, eventStream, upstream }. A mock that fabricated a `target`
    // field here previously masked a bug where the confirmation message read
    // `result.target` (always undefined) instead of the URL the user typed.
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url === '/api/v1/knowledge/crawl') {
        return makeFetchResponse({
          data: {
            id: 'crawl-1',
            jobId: 'crawl-1',
            runId: null,
            kind: 'crawl',
            status: 'queued',
            acceptedAt: null,
            eventStream: '/api/v1/knowledge/jobs/crawl-1/events',
            upstream: {},
          },
        }, 202)
      }
      // Selective-ingest resolution reads the user's preferences before the
      // crawl. An unset crawlIngestMode defaults to 'auto', so this explicit
      // add-source crawl must send ingest:true (else quarry persists nothing).
      if (url === '/api/v1/preferences') {
        return makeFetchResponse({ data: {} })
      }
      return makeFetchResponse(knowledgePayload)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderKnowledgePage()

    await screen.findByRole('heading', { name: /^mapper$/i })
    fireEvent.click(screen.getByRole('button', { name: /legg til kilde/i }))
    fireEvent.input(screen.getByLabelText(/nettadresse/i), {
      target: { value: 'https://docs.verevon.ai' },
    })
    fireEvent.input(screen.getByLabelText(/maks antall sider/i), {
      target: { value: '16' },
    })
    fireEvent.click(screen.getByRole('button', { name: /start gjennomsøking/i }))

    // The typed URL must reach the request payload sent to the backend, and the
    // resolved ingest decision (auto default → true) must ride along so the
    // crawl actually persists into Knowledge...
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/knowledge/crawl',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ url: 'https://docs.verevon.ai', maxPages: 16, ingest: true }),
      }),
    ))

    // ...and the confirmation message must display that same URL — not
    // "undefined" — even though the response never echoes it back.
    await waitFor(() => expect(screen.getByText(/startet en gjennomsøking av nettstedet for https:\/\/docs\.verevon\.ai/i)).toBeTruthy())
    expect(screen.queryByText(/undefined/i)).toBeNull()
    expect(screen.getByText(/følg kjøring crawl-1/i)).toBeTruthy()
  })

  it('refetches the knowledge workspace once the crawl run-event stream reaches completion', async () => {
    // Regression test for the "Tracked web sources" staleness bug: previously
    // the page refetched /api/v1/knowledge/sources exactly ONCE, immediately
    // after the crawl POST resolved — before the crawl (which runs
    // asynchronously in Quarry) had ingested a single page. This test proves
    // a SECOND refetch now happens once the crawl's own run-event SSE stream
    // reaches a terminal state, without requiring a manual page reload.
    let sourcesCallCount = 0
    const encoder = new TextEncoder()
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url === '/api/v1/knowledge/crawl') {
        return makeFetchResponse({
          data: {
            id: 'crawl-2',
            jobId: 'crawl-2',
            runId: null,
            kind: 'crawl',
            status: 'queued',
            acceptedAt: null,
            eventStream: '/api/v1/knowledge/jobs/crawl-2/events',
            upstream: {},
          },
        }, 202)
      }
      if (url.includes('/api/v1/knowledge/jobs/crawl-2/events')) {
        // Minimal terminal-status SSE stream, then close — mirrors a crawl
        // that finished ingesting.
        const body = new ReadableStream<Uint8Array>({
          start(controller) {
            controller.enqueue(encoder.encode('event: run_completed\ndata: {"status":"completed"}\n\n'))
            controller.close()
          },
        })
        return new Response(body, {
          status: 200,
          headers: { 'Content-Type': 'text/event-stream' },
        })
      }
      if (url === '/api/v1/knowledge/sources') {
        sourcesCallCount += 1
      }
      return makeFetchResponse(knowledgePayload)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderKnowledgePage()
    await screen.findByRole('heading', { name: /^mapper$/i })
    const callsBeforeCrawl = sourcesCallCount

    fireEvent.click(screen.getByRole('button', { name: /legg til kilde/i }))
    fireEvent.input(screen.getByLabelText(/nettadresse/i), {
      target: { value: 'https://docs.verevon.ai' },
    })
    fireEvent.click(screen.getByRole('button', { name: /start gjennomsøking/i }))

    await waitFor(() => expect(screen.getByText(/startet en gjennomsøking av nettstedet for https:\/\/docs\.verevon\.ai/i)).toBeTruthy())

    // Two refetches are expected: the immediate one right after the crawl
    // starts, and the completion-triggered one once the SSE stream ends.
    await waitFor(() => expect(sourcesCallCount).toBeGreaterThanOrEqual(callsBeforeCrawl + 2))
  })
})
