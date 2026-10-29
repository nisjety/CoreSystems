/** Returns a URL safe for an external browser navigation, or null when the
 * stored ticket metadata is not an HTTP(S) destination. */
export function safeTicketResourceUrl(value: string | undefined): string | null {
  if (!value) return null
  try {
    const url = new URL(value)
    return url.protocol === 'https:' || url.protocol === 'http:' ? value : null
  } catch {
    return null
  }
}
