import { describe, expect, it } from 'vitest'
import { canvasErrorMessage, translateApiError } from './errors'

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

// The room canvas (WS19-S5) words its own refusals. A canvas route keeps the code it answers with, but the Space denial
// words and the posting words share some of those codes, so canvasErrorMessage answers before translateApiError does.
describe('canvasErrorMessage', () => {
  it('gives the canvas sentence for canvas_read_not_authorized with details.reason space_entitlement_off', () => {
    const refusal = { code: 'canvas_read_not_authorized', status: 403, details: { reason: 'space_entitlement_off' } }
    expect(canvasErrorMessage(refusal, en)).toBe('Canvas is not enabled for this organization or you cannot read it.')
    expect(canvasErrorMessage(refusal, no)).toBe('Lerret er ikke slått på for denne organisasjonen, eller du kan ikke lese det.')
    // translateApiError reads the same refusal as the Space denial for that reason; the canvas does not.
    expect(translateApiError(refusal, en)).toMatch(/for Spaces/)
    expect(canvasErrorMessage(refusal, en)).not.toMatch(/Spaces/)
  })

  it('gives the canvas sentence for space_decision_unavailable, not the posting sentence', () => {
    const refusal = { code: 'space_decision_unavailable', status: 503 }
    expect(canvasErrorMessage(refusal, en)).toBe('Access to the canvas could not be confirmed right now. Nothing was changed. Try again shortly.')
    expect(canvasErrorMessage(refusal, no)).toBe('Tilgangen til lerretet kunne ikke bekreftes akkurat nå. Ingenting er endret. Prøv igjen om litt.')
    expect(canvasErrorMessage(refusal, en)).not.toBe(translateApiError(refusal, en))
  })

  it('gives the canvas sentence for idempotency_key_conflict and invalid_cursor', () => {
    expect(canvasErrorMessage({ code: 'idempotency_key_conflict', status: 409 }, en)).toBe(
      'This save was already sent with different content. Reload the canvas and save again.',
    )
    expect(canvasErrorMessage({ code: 'idempotency_key_conflict', status: 409 }, no)).toBe(
      'Lagringen er allerede sendt med annet innhold. Last lerretet inn på nytt og lagre igjen.',
    )
    expect(canvasErrorMessage({ code: 'invalid_cursor', status: 400 }, en)).toBe('That part of the history is no longer valid. Load it again.')
    expect(canvasErrorMessage({ code: 'invalid_cursor', status: 400 }, no)).toBe('Historikken er ikke gyldig lenger. Last den inn på nytt.')
  })

  it('gives the history-floor sentence exactly for space_history_floor_unsupported', () => {
    expect(canvasErrorMessage({ code: 'space_history_floor_unsupported', status: 403 }, en)).toBe(
      'Canvas is not available in rooms that show history from your join date.',
    )
  })

  it('other codes fall back to translateApiError', () => {
    const outage = { code: 'upstream_unavailable', status: 503 }
    expect(canvasErrorMessage(outage, en)).toBe(translateApiError(outage, en))
    const notFound = { code: 'space_not_found', status: 404 }
    expect(canvasErrorMessage(notFound, no)).toBe(translateApiError(notFound, no))
  })
})
