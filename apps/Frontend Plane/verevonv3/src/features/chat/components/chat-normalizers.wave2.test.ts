/**
 * Pure pieces of the wave-2 chat fixes: how a server failure is presented, the
 * frame-keyed step identity a resume relies on, what survives a reload, and
 * the metadata a turn never carries.
 */

import { describe, expect, it } from 'vitest'
import type { ChatThreadTranscriptTurn } from '@/features/chat/lib/chat-thread-history'
import { deriveConversationNodes } from '@/shared/chat-nodes'
import {
  formatResetTime,
  mergeServerTurnsWithCachedMetadata,
  refreshResetTimeWording,
  normalizeStep,
  streamFailurePresentation,
  transcriptTurnToChatTurn,
  turnsToTranscript,
} from './chat-normalizers'
import type { ChatTurn } from './chat-types'

const norwegian = (no: string, _en: string) => no
const english = (_no: string, en: string) => en

const assistant = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'a1',
  role: 'assistant',
  content: 'Svaret.',
  createdAt: '2026-09-24T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

describe('streamFailurePresentation', () => {
  it('keeps the route-specific message for an unavailable model service, and marks it as an outage', () => {
    const message = 'Den valgte ChatGPT-abonnementstilkoblingen svarte ikke mens utkastet ble kontrollert mot kildene.'
    expect(streamFailurePresentation('response_validation_unavailable', message, norwegian)).toEqual({
      message,
      unsaved: true,
      unavailable: true,
    })
    expect(streamFailurePresentation('response_validation_unavailable', '  ', english)).toMatchObject({
      message: expect.stringContaining('did not answer'),
      unavailable: true,
    })
  })

  it('translates the two codes that arrive with operator English and says nothing was saved', () => {
    const ended = streamFailurePresentation(
      'inference_stream_ended_without_terminal',
      'The answer stream ended before it was complete. Please try again.',
      norwegian,
    )
    expect(ended).toMatchObject({ unsaved: true, unavailable: false })
    expect(ended.message).toContain('ingenting ble lagret')
    const liveness = streamFailurePresentation('session_liveness_failed', 'The chat run is no longer active. Please retry.', english)
    expect(liveness).toMatchObject({ unsaved: true, unavailable: false })
    expect(liveness.message).toContain('was not saved')
  })

  it('says an answer that streamed but failed to save was not saved, in the reader\'s language', () => {
    const shown = streamFailurePresentation('assistant_persist_failed', 'Unable to persist the assistant response.', norwegian)
    expect(shown).toEqual({
      message: 'Svaret ble laget, men kunne ikke lagres. Prøv igjen.',
      unsaved: true,
      unavailable: false,
    })
    expect(streamFailurePresentation('assistant_persist_failed', 'Unable to persist the assistant response.', english).message)
      .toBe('The answer was produced but could not be saved. Try again.')
  })

  it('keeps the gateway\'s Norwegian message for a document revision conflict, and marks the answer unsaved', () => {
    const message = 'Dokumentet ble oppdatert av en annen kjøring. Last inn den nyeste versjonen og prøv endringen igjen.'
    expect(streamFailurePresentation('artifact_revision_conflict', message, norwegian)).toEqual({
      message,
      unsaved: true,
      unavailable: false,
    })
    expect(streamFailurePresentation('artifact_revision_conflict', '', english)).toMatchObject({
      message: expect.stringContaining('updated by another run'),
      unsaved: true,
    })
  })

  it('leaves every other code as the server said it', () => {
    expect(streamFailurePresentation('response_validation_failed', 'Ikke publisert.', norwegian)).toEqual({
      message: 'Ikke publisert.',
      unsaved: false,
      unavailable: false,
    })
    expect(streamFailurePresentation(undefined, '', norwegian).message).toBe('Stream error')
  })
})

describe('streamFailurePresentation for ChatGPT-subscription refusals', () => {
  // 2026-09-25 14:00 UTC; the plan resets at 01:21 UTC the next night.
  const NOW = Date.parse('2026-09-25T14:00:00Z')
  const RESET = '2026-09-26T01:21:00Z'
  const utc = { now: NOW, timeZone: 'UTC' }
  // What the broker's usage-limit text looks like; the chat never shows it.
  const SERVER = 'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles 26.09.2026 kl. 01:21 UTC. Ingen annen modell ble brukt.'

  it('names the usage limit and its local reset time, and waits for it', () => {
    expect(streamFailurePresentation('subscription_usage_limit', SERVER, norwegian, { ...utc, resetsAt: RESET, locale: 'no' })).toEqual({
      message: 'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles i morgen kl. 01:21. Ingen annen modell ble brukt.',
      unsaved: true,
      unavailable: true,
      action: 'wait',
      retryAt: RESET,
      serviceTitle: 'ChatGPT-planen har nådd bruksgrensen',
    })
    const english = streamFailurePresentation('subscription_usage_limit', SERVER, (_no, en) => en, { ...utc, resetsAt: RESET, locale: 'en' })
    expect(english.message).toMatch(/^The connected ChatGPT plan has reached its Codex usage limit\. It resets tomorrow at 1:21\sAM\. No other model was used\.$/)
    expect(english.serviceTitle).toBe('The ChatGPT plan reached its usage limit')
  })

  it('reads the reset time in the viewer\'s own zone', () => {
    // 01:21 UTC is 03:21 in Oslo (CEST).
    const shown = streamFailurePresentation('subscription_usage_limit', '', norwegian, { now: NOW, resetsAt: RESET, locale: 'no', timeZone: 'Europe/Oslo' })
    expect(shown.message).toContain('Den tilbakestilles i morgen kl. 03:21.')
  })

  it('takes the language for the reset time from tr when no locale is given', () => {
    expect(streamFailurePresentation('subscription_usage_limit', '', english, { ...utc, resetsAt: RESET }).message)
      .toMatch(/It resets tomorrow at 1:21\sAM\./)
  })

  it('waits for a known delay when the reset instant is missing', () => {
    const shown = streamFailurePresentation('subscription_usage_limit', '', norwegian, { ...utc, retryAfterMs: 90 * 60_000, locale: 'no' })
    expect(shown).toMatchObject({ action: 'wait', retryAt: '2026-09-25T15:30:00.000Z', unavailable: true, unsaved: true })
    expect(shown.message).toContain('Den tilbakestilles i dag kl. 15:30.')
  })

  it('offers a manual retry only, with honest words, when the reset time is unknown', () => {
    const shown = streamFailurePresentation('subscription_usage_limit', SERVER, norwegian, utc)
    expect(shown).toEqual({
      message: 'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Tidspunktet for tilbakestilling er ukjent; sjekk bruken i ChatGPT og prøv igjen senere. Ingen annen modell ble brukt.',
      unsaved: true,
      unavailable: true,
      action: 'retry',
      serviceTitle: 'ChatGPT-planen har nådd bruksgrensen',
    })
    expect(streamFailurePresentation('subscription_usage_limit', '', english, utc).message)
      .toBe('The connected ChatGPT plan has reached its Codex usage limit. The reset time is unknown; check your usage in ChatGPT and try again later. No other model was used.')
  })

  it('does not name a reset time that has already passed', () => {
    const shown = streamFailurePresentation('subscription_usage_limit', SERVER, norwegian, { ...utc, now: Date.parse('2026-09-26T02:00:00Z'), resetsAt: RESET, retryAfterMs: 60_000 })
    expect(shown).toMatchObject({ action: 'retry', unavailable: true })
    expect(shown.retryAt).toBeUndefined()
    expect(shown.message).toBe('Den tilkoblede ChatGPT-planen nådde bruksgrensen for Codex, men grensen skal nå være tilbakestilt. Prøv igjen. Ingen annen modell ble brukt.')
    expect(streamFailurePresentation('subscription_usage_limit', '', english, { now: Date.parse('2026-09-26T02:00:00Z'), resetsAt: RESET }).message)
      .toBe('The connected ChatGPT plan reached its Codex usage limit, but the limit should have reset by now. Try again. No other model was used.')
  })

  it('sends the reader to reconnect ChatGPT when the sign-in must be renewed', () => {
    expect(streamFailurePresentation('subscription_reauth_required', 'x', norwegian, utc)).toEqual({
      message: 'ChatGPT-tilkoblingen må fornyes. Koble til ChatGPT på nytt under Profil → ChatGPT-abonnement, og prøv igjen. Ingen annen modell ble brukt.',
      unsaved: true,
      unavailable: true,
      action: 'reconnect',
      serviceTitle: 'ChatGPT-tilkoblingen må fornyes',
    })
    expect(streamFailurePresentation('subscription_reauth_required', '', english, utc)).toMatchObject({
      message: 'Your ChatGPT connection needs to be renewed. Reconnect ChatGPT under Profile → ChatGPT subscription, then try again. No other model was used.',
      serviceTitle: 'The ChatGPT connection needs renewing',
    })
  })

  it('offers a retry for a transient ChatGPT failure', () => {
    expect(streamFailurePresentation('subscription_upstream_unavailable', 'x', norwegian, utc)).toEqual({
      message: 'ChatGPT svarte ikke akkurat nå. Prøv igjen om litt. Ingen annen modell ble brukt.',
      unsaved: true,
      unavailable: true,
      action: 'retry',
      serviceTitle: 'ChatGPT svarte ikke',
    })
    expect(streamFailurePresentation('subscription_upstream_unavailable', '', english, utc)).toMatchObject({
      message: 'ChatGPT did not answer just now. Try again shortly. No other model was used.',
      serviceTitle: 'ChatGPT did not answer',
    })
  })

  it('waits out a throttle for the seconds the server gave, and otherwise offers a retry', () => {
    expect(streamFailurePresentation('rate_limited', 'x', norwegian, { ...utc, retryAfterMs: 2_500 })).toEqual({
      message: 'ChatGPT begrenser forespørslene akkurat nå. Prøv igjen om 3 sekunder. Ingen annen modell ble brukt.',
      unsaved: true,
      unavailable: true,
      action: 'wait',
      retryAt: '2026-09-25T14:00:02.500Z',
      serviceTitle: 'ChatGPT svarte ikke',
    })
    expect(streamFailurePresentation('rate_limited', '', english, { ...utc, retryAfterMs: 400 }).message)
      .toBe('ChatGPT is limiting requests right now. Try again in 1 second. No other model was used.')
    expect(streamFailurePresentation('rate_limited', '', norwegian, utc)).toMatchObject({
      message: 'ChatGPT begrenser forespørslene akkurat nå. Prøv igjen om litt. Ingen annen modell ble brukt.',
      action: 'retry',
    })
    expect(streamFailurePresentation('rate_limited', '', norwegian, utc).retryAt).toBeUndefined()
  })

  it('offers no retry for a request ChatGPT rejected, and does not call it an outage', () => {
    expect(streamFailurePresentation('subscription_bad_request', 'x', norwegian, utc)).toEqual({
      message: 'ChatGPT avviste forespørselen. Endre eller kort ned meldingen, og prøv igjen. Ingen annen modell ble brukt.',
      unsaved: true,
      unavailable: false,
      action: 'none',
    })
    expect(streamFailurePresentation('subscription_bad_request', '', english, utc).message)
      .toBe('ChatGPT rejected the request. Change or shorten the message, then try again. No other model was used.')
  })

  it('names the subscription connection for the gateway\'s generic subscription failure', () => {
    expect(streamFailurePresentation('subscription_route_unavailable', 'Koble til abonnementet på nytt.', norwegian, utc)).toEqual({
      message: 'Den valgte ChatGPT-abonnementstilkoblingen kunne ikke fullføre svaret. Prøv igjen om litt. Ingen annen modell ble brukt.',
      unsaved: true,
      unavailable: true,
      action: 'retry',
      serviceTitle: 'ChatGPT svarte ikke',
    })
    expect(streamFailurePresentation('subscription_route_unavailable', '', english, utc).message)
      .toBe('The selected ChatGPT subscription connection could not complete the answer. Try again shortly. No other model was used.')
  })

  it('says Do is not switched on for the subscription, in words, and offers no retry of the refused turn', () => {
    // What model-gateway sends when its AGENTIC_SUBSCRIPTION_ENABLED switch is off.
    const operatorEnglish = 'openai-codex-subscription supports text-only, non-ZDR chat without image generation, browsing, deep research, plan mode, or agentic tool specs'
    const shown = streamFailurePresentation('agentic_subscription_disabled', operatorEnglish, norwegian, utc)
    expect(shown).toEqual({
      message: 'Utfør er ikke slått på for ChatGPT-abonnementet. Velg Spør, eller velg en annen modell for å bruke Utfør. Ingen modell ble brukt.',
      unsaved: true,
      unavailable: false,
      action: 'none',
    })
    expect(shown.message).not.toContain('text-only')
    expect(streamFailurePresentation('agentic_subscription_disabled', operatorEnglish, english, utc).message)
      .toBe('Do is not switched on for the ChatGPT subscription. Choose Ask, or pick another model to use Do. No model was used.')
  })

  it('translates a conversation that is too long, and offers no retry of the same turn', () => {
    const operatorEnglish = 'This conversation is too long for the selected model, even after compacting it. Start a new thread, or choose a model with a larger context window.'
    expect(streamFailurePresentation('prompt_too_long', operatorEnglish, norwegian, utc)).toEqual({
      message: 'Samtalen er for lang for modellen, selv etter komprimering. Start en ny tråd.',
      unsaved: true,
      unavailable: false,
      action: 'none',
    })
    expect(streamFailurePresentation('prompt_too_long', operatorEnglish, english, utc).message)
      .toBe('This conversation is too long for the model, even after compacting it. Start a new thread.')
  })

  it('translates an empty answer and a model service outage, and never shows the raw transport text', () => {
    expect(streamFailurePresentation('inference_stream_error', 'The inference stream ended unexpectedly.', norwegian, utc)).toEqual({
      message: 'Modellen svarte ikke denne gangen. Prøv igjen.',
      unsaved: true,
      unavailable: false,
    })
    expect(streamFailurePresentation('inference_stream_error', '', english, utc).message)
      .toBe('The model did not return an answer this time. Try again.')
    const raw = 'status: Unavailable, message: "subscription refused", metadata: MetadataMap { headers: {} }'
    const outage = streamFailurePresentation('model_plane_unavailable', raw, norwegian, utc)
    expect(outage).toEqual({ message: 'Modelltjenesten svarte ikke. Prøv igjen om litt.', unsaved: true, unavailable: true })
    expect(streamFailurePresentation('model_plane_unavailable', raw, english, utc).message)
      .toBe('The model service did not answer. Try again shortly.')
  })
})

describe('formatResetTime', () => {
  const NOW = Date.parse('2026-09-25T14:00:00Z') // a Friday

  it('says today, tomorrow, or the weekday and date, in the reader\'s language', () => {
    expect(formatResetTime('2026-09-25T18:05:00Z', 'no', NOW, 'UTC')).toBe('i dag kl. 18:05')
    expect(formatResetTime('2026-09-26T01:21:00Z', 'no', NOW, 'UTC')).toBe('i morgen kl. 01:21')
    expect(formatResetTime('2026-09-28T09:00:00Z', 'no', NOW, 'UTC')).toBe('mandag 28. september kl. 09:00')
    expect(formatResetTime('2026-09-25T18:05:00Z', 'en', NOW, 'UTC')).toMatch(/^today at 6:05\sPM$/)
    expect(formatResetTime('2026-09-26T01:21:00Z', 'en', NOW, 'UTC')).toMatch(/^tomorrow at 1:21\sAM$/)
    expect(formatResetTime('2026-09-28T09:00:00Z', 'en', NOW, 'UTC')).toMatch(/^Monday, September 28 at 9:00\sAM$/)
  })

  it('decides today and tomorrow by the viewer\'s calendar, not by UTC', () => {
    // 23:30 UTC on the 25th is already the 26th in Oslo.
    const lateNow = Date.parse('2026-09-25T23:30:00Z')
    expect(formatResetTime('2026-09-26T01:21:00Z', 'no', lateNow, 'Europe/Oslo')).toBe('i dag kl. 03:21')
    expect(formatResetTime('2026-09-26T01:21:00Z', 'no', lateNow, 'UTC')).toBe('i morgen kl. 01:21')
  })

  it('crosses a month end, and gives nothing for an instant that does not parse', () => {
    expect(formatResetTime('2026-10-01T08:00:00Z', 'no', Date.parse('2026-09-30T12:00:00Z'), 'UTC')).toBe('i morgen kl. 08:00')
    expect(formatResetTime('tomorrow', 'no', NOW, 'UTC')).toBe('')
  })

  it('rounds a reset inside a minute up to the next one, never down to a minute a retry is still refused', () => {
    // The retry control opens at 01:21:40, so "01:21" would name a minute too early.
    expect(formatResetTime('2026-09-26T01:21:40Z', 'no', NOW, 'UTC')).toBe('i morgen kl. 01:22')
    expect(formatResetTime('2026-09-26T01:21:00.001Z', 'no', NOW, 'UTC')).toBe('i morgen kl. 01:22')
    expect(formatResetTime('2026-09-26T01:21:40Z', 'en', NOW, 'UTC')).toMatch(/^tomorrow at 1:22\sAM$/)
    // A whole minute is shown as it is.
    expect(formatResetTime('2026-09-26T01:21:00Z', 'no', NOW, 'UTC')).toBe('i morgen kl. 01:21')
    // Rounding past midnight moves the day with it.
    expect(formatResetTime('2026-09-25T23:59:30Z', 'no', NOW, 'UTC')).toBe('i morgen kl. 00:00')
  })

  it('rounds the usage-limit message the same way, and keeps the exact instant for the retry control', () => {
    const shown = streamFailurePresentation('subscription_usage_limit', '', norwegian, {
      now: NOW, resetsAt: '2026-09-26T01:21:40Z', locale: 'no', timeZone: 'UTC',
    })
    expect(shown.message).toBe('Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles i morgen kl. 01:22. Ingen annen modell ble brukt.')
    expect(shown.retryAt).toBe('2026-09-26T01:21:40Z')
  })
})

describe('refreshResetTimeWording', () => {
  const NOW_FRIDAY = Date.parse('2026-09-25T14:00:00Z')
  const RESET = '2026-09-26T01:21:40Z'
  const written = 'Den tilbakestilles i morgen kl. 01:22. Ingen annen modell ble brukt.'

  it('says "i dag" once the day it was written for has come', () => {
    const afterMidnight = Date.parse('2026-09-26T00:10:00Z')
    expect(refreshResetTimeWording(written, RESET, 'no', afterMidnight, 'UTC'))
      .toBe('Den tilbakestilles i dag kl. 01:22. Ingen annen modell ble brukt.')
    // English, built with the same formatter, so the narrow space Intl puts before "AM" matches.
    const englishWritten = `It resets ${formatResetTime(RESET, 'en', Date.parse('2026-09-25T20:00:00Z'), 'UTC')}. No other model was used.`
    expect(englishWritten).toMatch(/^It resets tomorrow at 1:22\sAM\./)
    expect(refreshResetTimeWording(englishWritten, RESET, 'en', afterMidnight, 'UTC'))
      .toMatch(/^It resets today at 1:22\sAM\. No other model was used\.$/)
  })

  it('keeps the weekday wording until the day before, then says "i morgen"', () => {
    const monday = '2026-09-28T09:00:00Z'
    const farAhead = `Den tilbakestilles ${formatResetTime(monday, 'no', NOW_FRIDAY, 'UTC')}.`
    expect(farAhead).toBe('Den tilbakestilles mandag 28. september kl. 09:00.')
    expect(refreshResetTimeWording(farAhead, monday, 'no', Date.parse('2026-09-27T08:00:00Z'), 'UTC'))
      .toBe('Den tilbakestilles i morgen kl. 09:00.')
  })

  it('leaves a message alone that is already current, or has no reset phrase of its own', () => {
    const evening = Date.parse('2026-09-25T20:00:00Z')
    expect(refreshResetTimeWording(written, RESET, 'no', evening, 'UTC')).toBe(written)
    expect(refreshResetTimeWording('Bruksgrensen er nådd.', RESET, 'no', evening, 'UTC')).toBe('Bruksgrensen er nådd.')
    // Written in another language, or about another instant: not ours to rewrite.
    expect(refreshResetTimeWording(written, RESET, 'en', evening, 'UTC')).toBe(written)
    expect(refreshResetTimeWording(written, '2026-09-26T09:00:00Z', 'no', evening, 'UTC')).toBe(written)
    expect(refreshResetTimeWording(written, 'soon', 'no', evening, 'UTC')).toBe(written)
  })
})

describe('normalizeStep frame identity', () => {
  it('keys a step without its own id on the frame that carried it', () => {
    const first = normalizeStep({ title: 'Kontrollerer kildene', status: 'running', frameId: '5' }, 'asst-1', 'Q')
    const replayed = normalizeStep({ title: 'Kontrollerer kildene', status: 'running', frameId: '5' }, 'asst-1', 'Q')
    const later = normalizeStep({ title: 'Kontrollerer kildene', status: 'done', frameId: '9' }, 'asst-1', 'Q')
    expect(first?.id).toBe('asst-1:event-frame-5')
    expect(replayed?.id).toBe(first?.id)
    expect(later?.id).not.toBe(first?.id)
  })

  it("prefers the step's own id, and stays unique without either", () => {
    expect(normalizeStep({ id: 'draft-1:source-check', title: 'x', frameId: '5' }, 'asst-1')?.id).toBe('asst-1:event-draft-1:source-check')
    const a = normalizeStep({ title: 'Uten id' }, 'asst-1')
    const b = normalizeStep({ title: 'Uten id' }, 'asst-1')
    expect(a?.id).not.toBe(b?.id)
  })
})

describe('what a reload keeps', () => {
  it('keeps the cost basis and the three timing totals, and never the unsaved text', () => {
    const live = assistant({
      costBasis: 'subscription',
      timing: {
        serverMs: 43_440,
        providerMs: 43_220,
        orchestrationMs: 220,
        outcome: 'completed',
        phases: [{ phase: 'answer_stream', startMs: 100, endMs: 43_300 }],
      },
      status: 'error',
      content: 'Svaret ble ikke lagret.',
      unsavedContent: 'Hele svaret er her.',
    })
    const [stored] = turnsToTranscript([live])
    expect(stored).toMatchObject({
      costBasis: 'subscription',
      timing: { serverMs: 43_440, providerMs: 43_220, orchestrationMs: 220 },
    })
    expect(stored?.timing).not.toHaveProperty('phases')
    expect(JSON.stringify(stored)).not.toContain('Hele svaret er her.')

    const restored = transcriptTurnToChatTurn(stored!)
    expect(restored.costBasis).toBe('subscription')
    expect(restored.timing).toEqual({ serverMs: 43_440, providerMs: 43_220, orchestrationMs: 220 })
    expect(restored.unsavedContent).toBeUndefined()
  })

  it('drops a stored timing summary that is missing a total, and a cost basis it cannot read', () => {
    const restored = transcriptTurnToChatTurn({
      id: 'a1',
      role: 'assistant',
      content: 'Svaret.',
      createdAt: '2026-09-24T08:00:00.000Z',
      timing: { serverMs: 100, providerMs: 80 } as unknown as ChatThreadTranscriptTurn['timing'],
      costBasis: 'free' as unknown as ChatThreadTranscriptTurn['costBasis'],
    })
    expect(restored.timing).toBeUndefined()
    expect(restored.costBasis).toBeUndefined()
  })

  it('carries the cached stream-only fields onto the server-saved answer', () => {
    const server = [
      assistant({ id: 'u1', role: 'user', content: 'Spørsmål' }),
      assistant({ id: 'srv-a1' }),
    ]
    const cached = [
      assistant({ id: 'u1', role: 'user', content: 'Spørsmål' }),
      assistant({
        costBasis: 'subscription',
        timing: { serverMs: 1_000, providerMs: 900, orchestrationMs: 100 },
        verification: { verdict: 'supports', kbCitations: 1, webCitations: 0, webAllowed: false },
      }),
    ]
    const merged = mergeServerTurnsWithCachedMetadata(server, cached).at(1)
    expect(merged).toMatchObject({
      id: 'srv-a1',
      costBasis: 'subscription',
      timing: { serverMs: 1_000, providerMs: 900, orchestrationMs: 100 },
      verification: { verdict: 'supports' },
    })
  })
})

describe('head_summary', () => {
  it('is never carried onto a turn or rendered, even when a stored turn holds it', () => {
    const summary = 'Tidligere i samtalen: brukeren ba om et budsjett for Q3.'
    const stored = {
      id: 'a1',
      role: 'assistant',
      content: 'Svaret.',
      createdAt: '2026-09-24T08:00:00.000Z',
      head_summary: { summary, through_id: 'm0', pinned_ids: [] },
      metadata: { head_summary: { summary } },
    } as unknown as ChatThreadTranscriptTurn
    const turn = transcriptTurnToChatTurn(stored)
    expect(JSON.stringify(turn)).not.toContain(summary)
    expect(JSON.stringify(deriveConversationNodes(turn))).not.toContain(summary)
    expect(JSON.stringify(turnsToTranscript([turn]))).not.toContain(summary)
  })
})
