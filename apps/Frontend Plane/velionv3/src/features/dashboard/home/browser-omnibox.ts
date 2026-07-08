const HTTP_URL_RE = /^https?:\/\//i
const HOST_RE = /^(?:localhost|(?:[a-z0-9-]+\.)+[a-z]{2,}|\d{1,3}(?:\.\d{1,3}){3})(?::\d+)?(?:[/?#].*)?$/i

export function browserOmniboxTarget(input: string, fallbackUrl?: string | null): string | null {
  const value = input.trim()
  if (!value) return fallbackUrl?.trim() || null
  if (HTTP_URL_RE.test(value)) return value
  if (!/\s/.test(value) && HOST_RE.test(value)) {
    const scheme = value.startsWith('localhost') || /^\d{1,3}(?:\.\d{1,3}){3}/.test(value) ? 'http' : 'https'
    return `${scheme}://${value}`
  }
  return `https://www.google.com/search?q=${encodeURIComponent(value)}`
}
