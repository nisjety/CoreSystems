import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import { describe, expect, it } from 'vitest'
import { REGISTRY_GOLDEN_DIGEST, SPACE_SELF_API_ROWS, spaceSelfApiOfferedRows } from './agent-self-api'

// WS18-S6 and WS18-S7: the SPA's vendored copy of the Space agent self-API registry. The golden is
// apps/Model Plane/api/contracts/space-self-api-registry.json. This app keeps a byte-identical copy beside its
// sha256 pin, and the rows the toggle states must agree with that golden.
const vendoredPath = join(__dirname, '..', 'api', 'contracts', 'space-self-api-registry.json')
// apps/Frontend Plane/verevonv3/src/shared/actions -> apps
const ownerPath = join(
  __dirname,
  '..',
  '..',
  '..',
  '..',
  '..',
  'Model Plane',
  'api',
  'contracts',
  'space-self-api-registry.json',
)

const vendoredBytes = readFileSync(vendoredPath)
const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex')

type GoldenRow = {
  action_id: string
  risk: string
  default_level: string
  phase: number
  max_per_run: number
}
const golden = JSON.parse(vendoredBytes.toString('utf8')) as { rows: GoldenRow[] }

describe('space-self-api-registry.json (WS18-S6)', () => {
  it('hashes to the golden digest and to the .sha256 file beside it', () => {
    expect(sha256(vendoredBytes)).toBe(REGISTRY_GOLDEN_DIGEST)
    const pin = readFileSync(`${vendoredPath}.sha256`, 'utf8').trim().split(/\s+/)[0]
    expect(pin).toBe(REGISTRY_GOLDEN_DIGEST)
  })

  it('is byte-identical to the Model Plane owner copy', () => {
    // The sibling plane is part of the same monorepo; a checkout without it cannot judge the sync.
    if (!existsSync(ownerPath)) return
    expect(readFileSync(ownerPath).equals(vendoredBytes)).toBe(true)
  })

  it('states the rename, pin and post_card rows with the golden action id, risk, default level, phase and max per run', () => {
    expect(SPACE_SELF_API_ROWS.map((row) => row.actionId)).toEqual([
      'space.thread.rename',
      'space.thread.pin',
      'space.thread.post_card',
    ])
    for (const row of SPACE_SELF_API_ROWS) {
      const entry = golden.rows.find((candidate) => candidate.action_id === row.actionId)
      expect(entry, row.actionId).toBeDefined()
      expect(row.risk, row.actionId).toBe(entry?.risk)
      expect(row.defaultLevel, row.actionId).toBe(entry?.default_level)
      expect(row.phase, row.actionId).toBe(entry?.phase)
      expect(row.maxPerRun, row.actionId).toBe(entry?.max_per_run)
    }
  })

  it('offers a room agent rename, pin and post_card, in the golden order (WS18-S6, updated by WS18-S7)', () => {
    const offered: readonly string[] = spaceSelfApiOfferedRows().map((row) => row.actionId)
    expect(offered).toEqual(['space.thread.rename', 'space.thread.pin', 'space.thread.post_card'])
    expect(golden.rows.map((row) => row.action_id).filter((id) => offered.includes(id))).toEqual(offered)
  })
})
