// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { Route, Router } from '@solidjs/router'
import type { JSX } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import StudioPage from '@/features/studio/components/StudioPage'

function renderWithRouter(component: () => JSX.Element, path = '/studio/canvas') {
  window.history.pushState(null, '', path)
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/*all" component={component} />
    </Router>
  ))
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

    expect(screen.getAllByText('Text note 1').length).toBeGreaterThanOrEqual(1)
    fireEvent.input(screen.getByDisplayValue('Text note 1'), {
      target: { value: 'Launch proof point' },
    })
    expect(screen.getAllByText('Launch proof point').length).toBeGreaterThanOrEqual(1)
  })

  it('duplicates, deletes, and recovers back to the empty canvas', () => {
    renderWithRouter(() => <StudioPage section="canvas" />)

    // Build up from the empty canvas, then duplicate the added block.
    fireEvent.click(screen.getByRole('button', { name: 'Legg til Tekst-blokk' }))
    fireEvent.click(screen.getByRole('button', { name: 'Dupliser valgt blokk' }))

    expect(screen.getByDisplayValue('Text note 1 copy')).toBeTruthy()

    // Delete both blocks; the empty-state affordance returns.
    fireEvent.click(screen.getByRole('button', { name: 'Slett valgt blokk' }))
    fireEvent.click(screen.getByRole('button', { name: 'Slett valgt blokk' }))

    expect(screen.getByText('Start et Studio-brett')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Tekst' }))

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

    expect(screen.getByText(/Sosialt utkast opprettet/)).toBeTruthy()
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
