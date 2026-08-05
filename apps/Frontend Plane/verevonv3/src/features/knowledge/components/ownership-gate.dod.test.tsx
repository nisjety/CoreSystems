// @vitest-environment jsdom
//
// PR-6 RELEASE-BLOCKING honesty-gate test (the ABSOLUTE RULE guard).
//
// No per-user privacy affordance — the visibility Badge or the ShareDialog —
// may render unless the honesty gate is OPEN. The gate is open only when the
// backend reports `gate_open === true`, i.e. CONTROL_PLANE_ENFORCEMENT === 'strict'
// AND viewer identity is live (computed server-side in the gateway's
// /api/v1/ownership/status). A closed gate represents "enforcement != strict OR
// identity not live" — exactly the condition under which a "Private"/"Shared"
// affordance would be a FALSE-PRIVACY guarantee.
//
// If a future change renders a badge or ShareDialog without consulting the gate,
// these assertions fail.

import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Control the honesty gate per test. vi.mock is hoisted above the component
// imports, so PrivacyBadge/ShareDialog see this mock's isGateOpen().
let gateOpen = false
vi.mock('@/shared/context/ownership-gate', () => ({
  isGateOpen: () => gateOpen,
  enforcementMode: () => (gateOpen ? 'strict' : 'off'),
  ownershipStatus: () => undefined,
}))

import { PrivacyBadge } from '@/features/knowledge/components/PrivacyBadge'
import { ShareDialog } from '@/features/knowledge/components/ShareDialog'

beforeEach(() => {
  // ShareDialog fetches its grant list on mount; keep it offline + deterministic.
  vi.stubGlobal(
    'fetch',
    vi.fn(
      async () =>
        new Response(JSON.stringify({ grants: [] }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        }),
    ),
  )
})

afterEach(() => {
  cleanup()
  gateOpen = false
  vi.unstubAllGlobals()
})

describe('PR-6 honesty gate — privacy affordances render ONLY when enforcement is strict + identity live', () => {
  it('PrivacyBadge renders NOTHING when the gate is closed (enforcement != strict OR no identity)', () => {
    gateOpen = false
    render(() => <PrivacyBadge visibility="private" />)
    expect(screen.queryByText(/^Private$/)).toBeNull()
  })

  it('PrivacyBadge renders the badge when the gate is open', () => {
    gateOpen = true
    render(() => <PrivacyBadge visibility="private" />)
    expect(screen.getByText(/^Private$/)).toBeTruthy()
  })

  it('PrivacyBadge renders nothing when the gate is open but no visibility is set (honest empty)', () => {
    gateOpen = true
    render(() => <PrivacyBadge />)
    expect(screen.queryByText(/^(Private|Organization|Shared)$/)).toBeNull()
  })

  it('ShareDialog renders NO dialog when the gate is closed', () => {
    gateOpen = false
    const { container } = render(() => <ShareDialog docId="doc-1" onClose={() => undefined} />)
    expect(container.querySelector('[role="dialog"]')).toBeNull()
  })

  it('ShareDialog renders the dialog when the gate is open', () => {
    gateOpen = true
    const { container } = render(() => <ShareDialog docId="doc-1" onClose={() => undefined} />)
    expect(container.querySelector('[role="dialog"]')).not.toBeNull()
  })
})
