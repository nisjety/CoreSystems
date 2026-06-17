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

  it('renders the canvas and can add an editable text block', () => {
    renderWithRouter(() => <StudioPage section="canvas" />)

    expect(screen.getByText('Launch canvas')).toBeTruthy()
    expect(screen.getByLabelText('Studio canvas workspace')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Add Text block' }))

    expect(screen.getAllByText('Text note 2').length).toBeGreaterThanOrEqual(1)
    fireEvent.input(screen.getByDisplayValue('Text note 2'), {
      target: { value: 'Launch proof point' },
    })
    expect(screen.getAllByText('Launch proof point').length).toBeGreaterThanOrEqual(1)
  })

  it('duplicates, deletes, and recovers from an empty canvas', () => {
    renderWithRouter(() => <StudioPage section="canvas" />)

    fireEvent.click(screen.getByRole('button', { name: 'Duplicate selected block' }))

    expect(screen.getByDisplayValue('Ava Berg copy')).toBeTruthy()

    for (let index = 0; index < 7; index += 1) {
      fireEvent.click(screen.getByRole('button', { name: 'Delete selected block' }))
    }

    expect(screen.getByText('Start a Studio board')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Text' }))

    expect(screen.getByDisplayValue('Text note 1')).toBeTruthy()
  })

  it('saves the loaded Studio project before exporting it to a Social draft', async () => {
    const fetchMock = stubStudioFetch()
    renderWithRouter(() => <StudioPage section="canvas" />)

    await waitFor(() => {
      expect((screen.getByRole('button', { name: 'Send to drafts' }) as HTMLButtonElement).disabled).toBe(false)
    })

    fireEvent.click(screen.getByRole('button', { name: 'Send to drafts' }))

    await waitFor(() => {
      expect(fetchMock.mock.calls.some(([url, init]) => (
        String(url).endsWith('/api/v1/studio/projects/project_1') &&
        init?.method === 'PUT' &&
        (init.headers as Headers).get('x-velion-org-id') === 'org_acme'
      ))).toBe(true)
      expect(fetchMock.mock.calls.some(([url, init]) => (
        String(url).endsWith('/api/v1/studio/projects/project_1/export/social-draft') &&
        init?.method === 'POST' &&
        (init.headers as Headers).get('x-velion-org-id') === 'org_acme'
      ))).toBe(true)
    })

    expect(screen.getByText(/Social draft created/)).toBeTruthy()
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
