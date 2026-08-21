// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import FinetuneJobsPage from '@/features/finetune/components/FinetuneJobsPage'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

const runningJob = {
  job_id: 'job_1',
  org_id: 'org_1',
  agent_id: 'support-triage-agent',
  base_model: 'gpt-4o-mini',
  azure_file_id: 'file_1',
  azure_job_id: 'azure_job_1',
  fine_tuned_model: '',
  deployment_name: '',
  status: 'running',
  error_message: '',
  hyperparameters: '{}',
  training_example_count: 120,
  estimated_cost_usd: 4.2,
  actual_cost_usd: 0,
  created_by: 'user_1',
  created_at: '2026-08-01T00:00:00.000Z',
  updated_at: '2026-08-01T00:00:00.000Z',
  completed_at: null,
}

function renderPage() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(() => (
    <QueryClientProvider client={queryClient}>
      <FinetuneJobsPage />
    </QueryClientProvider>
  ))
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('FinetuneJobsPage', () => {
  it('reconciles a false-failure 502 by checking whether the fine-tune job was actually cancelled', async () => {
    setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'owner' })
    flush()
    let cancelled = false
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/v1/models') return jsonResponse({ models: [] })
      if (url === '/api/v1/finetune/jobs' && (!init || init.method === undefined)) {
        return jsonResponse({ jobs: [{ ...runningJob, status: cancelled ? 'cancelled' : 'running' }] })
      }
      if (url === '/api/v1/finetune/jobs/job_1' && init?.method === 'DELETE') {
        // The cancellation reaches the backend and is durably recorded, but
        // the response itself is lost to a transient gateway error.
        cancelled = true
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      return jsonResponse({})
    })
    vi.stubGlobal('fetch', fetchMock)

    renderPage()

    expect(await screen.findByText('support-triage-agent')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /^avbryt$/i }))

    await waitFor(() => expect(screen.getByText(/^cancelled$/i)).toBeTruthy())
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows a real failure when cancelling a fine-tune job genuinely did not go through', async () => {
    setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/v1/models') return jsonResponse({ models: [] })
      if (url === '/api/v1/finetune/jobs' && (!init || init.method === undefined)) {
        // Still running — the cancellation genuinely didn't land.
        return jsonResponse({ jobs: [runningJob] })
      }
      if (url === '/api/v1/finetune/jobs/job_1' && init?.method === 'DELETE') {
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      return jsonResponse({})
    })
    vi.stubGlobal('fetch', fetchMock)

    renderPage()

    expect(await screen.findByText('support-triage-agent')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /^avbryt$/i }))

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByText(/^running$/i)).toBeTruthy()
  })
})
