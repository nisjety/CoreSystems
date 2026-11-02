import { isSafeLinkHref } from '@/features/chat/lib/safe-href'

/**
 * Link safety for model- and member-authored link targets (WS07-S5).
 *
 * The scheme allowlist (https, http, mailto) is the one in
 * `features/chat/lib/safe-href.ts`, which the document export also uses, so
 * every place a written target can become clickable shares one rule.
 */

/** The trimmed URL when its scheme is allowed, otherwise `null`. Fails closed. */
export function safeHref(raw: string): string | null {
  if (typeof raw !== 'string') return null
  return isSafeLinkHref(raw) ? raw.trim() : null
}

/** The host a safe link points at (the domain of a mailto address), or `null`. */
export function linkHost(href: string): string | null {
  const safe = safeHref(href)
  if (!safe) return null
  try {
    if (/^mailto:/i.test(safe)) {
      const address = decodeURIComponent(safe.slice('mailto:'.length).split(/[?,]/)[0] ?? '')
      const at = address.lastIndexOf('@')
      return at >= 0 && at < address.length - 1 ? address.slice(at + 1).toLowerCase() : null
    }
    return new URL(safe).hostname.toLowerCase() || null
  } catch {
    return null
  }
}

const DOMAIN_LIKE_PATTERN = /^(?:https?:\/\/)?(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,}(?::\d+)?(?:[/?#]\S*)?$/i

function bareHost(host: string): string {
  return host.replace(/^www\./, '')
}

/**
 * True when the visible text reads as a URL or domain whose host differs from
 * the host the link really opens ("[example.com](https://evil.test)").
 */
export function looksSpoofed(text: string, href: string): boolean {
  const target = linkHost(href)
  if (!target) return false
  const visible = text.trim()
  if (!DOMAIN_LIKE_PATTERN.test(visible)) return false
  try {
    const shown = new URL(/^https?:\/\//i.test(visible) ? visible : `https://${visible}`).hostname.toLowerCase()
    return bareHost(shown) !== bareHost(target)
  } catch {
    return false
  }
}
