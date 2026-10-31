import {
  getBrowserSession,
  setBrowserControlMode,
  type BrowserControlMode,
  type BrowserSessionResponse,
} from '@/shared/api/browser-client'
import { ApiError } from '@/shared/api/http'

/** The typed code the gateway answers when a control change carries an epoch that has moved on (WS9.I). */
export const STALE_CONTROL_EPOCH_CODE = 'browser_control_epoch_stale'

/** Copy for the stale-epoch notice. The same sentence is the `browser_control_epoch_stale` row in errors.ts. */
export const STALE_CONTROL_COPY = {
  no: 'Styringen av nettleseren ble endret et annet sted. Oppdatert; prøv igjen.',
  en: 'Browser control changed elsewhere. It has been refreshed; try again.',
} as const

export function isStaleControlEpoch(reason: unknown): boolean {
  return reason instanceof ApiError && reason.code === STALE_CONTROL_EPOCH_CODE
}

export type ControlChangeOutcome = {
  /** The projection to attach: the new state after a change, the refetched state after a stale epoch. */
  response: BrowserSessionResponse
  /** True when the epoch had moved on: the change did not happen and the projection was refetched once. */
  stale: boolean
}

/**
 * Takes over or hands back browser control, compare-and-set on the epoch the person saw (WS9.I).
 * A stale epoch is never retried: the projection is refetched exactly once so the caller renders
 * what Quarry now says, and the person decides again. Every other failure is rethrown unchanged.
 */
export async function changeBrowserControl(
  orgId: string,
  sessionId: string,
  mode: BrowserControlMode,
  expectedEpoch: number | null | undefined,
): Promise<ControlChangeOutcome> {
  try {
    const response = await setBrowserControlMode(orgId, sessionId, mode, { expectedEpoch })
    return { response, stale: false }
  } catch (reason) {
    if (!isStaleControlEpoch(reason)) throw reason
    const response = await getBrowserSession(orgId, sessionId)
    return { response, stale: true }
  }
}
