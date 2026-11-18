import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import { describe, expect, it } from 'vitest'
import { SIDEBAR_LIMITS, emptySidebarDoc, sidebarDocSchema } from './space-sidebar'

// WS16-S9: the sidebar caps are pinned by space-sidebar-limits.json (+ .sha256). The file is byte-identical to the
// Application Plane fixture that convex-core's sidebar rules read, so the gateway, the SPA and Convex share one set of numbers.
const limitsPath = join(__dirname, 'space-sidebar-limits.json')
// apps/Frontend Plane/verevonv3/src/shared/actions -> apps
const ownerPath = join(
  __dirname,
  '..',
  '..',
  '..',
  '..',
  '..',
  'Application Plane',
  'convex-core',
  'test',
  'fixtures',
  'space-sidebar-limits.json',
)

const limitsBytes = readFileSync(limitsPath)
const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex')
const refs = (count: number) => Array.from({ length: count }, (_, index) => `room-${index}`)
const section = (index: number, refCount = 0) => ({
  id: `s${index}`,
  name: `Section ${index}`,
  collapsed: false,
  sort: 'manual',
  refs: refs(refCount),
})

describe('space-sidebar-limits.json (WS16-S9)', () => {
  it('hashes to the .sha256 pin beside it', () => {
    const pin = readFileSync(`${limitsPath}.sha256`, 'utf8').trim().split(/\s+/)[0]
    expect(sha256(limitsBytes)).toBe(pin)
  })

  it('is byte-identical to the Application Plane fixture, and that fixture pin agrees', () => {
    // The sibling plane is part of the same monorepo; a checkout without it cannot judge the sync.
    if (!existsSync(ownerPath)) return
    const owner = readFileSync(ownerPath)
    expect(owner.equals(limitsBytes)).toBe(true)
    const ownerPin = readFileSync(`${ownerPath}.sha256`, 'utf8').trim().split(/\s+/)[0]
    expect(ownerPin).toBe(sha256(limitsBytes))
  })
})

describe('sidebarDocSchema (WS16-S9)', () => {
  it('accepts the empty document and refuses a key the schema does not name', () => {
    const doc = emptySidebarDoc()
    expect(sidebarDocSchema.safeParse(doc).success).toBe(true)
    expect(sidebarDocSchema.safeParse({ ...doc, extra: true }).success).toBe(false)
    expect(sidebarDocSchema.safeParse({ ...doc, computed: { ...doc.computed, extra: true } }).success).toBe(false)
    expect(sidebarDocSchema.safeParse({ ...doc, groups: [{ id: 'channels', extra: true }] }).success).toBe(false)
    expect(sidebarDocSchema.safeParse({ ...doc, sections: [{ ...section(0), extra: true }] }).success).toBe(false)
  })

  it('refuses a list over its cap and accepts a list at the cap', () => {
    const doc = emptySidebarDoc()
    expect(sidebarDocSchema.safeParse({ ...doc, starred: refs(SIDEBAR_LIMITS.starred) }).success).toBe(true)
    expect(sidebarDocSchema.safeParse({ ...doc, starred: refs(SIDEBAR_LIMITS.starred + 1) }).success).toBe(false)
    expect(sidebarDocSchema.safeParse({ ...doc, hidden: refs(SIDEBAR_LIMITS.hidden + 1) }).success).toBe(false)
    const sections = (count: number) => Array.from({ length: count }, (_, index) => section(index))
    expect(sidebarDocSchema.safeParse({ ...doc, sections: sections(SIDEBAR_LIMITS.sections) }).success).toBe(true)
    expect(sidebarDocSchema.safeParse({ ...doc, sections: sections(SIDEBAR_LIMITS.sections + 1) }).success).toBe(false)
    expect(
      sidebarDocSchema.safeParse({ ...doc, sections: [section(0, SIDEBAR_LIMITS.refsPerSection + 1)] }).success,
    ).toBe(false)
  })
})
