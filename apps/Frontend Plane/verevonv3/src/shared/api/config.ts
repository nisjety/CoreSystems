export function gatewayBaseUrl(): string {
  // Default to same-origin ('') so the browser only ever talks to its own
  // origin, and the dev server (Vite proxy) or nginx (Docker) reverse-proxies
  // /api → the gateway. This keeps Better Auth's session cookie first-party
  // (SameSite=Lax works); a cross-origin base URL would drop the cookie on XHR.
  // An absolute VITE_VEREVON_GATEWAY_URL can still force a direct base if set.
  const configured = import.meta.env.VITE_VEREVON_GATEWAY_URL
  if (!configured) return ''
  return configured.replace(/\/+$/, '')
}
