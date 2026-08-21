// THE honesty gate (PR-6). Every per-user privacy affordance in the SPA —
// Private/Shared badge, "Private" visibility option, the Share button, the
// ShareDialog, the "shared with me" view — MUST render only when this gate is
// open. Shipping such an affordance while the backend isn't enforcing per-user
// privacy (or identity isn't live) would be a FALSE-PRIVACY guarantee, which the
// ownership plan's ABSOLUTE RULE forbids.
//
// The gate is decided SERVER-SIDE: GET /api/v1/ownership/status returns
// `gate_open = enforcement === 'strict' && identity_live`. The client only
// reads it. FAIL CLOSED: while loading, on error, or anything other than an
// explicit server `gate_open === true`, the gate is CLOSED.

import { createResource } from '@/shared/lib/create-resource-compat'

import { getOwnershipStatus, type OwnershipStatus } from '@/shared/api/ownership-client'

// Module-singleton: fetched once, shared by every affordance. createResource
// gives loading/error states; both resolve to a CLOSED gate below.
const [status] = createResource<OwnershipStatus>(getOwnershipStatus)

/** The raw status accessor (undefined while loading / on error). */
export function ownershipStatus(): OwnershipStatus | undefined {
  return status()
}

/**
 * The ONLY predicate affordances should branch on. True only when the server
 * says gate_open === true; loading/error/false → CLOSED.
 */
export function isGateOpen(): boolean {
  return status()?.gate_open === true
}

/** Enforcement mode for diagnostics/copy; defaults to the safe 'off'. */
export function enforcementMode(): OwnershipStatus['enforcement'] {
  return status()?.enforcement ?? 'off'
}
