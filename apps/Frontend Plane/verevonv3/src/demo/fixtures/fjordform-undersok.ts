/**
 * Scene 03 · Undersøk: fixtures (PLAN_2026-10-11.md sections 2 and 4, replaced
 * by CONTRACT.md amendment F).
 *
 * Same fictional world as `fjordform.ts` (Fjordform AS, Ida Berg, Nordlys
 * Handel AS, order 4821). The story is a customer problem, not a fault in
 * Verevon: Fjordform's own invoice 20471 to Nordlys Handel AS was written at
 * list price although the framework agreement and the order confirmation say
 * 1 240,00 per piece. Verevon notices the deviation, compares agreement,
 * order and invoice, explains it with traceable sources and prepares a credit
 * note. A person approves before anything goes to the customer.
 *
 * The numbers add up: 120 pieces x 1 240,00 = 148 800,00 (agreement and
 * order); 120 x 1 380,00 = 165 600,00 (invoice); the difference is
 * 120 x 140,00 = 16 800,00 excluding VAT. Amounts use a no-break space as the
 * thousands separator so a number never wraps. Everything here is invented;
 * hostnames end in `.fjordform.example`. All visible strings are Norwegian
 * bokmål.
 */
import fakturaUrl from './assets/faktura-20471.png'

const NB = ' '

export const undersokThreadId = 'thr_faktura_20471'
export const undersokRequestId = 'req_faktura_20471'
/** The durable orchestration run the run panels and the approval belong to. */
export const undersokRunId = 'run_faktura_20471'

/**
 * The context-aware start: a fictional notice from invoice control. Verevon
 * turns it into one prompt suggestion that says what happened, where it came
 * from and when, and proposes the next action. Clicking it only FILLS the
 * composer with this text; the person sends it.
 */
export const undersokQuestion = 'Denne fakturaen ser ut til å avvike fra avtalen. Skal jeg undersøke hvorfor?'

export const undersokSuggestion = {
  label: 'Dette har skjedd',
  text: undersokQuestion,
  source: 'Regnskap · faktura 20471 til Nordlys Handel AS',
  time: 'i dag 09:12',
}

export const undersokTitle = 'Faktura 20471 · avvik fra avtalen'

/** One `subagent.research_fan_out` call: three parallel sub-tasks sharing one batch. */
export const undersokBatchId = 'batch_faktura_20471'
export const undersokSubtasks = [
  { childRunId: 'run_faktura_20471_avtale', goal: 'Les rammeavtale · pris pkt. 3.1', role: 'Avtalekontroll' },
  { childRunId: 'run_faktura_20471_ordre', goal: 'Sjekk ordre 4821 · pris og antall', role: 'Ordrekontroll' },
  { childRunId: 'run_faktura_20471_faktura', goal: 'Kontroller faktura 20471 · linjer', role: 'Fakturakontroll' },
]

/**
 * The fan-out tool the turn calls. The real thinking line renders a running
 * tool as «Kjører {navn}», with `_` read as a space and the first letter
 * capitalised, so this reads «Tenker · Kjører 3 deloppgaver».
 */
export const undersokToolName = '3_deloppgaver'

/**
 * The browser agent's one step: open the invoice in the accounting system.
 *
 * The events carry no URL on purpose. In the 435 px live rail the compact
 * BrowserChrome toolbar wraps and its address row sits under the page frame,
 * so a URL is never visible, while the step caption under the thumbnail is
 * `<action> · <host>` and would end in an ellipsis for any hostname. Title
 * and screenshot are what the frame shows.
 */
export const undersokBrowserStep = {
  actionId: 'act_faktura_20471',
  actionType: 'Åpne faktura',
  pageTitle: 'Faktura 20471',
  reason: 'Leser fakturalinjene i regnskapssystemet.',
  screenshotRef: 'art_faktura_20471',
}

/**
 * Where the BFF serves the step's screenshot (`runArtifactUrl` in
 * chat-run-watch.ts) and the fixture image the runtime maps it to.
 */
export const undersokImages: Record<string, string> = {
  [`/api/v1/browser/sessions/${undersokRunId}/artifacts/${undersokBrowserStep.screenshotRef}`]: fakturaUrl,
}

/** Assistant line while the three checks run. */
export const undersokAnswerChunks = [
  'Jeg sammenholder ',
  'avtalen, ordren og fakturaen.',
]

/** The finding block, streamed after the sub-tasks are done. */
export const undersokFindingChunks = [
  `\n\nFakturaen bruker listepris, 1${NB}380,00 per stk. `,
  `Avtalen og ordren sier 1${NB}240,00. `,
  `Avviket er 16${NB}800,00 kr eks. mva.`,
]

export const undersokArtifactId = 'art_forslag_kreditnota'
export const undersokArtifactTitle = 'Forslag · kreditnota'

/** Agreement, order and invoice side by side: the price per piece, with its source. */
export const undersokCompareRows = [
  { label: 'Avtale', value: `1${NB}240,00 per stk`, ref: 'Rammeavtale pkt. 3.1' },
  { label: 'Ordre 4821', value: `1${NB}240,00 per stk`, ref: 'Bekreftelse' },
  { label: 'Faktura 20471', value: `1${NB}380,00 per stk`, ref: 'Regnskap' },
]

export const undersokDeviation = { amount: `16${NB}800,00 kr`, detail: '120 stk × 140,00' }
export const undersokProposal = `Kreditnota på 16${NB}800,00 kr eks. mva.`

/**
 * The proposal document as markdown, revealed step by step through re-emitted
 * revisions (1 to 3 comparison rows, 4 adds the deviation, 5 adds the
 * proposal). A list, not a table: the real table renderer is 620 px wide
 * minimum and would scroll sideways inside the 440 px Resultat surface. The
 * reference is emphasis so the film can fade it in just after its line
 * (scenes/03-undersok.css). No H1: the viewer header already carries the title.
 */
export function undersokArtifactMarkdown(stage: 1 | 2 | 3 | 4 | 5): string {
  const parts = undersokCompareRows
    .slice(0, Math.min(stage, 3))
    .map((row) => `- **${row.label}:** ${row.value} *· ${row.ref}*`)
  if (stage >= 4) {
    parts.push('', '## Avvik', '', `- **${undersokDeviation.amount}** for mye fakturert *· ${undersokDeviation.detail}*`)
  }
  if (stage >= 5) {
    parts.push('', '## Forslag', '', `- ${undersokProposal}`)
  }
  return parts.join('\n')
}

/** The approval gate: the credit note only goes to the customer after a person says yes. */
export const undersokApproval = {
  id: 'apr_kreditnota_20471',
  run_id: undersokRunId,
  kind: 'Opprett kreditnota',
  status: 'APPROVAL_STATE_REQUESTED',
  // One line on the approval card at 1024 px.
  detail: `Kreditnota på 16${NB}800,00 kr. Ingenting sendes til kunden før du godkjenner.`,
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
