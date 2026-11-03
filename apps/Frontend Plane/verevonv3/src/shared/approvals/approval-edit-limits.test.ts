/**
 * The approval edit limits (EP-10-S3) against the Model Plane golden. This SPA
 * holds a byte-identical copy of it inside its own tree (`./contracts`, pinned
 * by sha256 beside it), so the checks below read nothing outside this tree: a
 * golden that is missing fails here instead of passing by not running.
 */

import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import {
  APPROVAL_EDIT_CODES,
  APPROVAL_EDIT_CONTRACT_SHA256,
  APPROVAL_EDITED_FIELDS,
  MAX_BODY_TEXT_BYTES,
  MAX_RECIPIENT_BYTES,
  MAX_RECIPIENTS,
  MAX_SUBJECT_BYTES,
  MIN_RECIPIENTS,
  checkMailEditLimits,
  type MailSendPatch,
} from './approval-edit-limits'

type GoldenCase = {
  name: string
  input: unknown
  patch: Record<string, unknown>
  error?: string
  client: string | null
}

type Contract = {
  contract: string
  version: number
  limits: Record<string, number>
  codes: string[]
  edited_fields: string[]
  cases: GoldenCase[]
}

const here = dirname(fileURLToPath(import.meta.url))
const contractPath = join(here, 'contracts', 'approval-edit-cases.json')
const pinPath = join(here, 'contracts', 'approval-edit-cases.json.sha256')
const contractBytes = readFileSync(contractPath)
const contract = JSON.parse(contractBytes.toString('utf8')) as Contract

/** `{"$repeat": s, "count": n}` stands for `s` repeated `n` times, as in the Rust harness. */
function expand(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(expand)
  if (value !== null && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
    const record = value as Record<string, unknown>
    if (entries.length === 2 && typeof record.$repeat === 'string' && typeof record.count === 'number') {
      return record.$repeat.repeat(record.count)
    }
    return Object.fromEntries(entries.map(([key, item]) => [key, expand(item)]))
  }
  return value
}

describe('the vendored golden', () => {
  it('is the pinned copy, byte for byte, with its pin beside it', () => {
    expect(existsSync(pinPath)).toBe(true)
    const digest = createHash('sha256').update(contractBytes).digest('hex')
    expect(digest).toBe(APPROVAL_EDIT_CONTRACT_SHA256)
    expect(readFileSync(pinPath, 'utf8').split(/\s+/)[0]).toBe(APPROVAL_EDIT_CONTRACT_SHA256)
  })

  it('is the approval edit contract of the Model Plane', () => {
    expect(contract.contract).toBe('model-plane.approval-edit')
    expect(contract.version).toBe(1)
  })
})

describe('the limits mirror the server', () => {
  it('has the golden limits', () => {
    expect(contract.limits).toEqual({
      min_recipients: MIN_RECIPIENTS,
      max_recipients: MAX_RECIPIENTS,
      max_subject_bytes: MAX_SUBJECT_BYTES,
      max_body_text_bytes: MAX_BODY_TEXT_BYTES,
      max_recipient_bytes: MAX_RECIPIENT_BYTES,
    })
  })

  it('has the golden codes and edited field names, in order', () => {
    expect([...APPROVAL_EDIT_CODES]).toEqual(contract.codes)
    expect([...APPROVAL_EDITED_FIELDS]).toEqual(contract.edited_fields)
  })
})

describe('checkMailEditLimits', () => {
  it.each(contract.cases.map((row) => [row.name, row] as const))('%s', (_name, row) => {
    const patch = expand(row.patch) as MailSendPatch
    const result = checkMailEditLimits(patch)
    expect(result).toBe(row.client)
    // A limit the form refuses is one the server refuses too, unless the call was not editable.
    if (result !== null && row.error !== 'not_editable') {
      expect(row.error).toBe(result)
    }
  })

  it('accepts a patch at every limit', () => {
    expect(
      checkMailEditLimits({
        to: Array.from({ length: MAX_RECIPIENTS }, (_, n) => `p${n}@x.example`),
        subject: '€'.repeat(332) + 'xx',
        bodyText: 'x'.repeat(MAX_BODY_TEXT_BYTES),
      }),
    ).toBeNull()
  })
})
