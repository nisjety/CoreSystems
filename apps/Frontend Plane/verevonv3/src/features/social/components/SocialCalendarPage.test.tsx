// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import SocialCalendarPage from '@/features/social/components/SocialCalendarPage'

function jsonResponse(data: unknown, status = 200) {
  return new Response(JSON.stringify({ data }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function renderCalendar() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <SocialCalendarPage /> }],
    history: memoryHistory('/social/calendar'),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('SocialCalendarPage', () => {
  it('routes an unapproved post to human review instead of exposing publish or schedule controls', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/session')) {
        return jsonResponse({ user: { id: 'user_1', email: 'team@verevon.test', name: 'Verevon Team', emailVerified: true } })
      }
      if (url.endsWith('/api/v1/me/session-context')) {
        return jsonResponse({
          userId: 'user_1', email: 'team@verevon.test', name: 'Verevon Team', orgId: 'org_1', role: 'owner',
          orgs: [{ id: 'org_1', name: 'Verevon', role: 'owner' }],
        })
      }
      if (url.endsWith('/api/v1/social/calendar')) {
        return jsonResponse({
          accounts: [],
          recommendedWindows: [{ id: 'window_1', label: 'Tuesday morning', startsAt: '2026-08-04T09:00:00.000Z', reason: 'Audience peak' }],
          posts: [{
            id: 'social_post_1', title: 'Awaiting review', body: 'Body', status: 'pending_approval',
            scheduledAt: '2026-08-04T09:00:00.000Z', platforms: ['linkedin'], source: { kind: 'manual', label: 'Manual' },
            approval: { required: true, state: 'pending' }, media: [],
          }],
        })
      }
      return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderCalendar()

    const review = await screen.findByRole('link', { name: /gjennomgå godkjenning|review approval/i })
    expect(review.getAttribute('href')).toBe('/social/approvals')
    expect(screen.queryByRole('button', { name: /publiser med godkjenning|publish with approval/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /tuesday morning/i })).toBeNull()
  })

  it('opens on the live month and anchors a new draft to the live clock', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-08-03T14:27:00.000Z'))
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/session')) {
        return jsonResponse({ user: { id: 'user_1', email: 'team@verevon.test', name: 'Verevon Team', emailVerified: true } })
      }
      if (url.endsWith('/api/v1/me/session-context')) {
        return jsonResponse({
          userId: 'user_1', email: 'team@verevon.test', name: 'Verevon Team', orgId: 'org_1', role: 'owner',
          orgs: [{ id: 'org_1', name: 'Verevon', role: 'owner' }],
        })
      }
      if (url.endsWith('/api/v1/social/calendar')) return jsonResponse({ accounts: [], posts: [], recommendedWindows: [] })
      if (url.endsWith('/api/v1/social/posts') && init?.method === 'POST') {
        const body = JSON.parse(String(init.body)) as { scheduledAt: string }
        return jsonResponse({
          post: {
            id: 'social_post_1', title: 'Customer insight post', body: 'Body', status: 'draft',
            scheduledAt: body.scheduledAt, platforms: ['linkedin', 'x'], source: { kind: 'manual', label: 'Manual' },
            approval: { required: true, state: 'not_requested' }, media: [],
          },
        })
      }
      return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderCalendar()

    expect(await screen.findByRole('heading', { name: 'August 2026' })).toBeTruthy()
    await vi.runAllTimersAsync()
    const createDraft = screen.getByRole('button', { name: /opprett utkast|create draft/i })
    expect(createDraft.hasAttribute('disabled')).toBe(false)
    fireEvent.click(createDraft)

    await vi.runAllTicks()
    await Promise.resolve()
    const createCall = fetchMock.mock.calls.find(([input, init]) => String(input).endsWith('/api/v1/social/posts') && init?.method === 'POST')
    expect(createCall).toBeTruthy()
    expect(JSON.parse(String(createCall?.[1]?.body)).scheduledAt).toBe('2026-08-03T14:27:00.000Z')
  })
})
