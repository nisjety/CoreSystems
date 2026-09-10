import { requestJson } from './http'

/**
 * Connection details for the RustDesk-compatible Support Plane, as served by
 * the gateway's `remote_support` domain. `configured` is strict: it needs both a
 * rendezvous URL and the server public key, because without the key the
 * browser cannot authenticate the remote peer — and the product UI never falls
 * back to an unverified handshake.
 */
export interface RemoteSupportConfig {
  configured: boolean
  rendezvousUrl: string | null
  relayUrl: string | null
  serverPublicKey: string | null
  /** Environment variable names the gateway is still missing; empty when configured. */
  missing: string[]
}

export function getRemoteSupportConfig(): Promise<RemoteSupportConfig> {
  return requestJson<RemoteSupportConfig>('/api/v1/remote-support/config')
}
