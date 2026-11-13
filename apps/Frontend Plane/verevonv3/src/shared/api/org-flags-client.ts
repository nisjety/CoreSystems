import { createSignal } from 'solid-js'
import { ApiError } from './api-error'
import { requestJson } from './http'

/**
 * The organisation's own values for the presentation flags it may set (W5-079-c, F-t3-079). The gateway serves them
 * at `/api/v1/org-flags`, and this client reads them once per session. A value only narrows or enables a feature
 * inside the build switch (`VITE_*`), which stays the ceiling. No authorization decision reads them.
 */

/** One read: the values the organisation names, and whether the read failed. */
export type OrgFlagsSnapshot = {
  readonly flags: Readonly<Record<string, string>>
  readonly stale: boolean
}

/** The answer's body. `flags` is empty and `stale` is set when the gateway could not read the organisation. */
type OrgFlagsBody = {
  flags?: unknown
  revision?: unknown
  stale?: unknown
}

/**
 * No organisation layer: the switch upstream is off, or the viewer has no organisation. Every flag keeps the default
 * the call site passes.
 */
const NO_LAYER: OrgFlagsSnapshot = { flags: {}, stale: false }

/**
 * The read failed. Every presentation flag reads as off, so an outage hides a feature rather than showing it
 * unapproved.
 */
const FAILED: OrgFlagsSnapshot = { flags: {}, stale: true }

let session: Promise<OrgFlagsSnapshot> | undefined
const [snapshot, setSnapshot] = createSignal<OrgFlagsSnapshot | undefined>(undefined)

/**
 * Starts the session's one read and resolves with it. Later calls return the same promise, and the read never
 * rejects: a failed read resolves as failed, so no caller has to catch it.
 */
export function loadOrgFlags(): Promise<OrgFlagsSnapshot> {
  session ??= readOrgFlags().then((next) => {
    setSnapshot(next)
    return next
  })
  return session
}

/**
 * The organisation's value for one presentation flag, as a boolean. Until the read lands, and when the answer names no
 * value for the flag, it is `fallback`. A failed read is off. The first call starts the session's read. A read inside a
 * tracked scope updates once the answer lands.
 */
export function orgFlag(name: string, fallback: boolean): boolean {
  void loadOrgFlags()
  const current = snapshot()
  if (!current) return fallback
  if (current.stale) return false
  const value = current.flags[name]
  if (value === 'true') return true
  if (value === 'false') return false
  return fallback
}

async function readOrgFlags(): Promise<OrgFlagsSnapshot> {
  try {
    return parseOrgFlags(await requestJson<OrgFlagsBody>('/api/v1/org-flags'))
  } catch (error) {
    return isNoLayer(error) ? NO_LAYER : FAILED
  }
}

/**
 * A 404 (the upstream switch is off) and a refused membership (the viewer has no organisation) carry no layer. Any
 * other failure is a failed read.
 */
function isNoLayer(error: unknown): boolean {
  return error instanceof ApiError && (error.status === 404 || error.code === 'organization_membership_required')
}

function parseOrgFlags(body: unknown): OrgFlagsSnapshot {
  if (!body || typeof body !== 'object') return FAILED
  const { flags, stale } = body as OrgFlagsBody
  if (stale === true) return FAILED
  if (!flags || typeof flags !== 'object' || Array.isArray(flags)) return FAILED
  const values: Record<string, string> = {}
  for (const [name, value] of Object.entries(flags)) {
    if (typeof value === 'string') values[name] = value
  }
  return { flags: values, stale: false }
}
