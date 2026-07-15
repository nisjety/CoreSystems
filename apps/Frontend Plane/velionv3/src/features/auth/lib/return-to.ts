const RETURN_BASE = 'https://velion.invalid'

/** Accept only same-origin absolute paths for post-auth navigation. */
export function safeReturnTo(value: string | null | undefined): string | null {
  const candidate = value?.trim()
  if (!candidate || !candidate.startsWith('/') || candidate.startsWith('//') || candidate.includes('\\')) {
    return null
  }

  try {
    const parsed = new URL(candidate, RETURN_BASE)
    if (parsed.origin !== RETURN_BASE) return null
    return `${parsed.pathname}${parsed.search}${parsed.hash}`
  } catch {
    return null
  }
}
