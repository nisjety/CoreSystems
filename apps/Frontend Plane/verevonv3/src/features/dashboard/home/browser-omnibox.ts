const HTTP_URL_RE = /^https?:\/\//i
const HOST_RE = /^(?:localhost|(?:[a-z0-9-]+\.)+[a-z]{2,}|\d{1,3}(?:\.\d{1,3}){3})(?::\d+)?(?:[/?#].*)?$/i

/**
 * What the typed text is: an address to open, or words to search for. Only the
 * shape of the text is decided here; nothing is fetched or searched. A non-web
 * scheme such as `about:blank` or `javascript:` is a query, never an address.
 */
export type OmniboxInput =
  | { kind: 'empty' }
  | { kind: 'url'; url: string }
  | { kind: 'query'; query: string }

export function classifyOmniboxInput(input: string): OmniboxInput {
  const value = input.trim()
  if (!value) return { kind: 'empty' }
  if (HTTP_URL_RE.test(value)) return { kind: 'url', url: value }
  if (!/\s/.test(value) && HOST_RE.test(value)) {
    const scheme = value.startsWith('localhost') || /^\d{1,3}(?:\.\d{1,3}){3}/.test(value) ? 'http' : 'https'
    return { kind: 'url', url: `${scheme}://${value}` }
  }
  return { kind: 'query', query: value }
}

/**
 * The address a submitted omnibox opens, or null. An empty field opens the
 * current page address when there is one. Free text is never turned into a
 * third-party search URL: it is a query, and the omnibox resolver decides.
 */
export function browserOmniboxTarget(input: string, fallbackUrl?: string | null): string | null {
  const classified = classifyOmniboxInput(input)
  if (classified.kind === 'url') return classified.url
  if (classified.kind === 'empty') return fallbackUrl?.trim() || null
  return null
}
