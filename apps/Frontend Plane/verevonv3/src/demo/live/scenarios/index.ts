/**
 * The demo's cases, in the order the start screen offers them, and the
 * transcripts of seeded conversations that belong to a case. No case seeds a
 * conversation now: the three cases start from the dashboard's cards, and their
 * threads appear in the recent list once a visitor has run them.
 */
import type { Scenario } from '../scenario-types'
import type { ThreadRow } from '../shared-types'
import type { StoredMessage } from '../thread-store'
import { anbudScenario } from './anbud-miljodokumentasjon'
import { reklamasjonScenario } from './reklamasjon-bordplate'
import { tilbudScenario } from './tilbud-solhaug'

export const scenarios: readonly Scenario[] = [tilbudScenario, reklamasjonScenario, anbudScenario]

/** The model id seeded answers report (a fixture id from fixtures/fjordform.ts). */
const SEED_MODEL = 'nordisk-standard'

export function scenarioById(id: string): Scenario | undefined {
  return scenarios.find((scenario) => scenario.id === id)
}

/**
 * The transcripts of the seeded conversations that belong to a case: the case's
 * question, then the answer it gives. A case seeds a thread only through its
 * `seedThreadId`, and only when the seed lists that thread. The outcome of its
 * gate is not replayed, since the approval it waits for is gone with the page.
 * `cases` defaults to the registry.
 */
export function seededTranscripts(
  seed: readonly ThreadRow[],
  cases: readonly Scenario[] = scenarios,
): Record<string, StoredMessage[]> {
  const transcripts: Record<string, StoredMessage[]> = {}
  for (const scenario of cases) {
    const threadId = scenario.seedThreadId
    const row = seed.find((candidate) => candidate.threadId === threadId)
    if (!threadId || !row) continue
    const turn = scenario.turn({
      requestId: `req_seed_${threadId}`,
      threadId,
      runId: `run_seed_${threadId}`,
      approvalId: `apr_seed_${threadId}`,
      artifactId: `art_seed_${threadId}`,
      planMode: scenario.mode === 'do',
      model: SEED_MODEL,
      firstTurn: false,
    })
    transcripts[threadId] = [
      { id: `msg_seed_${threadId}_u`, role: 'user', content: scenario.suggestion.text, created_at: row.updatedAt },
      { id: `msg_seed_${threadId}_a`, role: 'assistant', content: turn.answer, created_at: row.updatedAt, model_used: SEED_MODEL },
    ]
  }
  return transcripts
}
