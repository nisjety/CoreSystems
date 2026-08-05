// @vitest-environment jsdom

import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import { SupportAssistDisclosure } from './SupportAssistDisclosure'

afterEach(cleanup)

describe('SupportAssistDisclosure', () => {
  it('shows the exact policy, configured retention posture, and reported model for a current Support run', () => {
    render(() => (
      <SupportAssistDisclosure
        metadata={{ model: 'verevon-balance', supportAiMode: 'review', zdr: true }}
      />
    ))

    expect(screen.getByRole('region', { name: /run information|kjøringsinformasjon/i })).toBeTruthy()
    expect(screen.getByText(/review mode|gjennomgangsmodus/i)).toBeTruthy()
    expect(screen.getByText(/zero data retention|ingen datalagring/i)).toBeTruthy()
    expect(screen.getByText('verevon-balance')).toBeTruthy()
    expect(screen.getByText(/confidence and cost were not reported|sikkerhet og kostnad ble ikke rapportert/i)).toBeTruthy()
  })

  it('does not fabricate a model identity when the response omits it', () => {
    render(() => (
      <SupportAssistDisclosure
        metadata={{ supportAiMode: 'assist', zdr: false }}
      />
    ))

    expect(screen.getByText(/assist mode|assist-modus/i)).toBeTruthy()
    expect(screen.getByText(/standard retention|standard lagring/i)).toBeTruthy()
    expect(screen.getByText(/model identity was not reported|modellidentitet ble ikke rapportert/i)).toBeTruthy()
  })

  it('renders only the exact server-reported usage metadata', () => {
    render(() => (
      <SupportAssistDisclosure
        metadata={{
          model: 'verevon-balance',
          supportAiMode: 'review',
          zdr: false,
          usage: {
            inputTokens: 42,
            outputTokens: 18,
            costUsd: 0.00042,
            latencyMs: 321,
            confidence: 0.78,
          },
        }}
      />
    ))

    expect(screen.getByText(/42 (input|inndata) · 18 (output tokens|utdata-tokens)/i)).toBeTruthy()
    expect(screen.getByText(/0\.00042 usd/i)).toBeTruthy()
    expect(screen.getByText(/321 ms/i)).toBeTruthy()
    expect(screen.getByText(/78% (heuristic answer-quality signal|heuristisk signal for svarkvalitet)/i)).toBeTruthy()
    expect(screen.queryByText(/confidence and cost were not reported/i)).toBeNull()
  })
})
