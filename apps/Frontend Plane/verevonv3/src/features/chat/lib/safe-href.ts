/**
 * Schemes a model-authored link target may open. Everything else — `javascript:`,
 * `data:`, `file:`, `blob:`, `vbscript:`, protocol-relative and relative targets —
 * is refused, so a link the model wrote can only ever reach the web or a mail client.
 */
const SAFE_LINK_SCHEME_PATTERN = /^(?:https?:|mailto:)/i

/**
 * C0 controls and DEL. Browsers strip tabs and newlines out of a URL before
 * they parse the scheme, so a target that smuggles them in is refused outright
 * rather than being judged on the text that happens to be left.
 */
const CONTROL_CHARACTER_PATTERN = /[\u0000-\u001f\u007f]/

/** Whether `href` may become a real, clickable link in the chat. Fails closed. */
export function isSafeLinkHref(href: string): boolean {
  const trimmed = href.trim()
  return SAFE_LINK_SCHEME_PATTERN.test(trimmed) && !CONTROL_CHARACTER_PATTERN.test(trimmed)
}
