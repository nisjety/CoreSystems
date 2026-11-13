import { describe, expect, it, vi } from 'vitest'

// W3-EP-4-S11-href: a search result opens or links only by an absolute http(s) address. A javascript:,
// data:, relative or malformed URL must never reach window.open or an anchor href, where it would run
// in the app origin. The panel and the result card both go through safeExternalHref.

vi.mock('@solidjs/router', () => ({ useNavigate: () => () => undefined }))

import { safeExternalHref } from './DashboardSearchPanel'

describe('safeExternalHref', () => {
  it.each([
    ['http', 'http://example.com/page?q=1', 'http://example.com/page?q=1'],
    ['https', 'https://example.com/a b', 'https://example.com/a%20b'],
    ['https with upper-case scheme', 'HTTPS://Example.com/', 'https://example.com/'],
  ])('keeps an absolute %s address', (_label, url, expected) => {
    expect(safeExternalHref(url)).toBe(expected)
  })

  it.each([
    ['javascript:', 'javascript:alert(document.cookie)'],
    ['javascript: with mixed case and whitespace', '  JavaScript:alert(1)'],
    ['javascript: hidden behind a tab', 'java\tscript:alert(1)'],
    ['data:', 'data:text/html,<script>alert(1)</script>'],
    ['vbscript:', 'vbscript:msgbox(1)'],
    ['file:', 'file:///etc/passwd'],
    ['blob:', 'blob:https://example.com/0f6c7f1d'],
    ['relative path', '/settings/account'],
    ['protocol-relative', '//evil.example.com/x'],
    ['bare host', 'example.com'],
    ['malformed', 'http://'],
    ['empty', ''],
  ])('refuses a %s URL', (_label, url) => {
    expect(safeExternalHref(url)).toBeNull()
  })
})
