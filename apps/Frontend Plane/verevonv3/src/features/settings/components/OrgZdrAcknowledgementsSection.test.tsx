// @vitest-environment jsdom

import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { OrgZdrAcknowledgementItem, OrgZdrAcknowledgements } from '@/shared/api/connection-zdr-client'
import { I18nProvider } from '@/shared/i18n'
import { en } from '@/shared/i18n/en'
import { localeStorageKey } from '@/shared/i18n/locales'
import { OrgZdrAcknowledgementsSection } from './OrgZdrAcknowledgementsSection'

// ZDR Z-21b: the organisation's record shows an acknowledgement whose text is below the minimum version as superseded,
// never as active, and counts it apart from the acknowledgements in force.

const client = vi.hoisted(() => ({
  listOrgZdrAcknowledgements: vi.fn(),
}))

vi.mock('@/shared/api/connection-zdr-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/connection-zdr-client')>()),
  listOrgZdrAcknowledgements: client.listOrgZdrAcknowledgements,
}))

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({
    status: 'authenticated',
    user: { id: 'user-1', role: 'member' },
    activeOrg: { id: 'org-1', name: 'Test', role: 'owner' },
    permissions: [],
  }),
}))

function row(overrides: Partial<OrgZdrAcknowledgementItem>): OrgZdrAcknowledgementItem {
  return {
    id: 'ack-1',
    connectionId: 'conn-1',
    userId: 'member-1',
    providerKey: 'openai-codex-subscription',
    capability: 'not_supported',
    textVersion: 'v1',
    locale: 'no',
    acknowledgedVia: 'connect',
    acknowledgedAt: '2026-11-01T10:00:00Z',
    revokedAt: null,
    revokeReason: null,
    status: 'active',
    ...overrides,
  }
}

function listing(items: OrgZdrAcknowledgementItem[], counts: OrgZdrAcknowledgements['counts']): OrgZdrAcknowledgements {
  return { items, counts, truncated: false }
}

beforeEach(() => {
  vi.stubEnv('VITE_ZDR_WAIVER_UI', 'true')
  window.localStorage.setItem(localeStorageKey, 'en')
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  vi.clearAllMocks()
})

describe('VITE_ZDR_WAIVER_UI: the organisation record of a text below the minimum', () => {
  it('labels the superseded row superseded, not active, and counts it apart from the rows in force', async () => {
    client.listOrgZdrAcknowledgements.mockResolvedValue(
      listing(
        [
          row({ id: 'ack-old', userId: 'member-old', textVersion: 'v1', status: 'superseded' }),
          row({ id: 'ack-new', userId: 'member-new', textVersion: 'v2', status: 'active' }),
        ],
        { active: 1, superseded: 1, revoked: 0 },
      ),
    )

    render(() => (
      <I18nProvider>
        <OrgZdrAcknowledgementsSection />
      </I18nProvider>
    ))

    expect(await screen.findByText(en.zdr.waiver.admin.counts(1, 1, 0))).toBeTruthy()
    expect(screen.getByText(/Status: Superseded/)).toBeTruthy()
    expect(screen.getByText(/Status: Active/)).toBeTruthy()
    expect(screen.queryByText(/Status: Withdrawn/)).toBeNull()
  })
})
