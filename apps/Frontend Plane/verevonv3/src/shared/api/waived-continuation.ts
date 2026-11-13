import { isConnectionZdrConnectionId } from '@/shared/actions/connection-zdr'
import { isZdrWaiverUiEnabled } from './connection-zdr-client'

// The connection a continuation names when its run was waived (ZDR Z-19e, D-ZDR-15). A continuation (an approval
// decision, a plan approval or rejection, a resume, a continue, a cancel) never inherits a waiver: the gateway asks
// Auth Core afresh, and it can only do that for the connection the browser names. This module builds the two wire
// fields for that, nothing else. It is a question put to the gateway, never an authority: the run's own receipt,
// checked upstream, decides whether the waiver can advance the run. Nothing here is stored in the browser.

/** The provider and the connection id of a waived run's subscription connection, as the thread's last run named them. */
export type WaivedRunConnection = Readonly<{
  /** The opaque Integration Core connection id of the subscription connection. */
  subscriptionConnectionId: string
  /** The connection's provider, such as the ChatGPT subscription provider. */
  provider: string
}>

/** The wire key for the connection id, in the body of a POST continuation and in the query of a GET one. */
export const WAIVED_CONNECTION_ID_KEY = 'subscription_connection_id'

/** The wire key for the connection's provider. */
export const WAIVED_PROVIDER_KEY = 'provider'

/**
 * The two fields a continuation sends to name the connection its run was waived for, or none. Empty with
 * VITE_ZDR_WAIVER_UI off, with no connection, or with a value the gateway would refuse (it takes 1 to 128 characters
 * with no whitespace), so a continuation without a waived run is byte-identical to what it was before.
 */
export function waivedContinuationFields(
  waived: WaivedRunConnection | undefined | null,
): { subscription_connection_id?: string; provider?: string } {
  if (!waived || !isZdrWaiverUiEnabled()) return {}
  const { subscriptionConnectionId, provider } = waived
  if (!isConnectionZdrConnectionId(subscriptionConnectionId) || !isConnectionZdrConnectionId(provider)) return {}
  return { subscription_connection_id: subscriptionConnectionId, provider }
}
