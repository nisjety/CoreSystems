// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createRouter, memoryHistory } from '@solidjs/router'
import type { JSX } from '@solidjs/web'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import StudioPage from '@/features/studio/components/StudioPage'

function renderWithRouter(component: () => JSX.Element, path = '/studio/canvas') {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component }],
    history: memoryHistory(path),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

describe('StudioPage', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('opens to an honest empty canvas and can add an editable text block', () => {
    renderWithRouter(() => <StudioPage section="canvas" />)

    // Component defaults to the Norwegian locale text (i18n.tr(no, en) with no
    // I18nProvider in the render tree resolves to the Norwegian string).
    expect(screen.getByText('Lanseringslerret')).toBeTruthy()
    expect(screen.getByLabelText('Studio-lerretsarbeidsområde')).toBeTruthy()
    // Phase 4 PR-3 seed strip: the canvas starts empty (no fabricated demo blocks).
    expect(screen.getByText('Start et Studio-brett')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Legg til Tekst-blokk' }))
    flush()

    expect(screen.getAllByText('Text note 1').length).toBeGreaterThanOrEqual(1)
    fireEvent.input(screen.getByDisplayValue('Text note 1'), {
      target: { value: 'Launch proof point' },
    })
    flush()
    expect(screen.getAllByText('Launch proof point').length).toBeGreaterThanOrEqual(1)
  })

  it('duplicates, deletes, and recovers back to the empty canvas', () => {
    renderWithRouter(() => <StudioPage section="canvas" />)

    // Build up from the empty canvas, then duplicate the added block. Each
    // click's addBlock/duplicateSelected/deleteSelected handler stages a
    // setBlocks + setSelectedBlockId write; the *next* click's handler reads
    // selectedBlock()/blocks() fresh, so every click needs its own flush()
    // before the next one fires — not just once at the end — or a later
    // handler will act on the previous click's stale pre-write state.
    fireEvent.click(screen.getByRole('button', { name: 'Legg til Tekst-blokk' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Dupliser valgt blokk' }))
    flush()

    expect(screen.getByDisplayValue('Text note 1 copy')).toBeTruthy()

    // Delete both blocks; the empty-state affordance returns.
    fireEvent.click(screen.getByRole('button', { name: 'Slett valgt blokk' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Slett valgt blokk' }))
    flush()

    expect(screen.getByText('Start et Studio-brett')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Tekst' }))
    flush()

    expect(screen.getByDisplayValue('Text note 1')).toBeTruthy()
  })

  it('saves the loaded Studio project before exporting it to a Social draft', async () => {
    const fetchMock = stubStudioFetch()
    renderWithRouter(() => <StudioPage section="canvas" />)

    await waitFor(() => {
      expect((screen.getByRole('button', { name: 'Send til utkast' }) as HTMLButtonElement).disabled).toBe(false)
    })

    fireEvent.click(screen.getByRole('button', { name: 'Send til utkast' }))

    await waitFor(() => {
      expect(fetchMock.mock.calls.some(([url, init]) => (
        String(url).endsWith('/api/v1/studio/projects/project_1') &&
        init?.method === 'PUT' &&
        (init.headers as Headers).get('x-verevon-org-id') === 'org_acme'
      ))).toBe(true)
      expect(fetchMock.mock.calls.some(([url, init]) => (
        String(url).endsWith('/api/v1/studio/projects/project_1/export/social-draft') &&
        init?.method === 'POST' &&
        (init.headers as Headers).get('x-verevon-org-id') === 'org_acme'
      ))).toBe(true)
    })

    // The fetch mock records each call synchronously the instant it's invoked,
    // before that call's own response/JSON-parsing promise chain resolves — so
    // the PUT+POST assertion above can observe both calls made while
    // `exportCurrentProject`'s trailing `setPersistenceMessage` (and its
    // `finally` busy-state reset) are still a few microtask hops away from
    // running. Poll for the persisted message rather than asserting
    // synchronously right after the previous `waitFor` resolves.
    await waitFor(() => {
      expect(screen.getByText(/Sosialt utkast opprettet/)).toBeTruthy()
    })
  })

  it('labels gateway-local Studio persistence as temporary', async () => {
    stubStudioFetch()
    renderWithRouter(() => <StudioPage section="canvas" />)

    await waitFor(() => {
      expect(screen.getByText(/Midlertidig Studio-prosjekt/)).toBeTruthy()
    })
  })
})

function stubStudioFetch() {
  const project = {
    id: 'project_1',
    orgId: 'org_acme',
    ownerUserId: 'user_acme',
    updatedByUserId: 'user_acme',
    title: 'Launch canvas',
    status: 'draft',
    persistence: 'ephemeral',
    blocks: [],
    selectedBlockId: null,
    createdAt: '2026-06-16T10:00:00Z',
    updatedAt: '2026-06-16T10:00:00Z',
  }

  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    if (url.endsWith('/api/v1/me/session-context')) {
      return jsonResponse({
        userId: 'user_acme',
        email: 'team@acme.test',
        name: 'Acme Team',
        orgId: 'org_acme',
        role: 'owner',
        orgs: [{ id: 'org_acme', name: 'Acme', role: 'owner' }],
      })
    }

    if (url.endsWith('/api/v1/studio/projects') && (!init?.method || init.method === 'GET')) {
      return jsonResponse({ projects: [project] })
    }

    if (url.endsWith('/api/v1/studio/projects/project_1') && init?.method === 'PUT') {
      return jsonResponse({ project })
    }

    if (url.endsWith('/api/v1/studio/projects/project_1/export/social-draft') && init?.method === 'POST') {
      return jsonResponse({
        project: {
          ...project,
          socialDraftId: 'social_studio_1',
          socialExportedAt: '2026-06-16T10:10:00Z',
        },
        socialPost: {
          id: 'social_studio_1',
          title: 'Launch canvas',
          body: 'Studio export',
          status: 'draft',
          scheduledAt: null,
          platforms: ['linkedin', 'x'],
          source: { kind: 'campaign', label: 'Launch canvas' },
          approval: { required: true, state: 'not_requested' },
          media: [],
        },
      })
    }

    return new Response(JSON.stringify({ error: { code: 'NOT_FOUND', message: `Unhandled ${url}` } }), {
      status: 404,
      headers: { 'Content-Type': 'application/json' },
    })
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ data }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}
