import { describe, expect, it } from 'vitest'
import { normalizePulseSite } from './pulse-site-stack.ts'

describe('Visibility website identity', () => {
  it('canonicalizes a public HTTPS site origin without retaining a path', () => {
    expect(normalizePulseSite('aquatiq.com')).toBe('https://aquatiq.com/')
    expect(normalizePulseSite(' HTTPS://WWW.AQUATIQ.COM/ ')).toBe('https://www.aquatiq.com/')
  })

  it.each(['http://aquatiq.com/', 'https://aquatiq.com/page', 'https://aquatiq.com/?q=1', 'https://user@aquatiq.com/', 'https://localhost/', 'https://127.0.0.1/', 'https://10.0.0.1/', 'https://aquatiq.com:8443/', 'javascript:alert(1)'])('rejects unsupported or unsafe site %s', (site) => {
    expect(() => normalizePulseSite(site)).toThrow()
  })

})
