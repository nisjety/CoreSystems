/** A typed address must be unambiguous before it can start a browser session. */
export function webAddress(input: string): string | null {
  const value = input.trim()
  if (!value || /\s/.test(value)) return null
  // Do not turn an explicit unsupported scheme into a different URL.
  if (/^[a-z][a-z\d+.-]*:/i.test(value) && !/^https?:\/\//i.test(value)) return null
  try {
    const url = new URL(/^https?:\/\//i.test(value) ? value : `https://${value}`)
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) return null
    if (!url.hostname.includes('.') && url.hostname !== 'localhost') return null
    return url.href
  } catch {
    return null
  }
}
