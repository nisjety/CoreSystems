// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { OrgSpacePolicySection } from '@/features/settings/components/OrgSpacePolicySection'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// WS03-S6: the org-admin Space policy console. Control owns the policy and the role check; this
// section only draws it and sends what an admin chose. Modelled on the ZDR gate test: a stubbed
// gateway records every call so a test can say what must NOT have been sent.

const KEYS = [
  'thread_create',
  'thread_read',
  'retrieval_read',
  'import_write',
  'agent_action',
  'schedule_fire',
  'sandbox_capability',
  'process_registry',
] as const

const ENABLED_BY_DEFAULT = new Set(['thread_create', 'thread_read'])

function entitlement(key: string, overrides: Record<string, unknown> = {}) {
  return {
    key,
    enabled: ENABLED_BY_DEFAULT.has(key),
    effective: ENABLED_BY_DEFAULT.has(key),
    zdr: 'compatible',
    zdr_blocked: false,
    decisions: [],
    ...overrides,
  }
}

function presetRow(id: string, enabled: string[], extra: Record<string, unknown> = {}) {
  return {
    id,
    entitlements: Object.fromEntries(KEYS.map((key) => [key, enabled.includes(key)])),
    available: true,
    blocked_by: [],
    ...extra,
  }
}

function policyView(overrides: Record<string, unknown> = {}) {
  return {
    org_id: 'org_1',
    state: 'provisioned',
    capabilities: { write: true, write_denied_reason: null },
    revisions: { policy: 1, entitlement: 5 },
    updated_at: '2026-10-27T09:00:00Z',
    attestation: {
      privacy_policy_ref: 'policy-1',
      purpose: 'collaboration',
      lawful_basis: 'contract',
      privacy_class: 'internal',
      third_party_processing_allowed: false,
      retention_class: 'standard',
      residency: 'eu',
      deletion_scope: 'org',
      zero_data_retention: false,
    },
    entitlements: KEYS.map((key) => entitlement(key)),
    presets: [
      presetRow('locked', []),
      presetRow('conversation', ['thread_create', 'thread_read']),
      presetRow('conversation_knowledge', ['thread_create', 'thread_read', 'retrieval_read']),
    ],
    matches_preset: 'conversation',
    active_spaces: 3,
    last_change: null,
    ...overrides,
  }
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

type Put = { key: string; revision: number; body: Record<string, unknown> }

type Gateway = {
  puts: Put[]
  policyGets: number
  paths: string[]
  /** What the next policy GET answers; the test may replace it. */
  policy: () => Response
  /** What the next PUT answers. */
  put: (body: Record<string, unknown>, attempt: number) => Response | Promise<Response>
  orgZdr: () => Response
}

function stubGateway(overrides: Partial<Gateway> = {}): Gateway {
  const gateway: Gateway = {
    puts: [],
    policyGets: 0,
    paths: [],
    policy: () => json({ data: policyView() }),
    put: () => json({ data: { changed: true, replayed: false, spaces_fenced: 3, policy: policyView() } }),
    orgZdr: () => json({ id: 'org_1', metadata: { interactiveRetention: { zdr: false } } }),
    ...overrides,
  }
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    gateway.paths.push(path)
    if (path.endsWith('/space-policy/entitlements') && init?.method === 'PUT') {
      const body = JSON.parse(String(init.body)) as Record<string, unknown>
      gateway.puts.push({
        key: String(body.idempotency_key),
        revision: Number(body.expected_entitlement_revision),
        body,
      })
      return gateway.put(body, gateway.puts.length)
    }
    if (path.includes('/space-policy/events')) return json({ data: [] })
    if (path.endsWith('/space-policy')) {
      gateway.policyGets += 1
      return gateway.policy()
    }
    if (path === '/api/v1/orgs/org_1') return gateway.orgZdr()
    return json({})
  }))
  return gateway
}

function signIn(role: 'owner' | 'admin' | 'member') {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role })
  flush()
}

function switchFor(label: string): HTMLInputElement {
  return screen.getByRole('switch', { name: label }) as HTMLInputElement
}

function isDisabled(element: HTMLElement): boolean {
  return element.hasAttribute('disabled') || element.getAttribute('aria-disabled') === 'true' || element.getAttribute('data-disabled') !== null
}

const WRITE_LABEL = 'Skrive i rom og starte samtaler'
const KNOWLEDGE_LABEL = 'Bruke kunnskap i rom'
const SANDBOX_LABEL = 'Agent-datamaskin (sandkasse)'

async function renderLoaded() {
  render(() => <OrgSpacePolicySection />)
  await screen.findByRole('switch', { name: WRITE_LABEL })
}

describe('OrgSpacePolicySection', () => {
  beforeEach(() => signIn('owner'))
  afterEach(() => {
    cleanup()
    clearSession()
    vi.unstubAllGlobals()
  })

  describe('states', () => {
    it('shows deny-by-default and no controls when the org is not provisioned', async () => {
      stubGateway({
        policy: () => json({
          data: policyView({
            state: 'not_provisioned',
            attestation: null,
            entitlements: [],
            presets: [],
            matches_preset: null,
            active_spaces: 0,
          }),
        }),
      })
      render(() => <OrgSpacePolicySection />)
      expect(await screen.findByTestId('space-policy-not-provisioned')).toBeTruthy()
      expect(screen.queryAllByRole('switch')).toHaveLength(0)
      expect(screen.queryByRole('button', { name: 'Velg' })).toBeNull()
      expect(screen.queryByRole('button', { name: /Lagre endringer/ })).toBeNull()
    })

    it('is read-only when capabilities.write is false', async () => {
      stubGateway({
        policy: () => json({ data: policyView({ capabilities: { write: false, write_denied_reason: 'admin_writes_disabled' } }) }),
      })
      await renderLoaded()
      expect(screen.getByTestId('space-policy-read-only')).toBeTruthy()
      expect(screen.getAllByRole('switch')).toHaveLength(KEYS.length)
      expect(screen.getAllByRole('switch').every(isDisabled)).toBe(true)
      for (const button of screen.getAllByRole('button', { name: 'Velg' })) expect(isDisabled(button)).toBe(true)
    })

    it('disables a ZDR-blocked switch that is off, and a preset that is not available', async () => {
      stubGateway({
        policy: () => json({
          data: policyView({
            entitlements: KEYS.map((key) => (key === 'sandbox_capability'
              ? entitlement(key, { zdr: 'zdr_unproven', zdr_blocked: true })
              : entitlement(key))),
            presets: [
              presetRow('locked', []),
              presetRow('conversation_knowledge', ['thread_create', 'thread_read', 'retrieval_read'], {
                available: false,
                blocked_by: ['retrieval_read'],
              }),
            ],
          }),
        }),
      })
      await renderLoaded()
      const sandbox = switchFor(SANDBOX_LABEL)
      expect(isDisabled(sandbox)).toBe(true)
      fireEvent.click(sandbox)
      flush()
      expect(sandbox.getAttribute('aria-checked')).toBe('false')
      expect(screen.getAllByText('Kan ikke slås på mens nulloppbevaring for rom er på.').length).toBeGreaterThan(0)

      const choose = screen.getAllByRole('button', { name: 'Velg' })
      expect(isDisabled(choose[0] as HTMLElement)).toBe(false)
      expect(isDisabled(choose[1] as HTMLElement)).toBe(true)
      expect(screen.getByText('Ikke tilgjengelig mens nulloppbevaring for rom er på.')).toBeTruthy()
    })

    it('shows the not-switched-on note on 404 space_policy_admin_disabled', async () => {
      stubGateway({
        policy: () => json({ error: { code: 'space_policy_admin_disabled', message: 'off' } }, 404),
      })
      render(() => <OrgSpacePolicySection />)
      expect(await screen.findByText('Rom-policy er ikke slått på for denne installasjonen.')).toBeTruthy()
      expect(screen.queryByRole('alert')).toBeNull()
      expect(screen.queryAllByRole('switch')).toHaveLength(0)
    })

    it('shows an error for any other load failure, never the console', async () => {
      stubGateway({ policy: () => json({ error: { code: 'internal', message: 'boom' } }, 500) })
      render(() => <OrgSpacePolicySection />)
      expect((await screen.findByRole('alert')).textContent).toContain('Kunne ikke laste rom-policy.')
      expect(screen.queryAllByRole('switch')).toHaveLength(0)
    })

    it('says "could not check" when the org ZDR read fails, never a guess', async () => {
      stubGateway({ orgZdr: () => json({ error: { code: 'internal', message: 'boom' } }, 500) })
      await renderLoaded()
      await waitFor(() => expect(screen.getByText('Kunne ikke sjekke')).toBeTruthy())
    })

    it('shows the org ZDR state and flags a mismatch with the attestation', async () => {
      stubGateway({ orgZdr: () => json({ id: 'org_1', metadata: { interactiveRetention: { zdr: true } } }) })
      await renderLoaded()
      expect(await screen.findByText(/Avviker fra rommenes personvernerklæring/)).toBeTruthy()
    })
  })

  describe('who may see it', () => {
    it('does not ask the gateway for anything when the user is not an admin', async () => {
      cleanup()
      clearSession()
      signIn('member')
      const gateway = stubGateway()
      render(() => <OrgSpacePolicySection />)
      expect(await screen.findByText('Bare administratorer kan se eller endre rom-policy.')).toBeTruthy()
      await new Promise((resolve) => setTimeout(resolve, 0))
      expect(gateway.paths.filter((path) => path.includes('space-policy'))).toEqual([])
      expect(gateway.paths).toEqual([])
      expect(screen.queryAllByRole('switch')).toHaveLength(0)
    })

    it('loads for an org admin too', async () => {
      cleanup()
      clearSession()
      signIn('admin')
      stubGateway()
      await renderLoaded()
      expect(switchFor(WRITE_LABEL)).toBeTruthy()
    })
  })

  describe('saving', () => {
    it('picking a preset then saving sends only that preset id', async () => {
      const gateway = stubGateway()
      await renderLoaded()
      const choose = screen.getAllByRole('button', { name: 'Velg' })
      fireEvent.click(choose[2] as HTMLElement)
      flush()
      expect((screen.getByRole('button', { name: 'Valgt' }) as HTMLButtonElement).getAttribute('aria-pressed')).toBe('true')
      // The draft turns knowledge on; nothing is turned off, so there is no confirmation step.
      fireEvent.click(await screen.findByRole('button', { name: 'Lagre endringer' }))
      await waitFor(() => expect(gateway.puts).toHaveLength(1))
      const sent = gateway.puts[0] as Put
      expect(sent.body.preset_id).toBe('conversation_knowledge')
      expect(sent.body).not.toHaveProperty('entitlements')
      expect(sent.revision).toBe(5)
      expect(sent.key.length).toBeGreaterThanOrEqual(8)
      for (const key of ['org_id', 'actor', 'role']) expect(sent.body).not.toHaveProperty(key)
      expect(await screen.findByText(/Lagret\. 3 aktive rom ble oppdatert\./)).toBeTruthy()
    })

    it('turning a bit off arms the confirm button first, then sends all eight keys', async () => {
      const gateway = stubGateway()
      await renderLoaded()
      fireEvent.click(switchFor(WRITE_LABEL))
      flush()
      const save = await screen.findByRole('button', { name: 'Lagre endringer' })
      fireEvent.click(save)
      flush()

      // Armed: the consequence is on screen and nothing has been sent.
      expect(await screen.findByText(/Slår av: Skrive i rom og starte samtaler/)).toBeTruthy()
      expect(gateway.puts).toHaveLength(0)

      fireEvent.click(screen.getByRole('button', { name: 'Bekreft at du slår av rettigheter' }))
      await waitFor(() => expect(gateway.puts).toHaveLength(1))
      const sent = gateway.puts[0] as Put
      expect(sent.body).not.toHaveProperty('preset_id')
      const entitlements = sent.body.entitlements as Record<string, boolean>
      expect(Object.keys(entitlements).sort()).toEqual([...KEYS].sort())
      expect(entitlements.thread_create).toBe(false)
      expect(entitlements.thread_read).toBe(true)
    })

    it('turning a bit on saves without a confirmation step', async () => {
      const gateway = stubGateway()
      await renderLoaded()
      fireEvent.click(switchFor(KNOWLEDGE_LABEL))
      flush()
      fireEvent.click(await screen.findByRole('button', { name: 'Lagre endringer' }))
      await waitFor(() => expect(gateway.puts).toHaveLength(1))
      expect((gateway.puts[0]?.body.entitlements as Record<string, boolean>).retrieval_read).toBe(true)
    })

    it('Discard drops the draft without sending anything', async () => {
      const gateway = stubGateway()
      await renderLoaded()
      fireEvent.click(switchFor(KNOWLEDGE_LABEL))
      flush()
      fireEvent.click(await screen.findByRole('button', { name: 'Forkast' }))
      flush()
      expect(screen.queryByRole('button', { name: 'Lagre endringer' })).toBeNull()
      expect(switchFor(KNOWLEDGE_LABEL).getAttribute('aria-checked')).toBe('false')
      expect(gateway.puts).toHaveLength(0)
    })

    it('on a revision conflict reloads, keeps the draft, and the next save uses a new key and revision', async () => {
      let revision = 5
      const gateway = stubGateway({
        policy: () => json({ data: policyView({ revisions: { policy: 1, entitlement: revision } }) }),
        put: (_body, attempt) => {
          if (attempt === 1) {
            revision = 6
            return json({ error: { code: 'space_policy_revision_conflict', message: 'changed' } }, 409)
          }
          return json({ data: { changed: true, replayed: false, spaces_fenced: 1, policy: policyView() } })
        },
      })
      await renderLoaded()
      fireEvent.click(switchFor(KNOWLEDGE_LABEL))
      flush()
      fireEvent.click(await screen.findByRole('button', { name: 'Lagre endringer' }))
      await waitFor(() => expect(gateway.puts).toHaveLength(1))

      expect((await screen.findByRole('alert')).textContent).toContain('Innstillingen ble endret av noen andre')
      await waitFor(() => expect(gateway.policyGets).toBeGreaterThanOrEqual(2))
      // The draft is still there: the switch is still on and Save is still offered.
      expect(switchFor(KNOWLEDGE_LABEL).getAttribute('aria-checked')).toBe('true')

      fireEvent.click(await screen.findByRole('button', { name: 'Lagre endringer' }))
      await waitFor(() => expect(gateway.puts).toHaveLength(2))
      expect(gateway.puts[1]?.key).not.toBe(gateway.puts[0]?.key)
      expect(gateway.puts[0]?.revision).toBe(5)
      expect(gateway.puts[1]?.revision).toBe(6)
    })

    it('a retry after a network error reuses the idempotency key', async () => {
      const gateway = stubGateway({
        put: (_body, attempt) => {
          if (attempt === 1) throw new TypeError('Failed to fetch')
          return json({ data: { changed: true, replayed: true, spaces_fenced: 0, policy: policyView() } })
        },
      })
      await renderLoaded()
      fireEvent.click(switchFor(KNOWLEDGE_LABEL))
      flush()
      fireEvent.click(await screen.findByRole('button', { name: 'Lagre endringer' }))
      await waitFor(() => expect(gateway.puts).toHaveLength(1))
      await screen.findByRole('alert')

      fireEvent.click(await screen.findByRole('button', { name: 'Lagre endringer' }))
      await waitFor(() => expect(gateway.puts).toHaveLength(2))
      expect(gateway.puts[1]?.key).toBe(gateway.puts[0]?.key)
      expect(gateway.puts[1]?.body).toEqual(gateway.puts[0]?.body)
    })

    it('a further edit after a failed save starts a new key', async () => {
      const gateway = stubGateway({
        put: (_body, attempt) => {
          if (attempt === 1) throw new TypeError('Failed to fetch')
          return json({ data: { changed: true, replayed: false, spaces_fenced: 0, policy: policyView() } })
        },
      })
      await renderLoaded()
      fireEvent.click(switchFor(KNOWLEDGE_LABEL))
      flush()
      fireEvent.click(await screen.findByRole('button', { name: 'Lagre endringer' }))
      await waitFor(() => expect(gateway.puts).toHaveLength(1))
      await screen.findByRole('alert')

      fireEvent.click(switchFor('Importere filer og kilder'))
      flush()
      fireEvent.click(await screen.findByRole('button', { name: 'Lagre endringer' }))
      await waitFor(() => expect(gateway.puts).toHaveLength(2))
      expect(gateway.puts[1]?.key).not.toBe(gateway.puts[0]?.key)
    })

    it('names the keys a ZDR conflict refused', async () => {
      stubGateway({
        put: () => json({
          error: { code: 'space_policy_zdr_conflict', message: 'x', details: { keys: ['sandbox_capability'] } },
        }, 409),
      })
      await renderLoaded()
      fireEvent.click(switchFor(KNOWLEDGE_LABEL))
      flush()
      fireEvent.click(await screen.findByRole('button', { name: 'Lagre endringer' }))
      expect((await screen.findByRole('alert')).textContent).toContain('Agent-datamaskin (sandkasse)')
    })
  })
})
