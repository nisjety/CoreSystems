import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { CONTROL_PLANE_DSAR_DISCLOSURE } from '@/shared/api/privacy-client'

const GO_SOURCE = ['apps', 'Control Plane', 'user-core', 'internal', 'users', 'gdpr.go']

/** The strings of `DSARControlPlaneDisclosure` as written in user-core's gdpr.go. */
function backendDisclosure(): string[] {
  let dir = dirname(fileURLToPath(import.meta.url))
  for (;;) {
    const candidate = join(dir, ...GO_SOURCE)
    if (existsSync(candidate)) {
      const text = readFileSync(candidate, 'utf8')
      const block = /var DSARControlPlaneDisclosure = \[\]string\{([\s\S]*?)\n\}/.exec(text)
      if (!block) throw new Error('DSARControlPlaneDisclosure not found in gdpr.go')
      return [...(block[1] ?? '').matchAll(/^\s*("(?:[^"\\]|\\.)*"),\s*$/gm)].map((m) => JSON.parse(m[1] ?? '""') as string)
    }
    const parent = dirname(dir)
    if (parent === dir) throw new Error('repository root with gdpr.go not found')
    dir = parent
  }
}

// The Privacy & data UI renders this disclosure as the authoritative scope
// statement. It must match user-core's gdpr.go BuildDSARExport notes VERBATIM so
// the UI never over- or under-states what the export/erase covers. If gdpr.go
// changes, this test fails until both are reconciled.
describe('CONTROL_PLANE_DSAR_DISCLOSURE', () => {
  it('matches user-core gdpr.go DSAR notes verbatim', () => {
    expect([...CONTROL_PLANE_DSAR_DISCLOSURE]).toEqual(backendDisclosure())
  })

  it('says a per-user erasure does not delete Model Plane or Data Plane data', () => {
    // docs/compliance/evidence/retention-and-erasure-evidence.md section 2: session-core
    // acknowledges a user erasure event and deletes nothing.
    const scope = CONTROL_PLANE_DSAR_DISCLOSURE.join(' ')
    expect(scope).toContain('a per-user erasure does not delete them')
    expect(scope).not.toMatch(/purged\/exported via/)
    expect(scope).not.toMatch(/follow-up subscribers/)
  })
})
