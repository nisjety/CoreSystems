/**
 * Scene 03 · Undersøk: fixtures (PLAN_2026-10-11.md sections 2 and 4).
 *
 * Same fictional world as `fjordform.ts` (Fjordform AS, Ida Berg, order 4821
 * to Visma.net). Everything here is invented: the export run, the test
 * environment's log, the schema and the mapping. Hostnames end in
 * `.fjordform.example`. All visible strings are Norwegian bokmål; the one
 * English sentence is the validator's own technical error text, quoted from
 * the log as the plan specifies.
 */
import eksportloggUrl from './assets/eksportlogg-kjoring-184.png'

export const undersokThreadId = 'thr_eksport_4821'
export const undersokRequestId = 'req_eksport_4821'
/** The durable orchestration run the Work canvas, the run panels and the approval belong to. */
export const undersokRunId = 'run_eksport_4821'

export const undersokQuestion = 'Eksporten av ordre 4821 til Visma feiler. Finn årsaken og foreslå en trygg fiks.'

export const undersokTitle = 'Eksport av ordre 4821 · årsak og fiks'

/** One `subagent.research_fan_out` call: three parallel sub-tasks sharing one batch. */
export const undersokBatchId = 'batch_eksport_4821'
export const undersokSubtasks = [
  { childRunId: 'run_eksport_4821_logg', goal: 'Les eksportlogg · kjøring 184', role: 'Logganalyse' },
  { childRunId: 'run_eksport_4821_skjema', goal: 'Sammenlign skjema v3', role: 'Skjemakontroll' },
  { childRunId: 'run_eksport_4821_mapping', goal: 'Sjekk mapping · kunderegister', role: 'Kodegjennomgang' },
]

/**
 * The fan-out tool the turn calls. The real thinking line renders a running
 * tool as «Kjører {navn}», with `_` read as a space and the first letter
 * capitalised, so this reads «Tenker · Kjører 3 deloppgaver».
 */
export const undersokToolName = '3_deloppgaver'

/**
 * The browser agent's one step: open the test environment's export log.
 *
 * The events carry no URL on purpose. In the 435 px live rail the compact
 * BrowserChrome toolbar wraps and its address row sits under the page frame,
 * so a URL is never visible, while the step caption under the thumbnail is
 * `<action> · <host>` and would end in an ellipsis for any hostname. Title
 * and screenshot are what the frame shows.
 */
export const undersokBrowserStep = {
  actionId: 'act_eksportlogg_184',
  actionType: 'Åpne logg',
  pageTitle: 'Eksportlogg',
  reason: 'Leser loggen for kjøring 184 i testmiljøet.',
  screenshotRef: 'art_eksportlogg_184',
}

/** The log line the evidence frame highlights (PLAN section 4). */
export const undersokLogLine = '09:41:03 · Validering feilet: required field customer_id is missing.'

/**
 * Where the BFF serves the step's screenshot (`runArtifactUrl` in
 * chat-run-watch.ts) and the fixture image the runtime maps it to.
 */
export const undersokImages: Record<string, string> = {
  [`/api/v1/browser/sessions/${undersokRunId}/artifacts/${undersokBrowserStep.screenshotRef}`]: eksportloggUrl,
}

/** Assistant line once the evidence is in. */
export const undersokAnswerChunks = [
  'Loggen og skjemaet ',
  'peker på samme årsak.',
]

/** The finding block, streamed after the sub-tasks are done. */
export const undersokFindingChunks = [
  '\n\n`customer_id` mangler i mappingen. ',
  'Loggen, skjemaet og koden bekrefter det.',
]

export const undersokArtifactId = 'art_forslag_mapping'
export const undersokArtifactTitle = 'Forslag · mapping-endring'

/** The proposed change (PLAN section 4), one line added. */
export const undersokDiffLines = [
  '  name: "name",',
  '  email: "email",',
  '+ customer_id: "id",',
]

export const undersokSimulationNote = 'Simulert test · ingen produksjonsdata endres'

/** The proposal document, revealed line by line through re-emitted revisions. */
export function undersokArtifactMarkdown(lineCount: number, withNote: boolean): string {
  // No H1: the viewer header already carries the title.
  const parts = [
    'Kunderegister → Visma.net · skjema v3',
    '',
    '```diff',
    ...undersokDiffLines.slice(0, lineCount),
    '```',
  ]
  // PLAN section 4 text exactly, no trailing period.
  if (withNote) parts.push('', undersokSimulationNote)
  return parts.join('\n')
}

/** The approval gate: the fix is only tried, in a simulation, after a person says yes. */
export const undersokApproval = {
  id: 'apr_simulert_test_4821',
  run_id: undersokRunId,
  kind: 'Kjør simulert test',
  status: 'APPROVAL_STATE_REQUESTED',
  // One line on the approval card at 1024 px (the note itself is in the proposal).
  detail: 'Mappingen prøves mot testmiljøet. Ingen produksjonsdata endres.',
}

/** `GET /api/v1/agents/runs/:id`: the run is live (paused at the gate, not finished). */
export const undersokRunDetail = {
  run: {
    run_id: undersokRunId,
    thread_id: undersokThreadId,
    status: 'running',
    mode: 'plan',
    goal: undersokQuestion,
    checkpoint_index: 0,
    steps_completed: 2,
    input_tokens: 0,
    output_tokens: 0,
  },
}
