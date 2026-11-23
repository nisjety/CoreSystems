// WS01-S8 (optional probe): the gateway's unauthenticated `GET /health` answers whether the service is
// reachable at all, so a room can say so before a send. Same origin as the rest of the client
// (`gatewayBaseUrl()`, proxied by nginx and the dev server), no credentials, no content. The answer is a
// status code, never a claim about any room, and it is never an authorization input.
import { gatewayBaseUrl } from '../config'

/** How long the probe waits for an answer before it counts the service as unreachable. */
export const SPACE_SERVICE_PROBE_TIMEOUT_MS = 5_000

/**
 * True when the gateway answered below 500. A failed request (no connection, a timeout) and a server error are both
 * `false`: from this browser the service cannot be reached right now. Never throws.
 */
export async function probeSpaceService(timeoutMs: number = SPACE_SERVICE_PROBE_TIMEOUT_MS): Promise<boolean> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    const response = await fetch(`${gatewayBaseUrl()}/health`, {
      method: 'GET',
      credentials: 'omit',
      cache: 'no-store',
      signal: controller.signal,
    })
    return response.status < 500
  } catch {
    return false
  } finally {
    clearTimeout(timer)
  }
}
