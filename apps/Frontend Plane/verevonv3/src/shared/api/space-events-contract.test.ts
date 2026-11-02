import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import { describe, expect, it } from 'vitest'

// WS02-S5 contract sync. `contracts/space-events.json` is the SPA's vendored copy of the Space event
// stream vocabulary that Model Plane owns (apps/Model Plane/api/contracts/space-events.json). The
// golden hash below pins the bytes; the sync test keeps the two copies identical. A change to the
// vocabulary is made at the owner, then vendored here with the hash bumped in the same commit.
const GOLDEN_SHA256 = '452cc619dd5c4f07e3fb67a2c0776b7a7b3a478cb9a5b3a09e686f809603535f'

const contractsDir = join(__dirname, 'contracts')
const vendoredPath = join(contractsDir, 'space-events.json')
// apps/Frontend Plane/verevonv3/src/shared/api -> apps
const ownerPath = join(__dirname, '..', '..', '..', '..', '..', 'Model Plane', 'api', 'contracts', 'space-events.json')

const vendoredBytes = readFileSync(vendoredPath)
const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex')

type Contract = {
  schema: string
  version: number
  cursor: { grammar: string }
  events: {
    connected: { data: string[]; mode: string[] }
    closing: { data: string[]; codes: Record<string, { reconnect: boolean }> }
    error: { codes: string[] }
    [name: string]: unknown
  }
  kinds: Record<string, { refs: string[] }>
}
const contract = JSON.parse(vendoredBytes.toString('utf8')) as Contract

describe('space-events.json (WS02-S5)', () => {
  it('hashes to the golden constant and to the .sha256 file beside it', () => {
    expect(sha256(vendoredBytes)).toBe(GOLDEN_SHA256)
    const pin = readFileSync(join(contractsDir, 'space-events.json.sha256'), 'utf8').trim().split(/\s+/)[0]
    expect(pin).toBe(GOLDEN_SHA256)
  })

  it('is byte-identical to the Model Plane owner copy', () => {
    // The sibling plane is part of the same monorepo; a checkout without it cannot judge the sync.
    if (!existsSync(ownerPath)) return
    const owner = readFileSync(ownerPath)
    expect(sha256(owner)).toBe(sha256(vendoredBytes))
    expect(owner.equals(vendoredBytes)).toBe(true)
    const ownerPin = readFileSync(`${ownerPath}.sha256`, 'utf8').trim().split(/\s+/)[0]
    expect(ownerPin).toBe(GOLDEN_SHA256)
  })

  it('is the v1 stream vocabulary with the cursor grammar the controller parses', () => {
    expect(contract.schema).toBe('space-events.v1')
    expect(contract.version).toBe(1)
    expect(contract.cursor.grammar).toBe('<generation>.<seq>')
  })

  it('carries ids and closed words only: no kind lists a text-bearing ref', () => {
    for (const [kind, spec] of Object.entries(contract.kinds)) {
      for (const ref of spec.refs) {
        expect(ref, `${kind}.${ref}`).not.toMatch(/^(text|content|title|preview|body|output|message)$/)
      }
    }
  })

  // The controller branches on these words; a vocabulary change that renames one must fail here
  // rather than silently turn a permanent stop into a retry.
  it('names the closing codes the controller treats specially, with the reconnect flag it assumes', () => {
    const codes = contract.events.closing.codes
    expect(codes.zdr_poll_only?.reconnect).toBe(false)
    expect(codes.membership_revoked?.reconnect).toBe(false)
    for (const failing of ['upstream_unavailable', 'invalid_frame', 'slow_consumer']) {
      expect(codes[failing]?.reconnect, failing).toBe(true)
    }
    // Routine closes reconnect without leaving live.
    for (const routine of ['lease_expired', 'reauthorize', 'server_draining']) {
      expect(codes[routine]?.reconnect, routine).toBe(true)
    }
    // Nothing else may stop the stream for good unnoticed.
    const noReconnect = Object.entries(codes).filter(([, spec]) => !spec.reconnect).map(([code]) => code).sort()
    expect(noReconnect).toEqual(['membership_revoked', 'zdr_poll_only'])
  })

  it('names the connected modes and the error codes the controller reacts to', () => {
    expect(contract.events.connected.mode).toEqual(['live', 'poll_only'])
    expect(contract.events.error.codes).toEqual(expect.arrayContaining(['unauthorized', 'space_stream_forbidden']))
    expect(contract.events.connected.data).toEqual(expect.arrayContaining(['generation', 'head', 'resumed', 'mode']))
  })

  it('has a hidden and a redacted stub so the sequence stays contiguous', () => {
    expect(Object.keys(contract.kinds)).toEqual(expect.arrayContaining(['hidden', 'redacted']))
    expect(contract.kinds.hidden?.refs).toEqual([])
  })
})
