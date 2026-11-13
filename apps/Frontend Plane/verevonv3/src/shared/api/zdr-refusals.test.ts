// @vitest-environment jsdom

/**
 * The Norwegian and English rendering of every typed zero-data-retention
 * refusal (Z-05b). The codes, the English fallback of each, the action and the
 * feature vocabulary are the Model Plane golden. This SPA holds a byte-identical
 * copy of it inside its own tree (`./contracts`, pinned by sha256 beside it), so
 * the checks below read nothing outside this tree: a golden that is missing
 * fails here instead of passing by not running. The repo-level drift check
 * (scripts/tests/zdr-contract-sync.test.sh) compares the copy with the golden.
 */

import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import {
  FEATURE_LABELS,
  REFUSAL_TEXT,
  ZDR_REFUSALS_CONTRACT_SHA256,
  isZdrCode,
  isZdrRefusalCode,
  zdrRefusalPresentation,
} from './zdr-refusals'

type Contract = {
  codes: Array<{ action: string; code: string; message_en: string; retryable: boolean; takes_feature: boolean }>
  features: Array<{ key: string; label_en: string }>
  contract: string
  version: number
}

const here = dirname(fileURLToPath(import.meta.url))
const contractPath = join(here, 'contracts', 'zdr-refusals.json')
const pinPath = join(here, 'contracts', 'zdr-refusals.json.sha256')
const contractBytes = readFileSync(contractPath)
const contract = JSON.parse(contractBytes.toString('utf8')) as Contract

const noText = (no: string) => no
const enText = (_no: string, en: string) => en

describe('the vendored golden', () => {
  it('is the pinned copy, byte for byte, with its pin beside it', () => {
    expect(existsSync(pinPath)).toBe(true)
    const digest = createHash('sha256').update(contractBytes).digest('hex')
    expect(digest).toBe(ZDR_REFUSALS_CONTRACT_SHA256)
    expect(readFileSync(pinPath, 'utf8').split(/\s+/)[0]).toBe(ZDR_REFUSALS_CONTRACT_SHA256)
  })

  it('is the refusal catalogue of the Model Plane', () => {
    expect(contract.contract).toBe('model-plane.zdr-refusals')
    expect(contract.version).toBe(1)
  })
})

describe('coverage: the SPA has an entry for every code and feature in the golden', () => {
  it('has Norwegian and English text, an action and the golden flags for every code', () => {
    const golden = contract.codes.map((row) => row.code).sort()
    expect(Object.keys(REFUSAL_TEXT).sort()).toEqual(golden)
    for (const row of contract.codes) {
      const entry = REFUSAL_TEXT[row.code as keyof typeof REFUSAL_TEXT]
      expect(entry.no.trim().length, `${row.code} needs Norwegian text`).toBeGreaterThan(0)
      expect(entry.en, `${row.code} English must be the golden's text`).toBe(row.message_en)
      expect(entry.action, `${row.code} action`).toBe(row.action)
      expect(entry.retryable, `${row.code} retryable`).toBe(row.retryable)
      expect(entry.takesFeature, `${row.code} takes_feature`).toBe(row.takes_feature)
      if (row.takes_feature) {
        expect(entry.no, `${row.code} Norwegian text names the feature`).toContain('{feature}')
      }
    }
  })

  it('has a Norwegian and an English label for every feature, and the golden English label', () => {
    const golden = contract.features.map((row) => row.key).sort()
    expect(Object.keys(FEATURE_LABELS).sort()).toEqual(golden)
    for (const row of contract.features) {
      const label = FEATURE_LABELS[row.key as keyof typeof FEATURE_LABELS]
      expect(label.en).toBe(row.label_en)
      expect(label.no.trim().length, `${row.key} needs a Norwegian label`).toBeGreaterThan(0)
      expect(label.no).not.toBe(row.label_en)
    }
  })

  it('treats every golden code as a ZDR refusal, and nothing else as one', () => {
    for (const row of contract.codes) expect(isZdrRefusalCode(row.code)).toBe(true)
    expect(isZdrRefusalCode('zdr_something_a_newer_server_sends')).toBe(false)
    expect(isZdrRefusalCode('subscription_usage_limit')).toBe(false)
    // The prefix is the wider test: an unknown ZDR-named code is still ZDR.
    expect(isZdrCode('zdr_something_a_newer_server_sends')).toBe(true)
    expect(isZdrCode('subscription_usage_limit')).toBe(false)
    expect(isZdrCode(undefined)).toBe(false)
  })
})

describe('rendering each code', () => {
  it('renders every golden code in Norwegian and in English, with its action', () => {
    for (const row of contract.codes) {
      const feature = row.takes_feature ? 'web_search' : undefined
      const no = zdrRefusalPresentation(row.code, feature, noText)
      const en = zdrRefusalPresentation(row.code, feature, enText)
      expect(no.known).toBe(true)
      expect(no.message.length, `${row.code} Norwegian message`).toBeGreaterThan(0)
      expect(no.message).not.toBe(en.message)
      expect(en.action).toBe(row.action)
      expect(no.retryable).toBe(row.retryable)
      if (row.takes_feature) {
        expect(en.message).toContain('web search and browsing')
        expect(no.message).toContain('websøk og nettlesing')
      } else {
        expect(en.message).toBe(row.message_en)
      }
    }
  })

  it('states the Norwegian sentence for a feature refusal, with the feature named', () => {
    const shown = zdrRefusalPresentation('zdr_feature_not_supported', 'web_search', noText)
    expect(shown.message).toBe(
      'Dette støttes ikke med Zero Data Retention ennå: websøk og nettlesing. Forespørselen ble avvist før noe kall til en modell.',
    )
    expect(shown.action).toBe('turn_off_feature')
    expect(shown.next).toBe('Slå av funksjonen og send på nytt.')
  })

  it('names a file-output refusal in Norwegian and English', () => {
    expect(zdrRefusalPresentation('zdr_feature_not_supported', 'artifact_output', noText).message).toContain(
      'filer og dokumenter assistenten oppretter',
    )
    expect(zdrRefusalPresentation('zdr_feature_not_supported', 'artifact_output', enText).message).toContain(
      'files and documents the assistant creates',
    )
  })

  it('names the feature by its stable key, never by the server text', () => {
    const shown = zdrRefusalPresentation('zdr_feature_not_supported', 'agentic', enText)
    expect(shown.message).toBe(
      'This is not supported with zero data retention yet: Do mode. The request was refused before any model call.',
    )
  })

  it('falls back to "this feature" when a feature refusal names no known feature', () => {
    expect(zdrRefusalPresentation('zdr_feature_not_supported', null, enText).message).toContain('this feature')
    expect(zdrRefusalPresentation('zdr_feature_not_supported', 'not_in_the_vocabulary', noText).message).toContain(
      'denne funksjonen',
    )
  })

  it('gives the way forward for the actions that have one, and none for the rest', () => {
    const forAction = (code: string) => zdrRefusalPresentation(code, undefined, enText)
    // Open the connection to review it, then send again.
    expect(forAction('zdr_connection_not_acknowledged')).toMatchObject({ action: 'open_integrations' })
    expect(forAction('zdr_connection_not_acknowledged').next).toBe(
      'Go to the ChatGPT connection, review it, and send again.',
    )
    // Choose another model, then send again.
    expect(forAction('zdr_unavailable')).toMatchObject({ action: 'choose_another_model' })
    expect(forAction('zdr_unavailable').next).toBe('Choose another model and send again.')
    // Retry is the button; the text need not repeat it.
    expect(forAction('zdr_inference_failed')).toMatchObject({ action: 'retry', retryable: true, next: null })
    // Nothing the reader can do here.
    expect(forAction('zdr_posture_mismatch')).toMatchObject({ action: 'none', next: null })
  })

  it('uses the Norwegian next step for Norwegian readers', () => {
    expect(zdrRefusalPresentation('zdr_connection_not_acknowledged', undefined, noText).next).toBe(
      'Gå til ChatGPT-tilkoblingen, se over den, og send på nytt.',
    )
  })

  it('never states that nothing was sent to a model when the code is unknown', () => {
    const shown = zdrRefusalPresentation('zdr_something_a_newer_server_sends', null, enText)
    expect(shown.known).toBe(false)
    expect(shown.action).toBe('none')
    expect(shown.retryable).toBe(false)
    expect(shown.next).toBeNull()
    expect(shown.message).toBe(
      'The request could not be completed with zero data retention. Contact support if the problem persists.',
    )
    expect(shown.message).not.toMatch(/not sent|ikke sendt/i)
  })

  it('shows the same generic text in Norwegian for an unknown code', () => {
    expect(zdrRefusalPresentation('zdr_something_a_newer_server_sends', null, noText).message).toBe(
      'Forespørselen med Zero Data Retention kunne ikke fullføres. Kontakt support hvis problemet vedvarer.',
    )
  })
})
