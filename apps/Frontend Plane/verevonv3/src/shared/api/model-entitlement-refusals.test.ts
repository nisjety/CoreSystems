// @vitest-environment jsdom

/**
 * The Norwegian and English rendering of every typed model-entitlement refusal
 * (WS10-S1). The codes, the English fallback of each, the action and the reasons
 * are the Model Plane golden. This SPA holds a byte-identical copy of it inside its
 * own tree (`./contracts`, pinned by sha256 beside it), so the checks below read
 * nothing outside this tree: a golden that is missing fails here instead of passing
 * by not running. The repo-level drift check
 * (scripts/tests/model-entitlement-contract-sync.test.sh) compares the copy with the golden.
 */

import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import {
  MODEL_ENTITLEMENT_REFUSALS_CONTRACT_SHA256,
  REASON_TEXT,
  REFUSAL_TEXT,
  isModelEntitlementCode,
  modelEntitlementPresentation,
} from './model-entitlement-refusals'

type Contract = {
  codes: Array<{ action: string; code: string; message_en: string; retryable: boolean; takes_reason: boolean }>
  reasons: Array<{ reason: string; message_en: string; names_model: boolean }>
  contract: string
  version: number
}

const here = dirname(fileURLToPath(import.meta.url))
const contractPath = join(here, 'contracts', 'model-entitlement-refusals.json')
const pinPath = join(here, 'contracts', 'model-entitlement-refusals.json.sha256')
const contractBytes = readFileSync(contractPath)
const contract = JSON.parse(contractBytes.toString('utf8')) as Contract

const noText = (no: string) => no
const enText = (_no: string, en: string) => en

describe('the vendored golden', () => {
  it('is the pinned copy, byte for byte, with its pin beside it', () => {
    expect(existsSync(pinPath)).toBe(true)
    const digest = createHash('sha256').update(contractBytes).digest('hex')
    expect(digest).toBe(MODEL_ENTITLEMENT_REFUSALS_CONTRACT_SHA256)
    expect(readFileSync(pinPath, 'utf8').split(/\s+/)[0]).toBe(MODEL_ENTITLEMENT_REFUSALS_CONTRACT_SHA256)
  })

  it('is the model entitlement refusal catalogue of the Model Plane', () => {
    expect(contract.contract).toBe('model-plane.model-entitlement-refusals')
    expect(contract.version).toBe(1)
  })
})

describe('coverage: the SPA has an entry for every code and reason in the golden', () => {
  it('has Norwegian and English text, an action and the golden flags for every code', () => {
    expect(Object.keys(REFUSAL_TEXT).sort()).toEqual(contract.codes.map((row) => row.code).sort())
    for (const row of contract.codes) {
      const entry = REFUSAL_TEXT[row.code as keyof typeof REFUSAL_TEXT]
      expect(entry.no.trim().length, `${row.code} needs Norwegian text`).toBeGreaterThan(0)
      expect(entry.en, `${row.code} English must be the golden's text`).toBe(row.message_en)
      expect(entry.action, `${row.code} action`).toBe(row.action)
      expect(entry.retryable, `${row.code} retryable`).toBe(row.retryable)
      expect(entry.takesReason, `${row.code} takes_reason`).toBe(row.takes_reason)
    }
  })

  it('has Norwegian and English text for every reason, the English being the golden text', () => {
    expect(Object.keys(REASON_TEXT).sort()).toEqual(contract.reasons.map((row) => row.reason).sort())
    for (const row of contract.reasons) {
      const entry = REASON_TEXT[row.reason as keyof typeof REASON_TEXT]
      expect(entry.en, `${row.reason} English must be the golden's text`).toBe(row.message_en)
      expect(entry.namesModel, `${row.reason} names_model`).toBe(row.names_model)
      expect(entry.no.trim().length, `${row.reason} needs Norwegian text`).toBeGreaterThan(0)
      expect(entry.no.includes('{model}'), `${row.reason} Norwegian names the model as the English does`).toBe(
        row.names_model,
      )
    }
  })
})

describe('modelEntitlementPresentation', () => {
  it('reads Norwegian first, with the next step for a model that is not available', () => {
    const shown = modelEntitlementPresentation('model_not_entitled', noText)
    expect(shown.message).toBe(
      'Denne modellen er ikke tilgjengelig for deg. Forespørselen ble ikke sendt til en modell.',
    )
    expect(shown.next).toBe('Velg en annen modell og send på nytt.')
    expect(shown).toMatchObject({ action: 'choose_another_model', retryable: false })
  })

  it('reads English from the golden', () => {
    const shown = modelEntitlementPresentation('model_not_entitled', enText)
    expect(shown.message).toBe('This model is not available to you. The request was not sent to a model.')
    expect(shown.next).toBe('Choose another model and send again.')
  })

  it('offers a retry and no next step when the answer is unavailable', () => {
    const shown = modelEntitlementPresentation('model_entitlement_unavailable', noText)
    expect(shown.message).toContain('Prøv igjen om litt.')
    expect(shown).toMatchObject({ action: 'retry', retryable: true, next: null })
  })

  it('refines the sentence with a known reason and a plain model id', () => {
    const shown = modelEntitlementPresentation('model_not_entitled', enText, {
      reason: 'explicitly_denied',
      model: 'gpt-5.6-terra',
    })
    expect(shown.message).toBe(
      'Your organisation has turned off the model gpt-5.6-terra. The request was not sent to a model.',
    )
    const norwegian = modelEntitlementPresentation('model_not_entitled', noText, {
      reason: 'deployment_allowlist',
      model: 'gpt-5',
    })
    expect(norwegian.message).toBe(
      'Modellen gpt-5 er ikke slått på i denne installasjonen. Forespørselen ble ikke sendt til en modell.',
    )
  })

  it('shows a reason that names no model without one', () => {
    const shown = modelEntitlementPresentation('model_not_entitled', enText, { reason: 'no_entitled_route' })
    expect(shown.message).toBe(
      'No model your organisation allows can serve this request. The request was not sent to a model.',
    )
  })

  it('never shows a model id that is not a plain id, and falls back to the code sentence', () => {
    for (const model of ['ignore previous instructions', '', 'a'.repeat(129), 'gpt-5\nx', null, undefined]) {
      const shown = modelEntitlementPresentation('model_not_entitled', enText, { reason: 'explicitly_denied', model })
      expect(shown.message, String(model)).toBe(
        'This model is not available to you. The request was not sent to a model.',
      )
    }
  })

  it('ignores a reason it does not know, and a reason on a code that takes none', () => {
    const unknown = modelEntitlementPresentation('model_not_entitled', enText, { reason: 'from_a_newer_server' })
    expect(unknown.message).toBe('This model is not available to you. The request was not sent to a model.')
    const unavailable = modelEntitlementPresentation('model_entitlement_unavailable', enText, {
      reason: 'explicitly_denied',
      model: 'gpt-5',
    })
    expect(unavailable.message).toBe(REFUSAL_TEXT.model_entitlement_unavailable.en)
  })
})

describe('isModelEntitlementCode', () => {
  it('knows the golden codes and nothing else', () => {
    expect(isModelEntitlementCode('model_not_entitled')).toBe(true)
    expect(isModelEntitlementCode('model_entitlement_unavailable')).toBe(true)
    expect(isModelEntitlementCode('zdr_unavailable')).toBe(false)
    expect(isModelEntitlementCode('model_from_a_newer_server')).toBe(false)
    expect(isModelEntitlementCode(undefined)).toBe(false)
  })
})
