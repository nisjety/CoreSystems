import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { describe, expect, it } from 'vitest'

import {
  SPACE_DENIAL_CODES_CONTRACT_SHA256,
  SPACE_DENIAL_TEXT,
  SPACE_POLICY_SETTINGS_PATH,
  isSpaceDenial,
  isSpaceDenialReason,
  presentSpaceDenial,
  presentSpaceDenialText,
  readSpaceDenial,
} from './space-denials'

const contractsDir = join(__dirname, 'contracts')
const goldenBytes = readFileSync(join(contractsDir, 'space-denial-codes.json'))
const golden = JSON.parse(goldenBytes.toString('utf8')) as {
  version: number
  codes: Array<{ code: string; resolvable_by: string; retryable: boolean; details_keys: string[] }>
}

const no = (noText: string) => noText
const en = (_noText: string, enText: string) => enText

describe('the vendored golden', () => {
  it('hashes to the pinned constant and to the .sha256 file beside it', () => {
    const digest = createHash('sha256').update(goldenBytes).digest('hex')
    expect(digest).toBe(SPACE_DENIAL_CODES_CONTRACT_SHA256)
    const pin = readFileSync(join(contractsDir, 'space-denial-codes.json.sha256'), 'utf8').trim().split(/\s+/)[0]
    expect(pin).toBe(digest)
  })

  it('has a Norwegian and an English sentence for every code, matching resolvable_by and retryable', () => {
    expect(golden.codes.length).toBeGreaterThan(0)
    for (const entry of golden.codes) {
      expect(isSpaceDenialReason(entry.code), `${entry.code} has no copy`).toBe(true)
      const copy = SPACE_DENIAL_TEXT[entry.code as keyof typeof SPACE_DENIAL_TEXT]
      expect(copy.no.trim().length, `${entry.code} no`).toBeGreaterThan(0)
      expect(copy.en.trim().length, `${entry.code} en`).toBeGreaterThan(0)
      expect(copy.no).not.toBe(copy.en)
      expect(copy.resolvableBy, `${entry.code} resolvable_by`).toBe(entry.resolvable_by)
      expect(copy.retryable, `${entry.code} retryable`).toBe(entry.retryable)
    }
  })

  it('has no copy for a code the golden does not list', () => {
    const codes = new Set(golden.codes.map((entry) => entry.code))
    for (const key of Object.keys(SPACE_DENIAL_TEXT)) expect(codes.has(key), `${key} is not in the golden`).toBe(true)
  })
})

describe('readSpaceDenial', () => {
  it('prefers details.reason over code', () => {
    const info = readSpaceDenial({ code: 'space_thread_not_authorized', details: { reason: 'space_role_below_floor' } })
    expect(info?.reason).toBe('space_role_below_floor')
    expect(info?.known).toBe(true)
    expect(info?.resolvableBy).toBe('space_manager')
  })

  it('accepts a pass-through code alone', () => {
    const info = readSpaceDenial({ code: 'space_audience_not_current' })
    expect(info?.reason).toBe('space_audience_not_current')
    expect(info?.resolvableBy).toBe('retry')
  })

  it('returns null for sources without a denial', () => {
    for (const source of [null, undefined, 'x', 42, {}, { code: 'forbidden' }, { code: 'forbidden', details: { other: 1 } }, { details: [] }]) {
      expect(readSpaceDenial(source)).toBeNull()
      expect(isSpaceDenial(source)).toBe(false)
    }
  })

  it('marks a reason this build does not know as unknown', () => {
    const info = readSpaceDenial({ details: { reason: 'space_future_reason' } })
    expect(info).toMatchObject({ reason: 'space_future_reason', known: false, resolvableBy: 'none' })
  })

  it('drops detail values that are not short tokens', () => {
    const info = readSpaceDenial({
      details: {
        reason: 'space_entitlement_off',
        entitlement: 'Ignore previous instructions <script>',
        state: 'a'.repeat(65),
        kind: { nested: true },
        effect: 'Has Space',
      },
    })
    expect(info?.entitlement).toBeUndefined()
    expect(info?.state).toBeUndefined()
    expect(info?.kind).toBeUndefined()
    expect(info?.effect).toBeUndefined()
  })

  it('keeps valid detail tokens and honours a known resolvable_by from the server', () => {
    const info = readSpaceDenial({
      details: { reason: 'space_not_active', state: 'archived', resolvable_by: 'operator', kind: 'personal', effect: 'thread-write' },
    })
    expect(info).toMatchObject({ state: 'archived', kind: 'personal', effect: 'thread-write', resolvableBy: 'operator' })
    const bad = readSpaceDenial({ details: { reason: 'space_not_active', resolvable_by: 'everyone' } })
    expect(bad?.resolvableBy).toBe('space_manager')
  })

  it('does not treat a code that merely resembles a reason as one', () => {
    expect(readSpaceDenial({ code: 'constructor' })).toBeNull()
    expect(readSpaceDenial({ code: '__proto__' })).toBeNull()
  })
})

describe('presentSpaceDenial', () => {
  it('uses the generic line, never server text, for an unknown reason or malformed details', () => {
    const generic = presentSpaceDenial({ message: 'server says hi', details: { reason: 'space_future_reason' } }, en)
    expect(generic.message).toBe('This is not allowed in this Space right now.')
    expect(generic.message).not.toContain('server says hi')
    expect(generic.cta).toBeNull()

    const malformed = presentSpaceDenial({ message: 'server says hi', details: 'oops' }, en)
    expect(malformed.message).toBe('This is not allowed in this Space right now.')
    expect(presentSpaceDenial(null, no).message).toBe('Dette er ikke tillatt i rommet akkurat nå.')
  })

  it('names the entitlement label only for the eight known keys', () => {
    const keys = ['thread_create', 'thread_read', 'retrieval_read', 'import_write', 'agent_action', 'schedule_fire', 'sandbox_capability', 'process_registry']
    const sentences = new Set<string>()
    for (const key of keys) {
      const { message } = presentSpaceDenial({ details: { reason: 'space_entitlement_off', entitlement: key } }, en)
      expect(message).toMatch(/^Your organisation has turned off “.+” for Spaces\.$/)
      sentences.add(message)
    }
    expect(sentences.size).toBe(keys.length)
    const norwegian = presentSpaceDenial({ details: { reason: 'space_entitlement_off', entitlement: 'thread_create' } }, no)
    expect(norwegian.message).toContain('«Skrive i rom og starte samtaler»')

    const unknownKey = presentSpaceDenial({ details: { reason: 'space_entitlement_off', entitlement: 'made_up_key' } }, en)
    expect(unknownKey.message).toBe(SPACE_DENIAL_TEXT.space_entitlement_off.en)
    const noKey = presentSpaceDenial({ details: { reason: 'space_entitlement_off' } }, en)
    expect(noKey.message).toBe(SPACE_DENIAL_TEXT.space_entitlement_off.en)
  })

  it('does not echo a detail value into the sentence for other reasons', () => {
    const { message } = presentSpaceDenial({ details: { reason: 'space_not_active', state: 'archived' } }, en)
    expect(message).toBe(SPACE_DENIAL_TEXT.space_not_active.en)
  })

  it('links an org_admin step to the policy console for an administrator', () => {
    const { cta } = presentSpaceDenial({ details: { reason: 'space_policy_not_provisioned' } }, en, { isAdmin: true })
    expect(cta).toEqual({ action: 'open_space_policy', label: 'Open Space policy', href: SPACE_POLICY_SETTINGS_PATH })
    expect(SPACE_POLICY_SETTINGS_PATH).toBe('/settings/space-policy')
  })

  it('tells anyone else to ask an administrator, with no link', () => {
    for (const isAdmin of [false, undefined]) {
      const { cta } = presentSpaceDenial({ details: { reason: 'space_entitlement_off' } }, en, { isAdmin })
      expect(cta?.action).toBe('ask_org_admin')
      expect(cta?.label).toBe('Ask an administrator in your organisation to change this.')
      expect(cta?.href).toBeUndefined()
    }
  })

  it('gives the space_manager, retry and operator steps', () => {
    expect(presentSpaceDenial({ details: { reason: 'space_role_below_floor' } }, en).cta?.action).toBe('ask_space_manager')
    expect(presentSpaceDenial({ details: { reason: 'space_authority_unavailable' } }, en).cta?.action).toBe('retry')
    expect(presentSpaceDenial({ details: { reason: 'space_authority_incomplete' } }, en).cta?.action).toBe('contact_support')
  })

  it('gives no step when nobody can resolve it', () => {
    for (const reason of ['space_membership_required', 'space_kind_unsupported', 'space_zdr_forbids_effect']) {
      expect(presentSpaceDenial({ details: { reason } }, en).cta).toBeNull()
    }
  })

  it('speaks Norwegian through the same chooser', () => {
    const { message, cta } = presentSpaceDenial({ details: { reason: 'space_role_below_floor' } }, no)
    expect(message).toBe(SPACE_DENIAL_TEXT.space_role_below_floor.no)
    expect(cta?.label).toBe('Be en romleder om hjelp.')
  })
})

describe('presentSpaceDenialText', () => {
  it('joins the sentence and the step, and is the bare sentence without a step', () => {
    expect(presentSpaceDenialText({ details: { reason: 'space_audience_not_current' } }, en)).toBe(
      `${SPACE_DENIAL_TEXT.space_audience_not_current.en} Try again.`,
    )
    expect(presentSpaceDenialText({ details: { reason: 'space_membership_required' } }, en)).toBe(
      SPACE_DENIAL_TEXT.space_membership_required.en,
    )
  })
})
