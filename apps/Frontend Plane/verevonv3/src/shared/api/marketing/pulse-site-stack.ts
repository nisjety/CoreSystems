/** Website identity validation only. The website registry belongs to Marketing Core. */
export function normalizePulseSite(input: string): string {
  const value = input.trim()
  if (!value || value.length > 253 || /\s/.test(value)) throw new Error('Enter a valid public website URL.')
  const candidate = /^[a-z][a-z\d+.-]*:/i.test(value) ? value : `https://${value}`
  let url: URL
  try { url = new URL(candidate) } catch { throw new Error('Enter a valid public website URL.') }
  const host = url.hostname.toLowerCase()
  const labels = host.split('.')
  const validHost = labels.length >= 2 && labels.every((label) => label.length <= 63 && /^[a-z\d](?:[a-z\d-]*[a-z\d])?$/.test(label)) && /[a-z]/.test(labels.at(-1) ?? '')
  if (url.protocol !== 'https:' || url.username || url.password || url.port || url.pathname !== '/' || url.search || url.hash || !validHost || /^(?:\d+\.){3}\d+$/.test(host) || ['localhost', 'local', 'internal', 'test', 'invalid'].includes(labels.at(-1) ?? '')) {
    throw new Error('Use a public HTTPS site address without a path, port, or query.')
  }
  return `https://${host}/`
}
