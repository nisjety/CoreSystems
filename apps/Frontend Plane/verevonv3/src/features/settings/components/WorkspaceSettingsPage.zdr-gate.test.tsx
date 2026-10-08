// @vitest-environment jsdom

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { VerevonWorkspaceSettingsPage } from '@/features/settings/components/WorkspaceSettingsPage'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// The org Zero Data Retention switch is gated on an attested provider. The gateway refuses
// the PATCH (`apps/gateway/src/domains/orgs/settings.rs`); this page must not offer a switch
// that can only fail, and must say why. The catalogue bodies are the gateway suite's own
// cases (one shared file), so what the toggle reads and what the gate decides come from the
// same data. The file is read, never skipped: a missing file fails the suite.

const here = dirname(fileURLToPath(import.meta.url))
const FIXTURE = join(
  here, '..', '..', '..', '..', 'apps', 'gateway', 'tests', 'fixtures', 'zdr-model-catalogue-cases.json',
)

function catalogue(caseName: string): unknown {
  const parsed = JSON.parse(readFileSync(FIXTURE, 'utf8')) as { cases: { name: string; body: unknown }[] }
  const found = parsed.cases.find((entry) => entry.name === caseName)
  if (!found) throw new Error(`fixture case ${caseName} is missing`)
  return found.body
}

const NOTHING_ATTESTED = 'fleet_as_deployed_today_nothing_attested'
const ATTESTED = 'azure_openai_chat_model_attested'

type Catalogue = typeof NOTHING_ATTESTED | typeof ATTESTED | 'down'

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

/** What the page asked the gateway to do, so a test can say what must NOT have happened. */
type Calls = { patches: unknown[]; modelRequests: number }

function stubGateway(options: {
  catalogue: Catalogue
  zdrOn?: boolean
  entitled?: boolean
  patch?: (body: unknown) => Response
}): Calls {
  const calls: Calls = { patches: [], modelRequests: 0 }
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    if (path === '/api/v1/orgs/org_1/settings' && init?.method === 'PATCH') {
      const body = JSON.parse(String(init.body)) as unknown
      calls.patches.push(body)
      return options.patch
        ? options.patch(body)
        : json({ id: 'org_1', metadata: { interactiveRetention: { zdr: (body as { zeroDataRetention: boolean }).zeroDataRetention } } })
    }
    if (path === '/api/v1/orgs/org_1') {
      return json({
        id: 'org_1',
        name: 'Acme',
        metadata: { interactiveRetention: { zdr: options.zdrOn ?? false }, supportAi: { mode: 'review' } },
      })
    }
    if (path === '/api/v1/orgs/org_1/entitlements') {
      return json({
        organization_id: 'org_1',
        entitlements: [{ key: 'feature.zero_data_retention', enabled: options.entitled ?? true }],
      })
    }
    if (path === '/api/v1/models') {
      calls.modelRequests += 1
      return options.catalogue === 'down'
        ? json({ error: { code: 'upstream_unavailable', message: 'The upstream service is unavailable.' } }, 502)
        : json(catalogue(options.catalogue))
    }
    if (path.startsWith('/api/v1/audit')) return json({ data: [], meta: { count: 0 }, error: null })
    return json({})
  }))
  return calls
}

function signInAsOrgOwner() {
  setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'owner' })
  flush()
}

function zdrSwitch(): HTMLElement {
  return screen.getByRole('switch', { name: /^zero data retention/i })
}

function isDisabled(element: HTMLElement): boolean {
  return element.hasAttribute('disabled')
    || element.getAttribute('aria-disabled') === 'true'
    || element.getAttribute('data-disabled') !== null
}

async function renderSecuritySection(calls: Calls) {
  render(() => <VerevonWorkspaceSettingsPage section="org-security" />)
  // The switch exists from the first paint, disabled; it settles once the posture,
  // the entitlement and the catalogue have answered. Every test below asserts a
  // specific settled state (a note, an enabled switch) before it relies on one.
  await waitFor(() => expect(calls.modelRequests).toBeGreaterThan(0))
}

const NO_PROVIDER_NOTE = /ingen AI-leverandør i dette miljøet har en registrert nullbevaringsgodkjenning/i
const CATALOGUE_DOWN_NOTE = /Kunne ikke kontrollere hvilke leverandører som har registrert ZDR-godkjenning/i
const LAPSED_BANNER = /Zero Data Retention er på, men ingen AI-leverandør har en registrert nullbevaringsgodkjenning/i

describe('org ZDR switch on the security page is gated on an attested provider', () => {
  beforeEach(() => signInAsOrgOwner())
  afterEach(() => {
    cleanup()
    clearSession()
    vi.unstubAllGlobals()
  })

  it('is disabled with the reason while no provider is attested, and a click writes nothing', async () => {
    const calls = stubGateway({ catalogue: NOTHING_ATTESTED, entitled: true })

    await renderSecuritySection(calls)

    expect(await screen.findByText(NO_PROVIDER_NOTE)).toBeTruthy()
    expect(isDisabled(zdrSwitch())).toBe(true)
    expect(zdrSwitch().getAttribute('aria-checked')).toBe('false')
    fireEvent.click(zdrSwitch())
    flush()
    expect(calls.patches).toEqual([])
    // The plan note is a different reason and must not show for an entitled org.
    expect(screen.queryByText(/premiumplaner/i)).toBeNull()
  })

  it('is enabled once a chat provider is attested, and turning it on sends the PATCH', async () => {
    const calls = stubGateway({ catalogue: ATTESTED, entitled: true })

    await renderSecuritySection(calls)

    await waitFor(() => expect(isDisabled(zdrSwitch())).toBe(false))
    expect(screen.queryByText(NO_PROVIDER_NOTE)).toBeNull()
    expect(screen.queryByText(CATALOGUE_DOWN_NOTE)).toBeNull()
    fireEvent.click(zdrSwitch())
    await waitFor(() => expect(calls.patches).toEqual([{ zeroDataRetention: true }]))
    await waitFor(() => expect(zdrSwitch().getAttribute('aria-checked')).toBe('true'))
  })

  it('keeps the plan lock and the attestation lock as two separate reasons', async () => {
    const calls = stubGateway({ catalogue: ATTESTED, entitled: false })

    await renderSecuritySection(calls)

    // Attested, but the plan does not include ZDR: still locked, for the plan reason only.
    expect(await screen.findByText(/premiumplaner/i)).toBeTruthy()
    expect(isDisabled(zdrSwitch())).toBe(true)
    expect(screen.queryByText(NO_PROVIDER_NOTE)).toBeNull()
  })

  it('fails closed when the catalogue cannot be read: disabled, says it could not check, not "none attested"', async () => {
    const calls = stubGateway({ catalogue: 'down', entitled: true })

    await renderSecuritySection(calls)

    expect(await screen.findByText(CATALOGUE_DOWN_NOTE)).toBeTruthy()
    expect(screen.queryByText(NO_PROVIDER_NOTE)).toBeNull()
    expect(isDisabled(zdrSwitch())).toBe(true)
    fireEvent.click(zdrSwitch())
    flush()
    expect(calls.patches).toEqual([])
  })

  it('keeps the switch usable and shows a banner when ZDR is on but attestation has lapsed', async () => {
    // The attestation expires at its REVIEW_BY date. An org that switched ZDR on while
    // a provider was attested must be able to see what happened, and to switch it off.
    const calls = stubGateway({ catalogue: NOTHING_ATTESTED, zdrOn: true, entitled: true })

    await renderSecuritySection(calls)

    const banner = await screen.findByText(LAPSED_BANNER)
    expect(banner.getAttribute('role')).toBe('alert')
    expect(isDisabled(zdrSwitch())).toBe(false)
    expect(zdrSwitch().getAttribute('aria-checked')).toBe('true')
    // Turning it OFF never needs an attested provider.
    expect(screen.queryByText(NO_PROVIDER_NOTE)).toBeNull()
    fireEvent.click(zdrSwitch())
    await waitFor(() => expect(calls.patches).toEqual([{ zeroDataRetention: false }]))
    await waitFor(() => expect(zdrSwitch().getAttribute('aria-checked')).toBe('false'))
  })

  it('shows no lapse banner while the catalogue is merely unreachable', async () => {
    const calls = stubGateway({ catalogue: 'down', zdrOn: true, entitled: true })

    await renderSecuritySection(calls)

    await waitFor(() => expect(zdrSwitch().getAttribute('aria-checked')).toBe('true'))
    expect(screen.queryByText(LAPSED_BANNER)).toBeNull()
    expect(isDisabled(zdrSwitch())).toBe(false)
  })

  it('shows no banner when ZDR is on and a provider is attested', async () => {
    const calls = stubGateway({ catalogue: ATTESTED, zdrOn: true, entitled: true })

    await renderSecuritySection(calls)

    await waitFor(() => expect(zdrSwitch().getAttribute('aria-checked')).toBe('true'))
    expect(screen.queryByText(LAPSED_BANNER)).toBeNull()
  })

  it('reverts and explains when the gateway refuses after the page believed a provider was attested', async () => {
    // The catalogue can change between the page load and the click (the attestation lapses).
    const calls = stubGateway({
      catalogue: ATTESTED,
      entitled: true,
      patch: () => json({
        error: { code: 'zdr_no_attested_provider', message: 'Zero Data Retention cannot be turned on: no AI provider ...' },
      }, 409),
    })

    await renderSecuritySection(calls)
    await waitFor(() => expect(isDisabled(zdrSwitch())).toBe(false))

    fireEvent.click(zdrSwitch())

    await waitFor(() => expect(calls.patches).toEqual([{ zeroDataRetention: true }]))
    // The page's own Norwegian copy, never the gateway's English ops text.
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toMatch(/Fortsett uten, og slå det på i innstillingene når en leverandør er godkjent/)
    expect(screen.queryByText(/cannot be turned on: no AI provider/)).toBeNull()
    await waitFor(() => expect(zdrSwitch().getAttribute('aria-checked')).toBe('false'))
    // And the switch now agrees with the gateway instead of inviting a second try.
    await waitFor(() => expect(isDisabled(zdrSwitch())).toBe(true))
  })

  it('explains a catalogue outage that only shows up at click time, and locks the switch until a reload', async () => {
    const calls = stubGateway({
      catalogue: ATTESTED,
      entitled: true,
      patch: () => json({
        error: { code: 'zdr_gate_unavailable', message: 'Zero Data Retention cannot be turned on right now: ...' },
      }, 503),
    })

    await renderSecuritySection(calls)
    await waitFor(() => expect(isDisabled(zdrSwitch())).toBe(false))

    fireEvent.click(zdrSwitch())

    await waitFor(() => expect(calls.patches).toHaveLength(1))
    expect(await screen.findByText(/Prøv igjen om litt/i)).toBeTruthy()
    await waitFor(() => expect(zdrSwitch().getAttribute('aria-checked')).toBe('false'))
    await waitFor(() => expect(isDisabled(zdrSwitch())).toBe(true))
  })
})
