// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, render, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import SocialOperationsPage from '@/features/social/components/SocialOperationsPage'
import type {
  SocialApprovalItem,
  SocialCalendar,
  SocialCampaign,
  SocialPlatformAdapter,
  SocialPost,
} from '@/shared/api/social-client'

function renderSocialOperations(section: Parameters<typeof SocialOperationsPage>[0]['section']) {
  window.history.pushState(null, '', `/social/${section}`)
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/*all" component={() => <SocialOperationsPage section={section} />} />
    </Router>
  ))
}

function waitForSocial(assertion: () => void) {
  return waitFor(assertion, { timeout: 3000 })
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('SocialOperationsPage', () => {
  it('loads org-scoped account and adapter readiness', async () => {
    stubSocialFetch()

    renderSocialOperations('accounts')

    await waitForSocial(() => expect(screen.getByText('Acme LinkedIn')).toBeTruthy())
    expect(screen.getByText('Sporede leverandører')).toBeTruthy()
    expect(screen.getByText('Kan publisere')).toBeTruthy()
    expect(screen.getByText(/Nødvendige funksjoner er på plass/)).toBeTruthy()
    expect(screen.getByText(/Mangler social.post.write/)).toBeTruthy()

    const fetchMock = vi.mocked(fetch)
    expect(fetchMock.mock.calls.some(([url, init]) =>
      String(url).endsWith('/api/v1/social/calendar') &&
      (init?.headers as Headers).get('x-verevon-org-id') === 'org_acme',
    )).toBe(true)
  })

  it('derives drafts and media blockers from live social posts', async () => {
    stubSocialFetch()

    renderSocialOperations('drafts')

    await waitForSocial(() => expect(screen.getByText('Launch reel')).toBeTruthy())
    expect(screen.getByText(/Mediaførste plattformer trenger et klart bilde eller en video/)).toBeTruthy()

    const metrics = screen.getByLabelText('Utkast nøkkeltall')
    expect(within(metrics).getByText('Trenger media')).toBeTruthy()
    expect(within(metrics).getAllByText('1').length).toBeGreaterThanOrEqual(2)
  })

  it('uses dedicated approvals and campaigns endpoints when they are available', async () => {
    const approvalPost = socialPostFixture({
      id: 'post_legal',
      title: 'Legal review',
      body: 'Launch copy needs legal approval before publishing.',
      platforms: ['linkedin', 'x'],
      source: { kind: 'campaign', label: 'Launch board', href: '/social/calendar' },
    })
    const approvals: SocialApprovalItem[] = [{
      id: 'approval_legal',
      postId: approvalPost.id,
      campaignId: 'campaign_summer',
      state: 'requested',
      dueAt: '2026-06-19T13:00:00.000Z',
      requestedByUserId: 'legal_ops',
      requestedOfUserId: 'approver_1',
      decisionReason: '',
      post: approvalPost,
      createdAt: '2026-06-18T10:00:00.000Z',
      updatedAt: '2026-06-18T10:00:00.000Z',
    }]
    const campaigns: SocialCampaign[] = [{
      id: 'campaign_summer',
      name: 'Summer launch',
      brief: 'Three-channel launch campaign with approved social variants.',
      goal: 'Coordinate launch posts across selected channels.',
      status: 'active',
      platforms: ['linkedin', 'x', 'instagram'],
      startsAt: '2026-06-17T10:00:00.000Z',
      endsAt: '2026-06-24T10:00:00.000Z',
      source: { kind: 'campaign', label: 'Launch board', href: '/social/calendar' },
      ownerUserId: 'growth',
      createdAt: '2026-06-17T10:00:00.000Z',
      updatedAt: '2026-06-17T10:00:00.000Z',
    }]
    stubSocialFetch({ approvals, campaigns })

    renderSocialOperations('approvals')

    await waitForSocial(() => expect(screen.getByText('Legal review')).toBeTruthy())
    expect(screen.getByText(/Manuell gjennomgang kreves/)).toBeTruthy()

    cleanup()
    renderSocialOperations('campaigns')

    await waitForSocial(() => expect(screen.getByText('Summer launch')).toBeTruthy())
    expect(screen.getByText(/Three-channel launch campaign/)).toBeTruthy()

    const fetchMock = vi.mocked(fetch)
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/api/v1/social/approvals'))).toBe(true)
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/api/v1/social/campaigns'))).toBe(true)
  })

  it('shows a graceful pending state when optional social endpoints are unavailable', async () => {
    stubSocialFetch()

    renderSocialOperations('competitors')

    await waitForSocial(() => expect(screen.getByText(/Ingen databaseforankrede konkurrentovervåking-poster ble lastet/)).toBeTruthy())
    expect(screen.queryByText('LinkedIn competitor lane')).toBeNull()
    expect(within(screen.getByLabelText('Konkurrentovervåking nøkkeltall')).getAllByText('0')).toHaveLength(2)
  })
})

function stubSocialFetch(options: {
  approvals?: SocialApprovalItem[]
  campaigns?: SocialCampaign[]
} = {}) {
  const calendar: SocialCalendar = {
    accounts: [
      {
        id: 'acct_linkedin',
        providerKey: 'linkedin',
        label: 'Acme LinkedIn',
        handle: 'linkedin.com/company/acme',
        status: 'connected',
        capabilities: ['social.profile.read', 'social.post.write'],
        accent: '#0a66c2',
      },
      {
        id: 'acct_instagram',
        providerKey: 'instagram',
        label: 'Acme Instagram',
        handle: '@acme',
        status: 'manual_review',
        capabilities: ['social.profile.read', 'social.media.upload'],
        accent: '#d9468f',
      },
    ],
    posts: [
      {
        id: 'post_launch_reel',
        title: 'Launch reel',
        body: 'A short-form launch note for visual channels.',
        status: 'draft',
        scheduledAt: '2026-06-19T13:00:00.000Z',
        platforms: ['instagram'],
        source: { kind: 'campaign', label: 'Launch calendar', href: null },
        approval: { required: true, state: 'not_requested' },
        media: [],
        previews: [],
      },
      {
        id: 'post_linkedin',
        title: 'Customer signal post',
        body: 'Customer signal summary.',
        status: 'scheduled',
        scheduledAt: '2026-06-16T08:30:00.000Z',
        platforms: ['linkedin'],
        source: { kind: 'inbox', label: 'Inbox trend', href: '/inbox?view=social' },
        approval: { required: true, state: 'approved' },
        media: [{ kind: 'image', label: 'Visual', status: 'ready' }],
        previews: [],
      },
    ],
    recommendedWindows: [],
  }
  const adapters: SocialPlatformAdapter[] = [
    {
      providerKey: 'linkedin',
      label: 'LinkedIn',
      mode: 'direct_api',
      endpoint: 'POST /rest/posts',
      maxCharacters: 3000,
      mediaRequired: false,
      requiredCapabilities: ['social.profile.read', 'social.post.write'],
      notes: ['LinkedIn posts API.'],
    },
    {
      providerKey: 'instagram',
      label: 'Instagram',
      mode: 'media_container',
      endpoint: 'POST /{ig-user-id}/media',
      maxCharacters: 2200,
      mediaRequired: true,
      requiredCapabilities: ['social.profile.read', 'social.post.write', 'social.media.upload'],
      notes: ['Instagram media container publish flow.'],
    },
  ]

  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input)
    if (url.endsWith('/api/v1/auth/session')) {
      return jsonResponse({
        user: {
          id: 'user_acme',
          email: 'team@acme.test',
          name: 'Acme Team',
          emailVerified: true,
        },
      })
    }
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
    if (url.endsWith('/api/v1/social/calendar')) return jsonResponse(calendar)
    if (url.endsWith('/api/v1/social/adapters')) return jsonResponse({ adapters })
    if (url.endsWith('/api/v1/social/approvals') && options.approvals) return jsonResponse({ approvals: options.approvals })
    if (url.endsWith('/api/v1/social/campaigns') && options.campaigns) return jsonResponse({ campaigns: options.campaigns })

    return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
  }))
}

function socialPostFixture(overrides: Partial<SocialPost>): SocialPost {
  return {
    id: 'post_fixture',
    title: 'Fixture post',
    body: 'Fixture copy.',
    status: 'draft',
    scheduledAt: '2026-06-19T13:00:00.000Z',
    platforms: ['linkedin'],
    source: { kind: 'manual', label: 'Fixture' },
    approval: { required: true, state: 'requested' },
    media: [],
    previews: [],
    ...overrides,
  }
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify({ data: body }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}
