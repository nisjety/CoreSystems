/**
 * Case 2 · Reklamasjon fra Havnebrygga Kafé (Ask). A service case, not a film.
 *
 * A café reports a crack along the glue joint of a table top, on a table delivered
 * and signed for on 22 September. Verevon reads the warranty terms (five years against
 * construction and material faults), the delivery receipt (signed without remark) and
 * the production log, which records the same joint fault on 3 of the 40 tables in
 * series 2609, two of them at other customers. It prepares a replacement and a reply
 * in the inbox as a draft. Nothing is ordered or sent before a person says yes.
 *
 * Timeline, ms after the POST:
 *   10        connected (run id); 20 the title (first turn only)
 *   270–630   four citations, 120 ms apart: the sources open
 *   1010      the answer streams; step «Sjekker garantivilkår» done
 *   1410      step «Sjekker leveringen» done
 *   1700      step «Søker i produksjonsloggen» done
 *   1800      revision 1 of the claim: coverage, delivery and series
 *   1900      step «Skriver svar og bestilling», running (its id is the run id)
 *   2100      revision 2: the remedy, a new table top fitted on Thursday 15 October
 *   2400      revision 3: the reply to the café
 *   2600–3050 the second paragraph streams
 *   3300      the gate: the same step with status awaiting_approval (the card)
 *   3500      done; the chat channel stays open until a person decides
 *
 * After «Godkjenn» (ms after the click): the completion line streams from 260 ms, and
 * after it the artifact gains a final note that the reply is a draft and not sent. The
 * new table top is ordered for the fitter on Thursday 15 October; the run completes.
 * After «Avvis»: nothing is ordered, the reply is discarded (the artifact says so), and
 * the run is cancelled.
 */
import {
  reklamasjonAnswerChunks,
  reklamasjonApproval,
  reklamasjonArtifactMarkdown,
  reklamasjonArtifactTitle,
  reklamasjonDeliveryStep,
  reklamasjonFollowChunks,
  reklamasjonReplyStep,
  reklamasjonSeriesStep,
  reklamasjonSources,
  reklamasjonSuggestion,
  reklamasjonTitle,
  reklamasjonWarrantyStep,
} from '../../fixtures/fjordform-reklamasjon'
import { artifact, chunkTextOf, citation, connected, done, lastAt, paced, step, titleFrame } from '../frames'
import type { Gate, Scenario, StreamFrame, TurnContext, TurnScript } from '../scenario-types'

const SOURCE_GROUP = { id: 'kb:fjordform', label: 'Fjordform AS' }

const APPROVE_TEXT = '\n\n**Godkjent.** Ny bordplate til bord 6 er bestilt til montøren, som bytter den torsdag 15. oktober mellom 08 og 12. Svaret ligger som utkast i innboksen under Havnebrygga Kafé AS. Ingenting er sendt – du leser gjennom og sender selv.'
const REJECT_TEXT = '\n\n**Avvist.** Ingen bordplate er bestilt, og svarutkastet er forkastet. Ingenting er sendt til kunden.'
const FILED_NOTE = '*Svaret er et utkast i innboksen. Ikke sendt.*'
const DISCARDED_NOTE = '*Svaret er forkastet. Ikke sendt, og ingen bordplate er bestilt.*'

function reklamasjonTurn(ctx: TurnContext): TurnScript {
  const { requestId, runId, artifactId } = ctx
  // The same id and version grows in place while the turn streams (chat-artifacts.ts),
  // so the three stages are one draft; the decision then adds version 2 to its history.
  const revision = (at: number, stage: 1 | 2 | 3): StreamFrame => artifact(at, {
    id: artifactId,
    title: reklamasjonArtifactTitle,
    content: reklamasjonArtifactMarkdown(stage),
    version: 1,
  })

  const chat: StreamFrame[] = [
    connected(10, { requestId, threadId: ctx.threadId, model: ctx.model, runId }),
    ...(ctx.firstTurn ? [titleFrame(20, { requestId, title: reklamasjonTitle })] : []),
    ...reklamasjonSources.map((source, index) => citation(270 + index * 120, source, SOURCE_GROUP)),
    step(1_010, {
      id: reklamasjonWarrantyStep.id,
      title: reklamasjonWarrantyStep.title,
      detail: reklamasjonWarrantyStep.detail,
      status: 'done',
    }),
    ...paced(1_010, reklamasjonAnswerChunks.join(''), requestId),
    step(1_410, {
      id: reklamasjonDeliveryStep.id,
      title: reklamasjonDeliveryStep.title,
      detail: reklamasjonDeliveryStep.detail,
      status: 'done',
    }),
    step(1_700, {
      id: reklamasjonSeriesStep.id,
      title: reklamasjonSeriesStep.title,
      detail: reklamasjonSeriesStep.detail,
      status: 'done',
    }),
    revision(1_800, 1),
    step(1_900, { id: runId, title: reklamasjonReplyStep.title, detail: reklamasjonReplyStep.detail, status: 'running' }),
    revision(2_100, 2),
    revision(2_400, 3),
    ...paced(2_600, reklamasjonFollowChunks.join(''), requestId),
    step(3_300, {
      id: runId,
      title: reklamasjonReplyStep.title,
      detail: reklamasjonReplyStep.detail,
      status: 'awaiting_approval',
    }),
    done(3_500, { requestId, model: ctx.model }),
  ].sort((a, b) => a.at - b.at)

  const approveText = paced(260, APPROVE_TEXT, requestId)
  const rejectText = paced(260, REJECT_TEXT, requestId)
  const gate: Gate = {
    kind: reklamasjonApproval.kind,
    detail: reklamasjonApproval.detail,
    approve: {
      chat: [
        step(120, {
          id: runId,
          title: reklamasjonReplyStep.title,
          detail: 'Bordplaten er bestilt, og svaret er lagt i utkast.',
          status: 'done',
        }),
        ...approveText,
        artifact(lastAt(approveText) + 120, {
          id: artifactId,
          title: reklamasjonArtifactTitle,
          content: [reklamasjonArtifactMarkdown(3), '', FILED_NOTE].join('\n'),
          version: 2,
        }),
      ],
      run: [],
      text: APPROVE_TEXT,
      runOutcome: 'completed',
    },
    reject: {
      chat: [
        step(120, {
          id: runId,
          title: reklamasjonReplyStep.title,
          detail: 'Avvist. Ingen bordplate er bestilt, og svaret er forkastet.',
          status: 'done',
        }),
        ...rejectText,
        artifact(lastAt(rejectText) + 120, {
          id: artifactId,
          title: reklamasjonArtifactTitle,
          content: [reklamasjonArtifactMarkdown(3), '', DISCARDED_NOTE].join('\n'),
          version: 2,
        }),
      ],
      run: [],
      text: REJECT_TEXT,
      runOutcome: 'cancelled',
    },
  }

  return { chat, run: [], answer: chunkTextOf(chat), gate }
}

export const reklamasjonScenario: Scenario = {
  id: 'reklamasjon-bordplate',
  mode: 'ask',
  title: reklamasjonTitle,
  label: 'Reklamasjon fra Havnebrygga Kafé',
  suggestion: {
    text: reklamasjonSuggestion.text,
    source: reklamasjonSuggestion.source,
    time: reklamasjonSuggestion.time,
    icon: 'package',
  },
  // Free text answers a case only when it names it: the customer (a stem, so Havnebryggas counts) or
  // the production series. Generic pairs such as garanti + bordplate would hand this case to every
  // unrelated question about warranties or table tops.
  triggers: [['havnebrygga*'], ['serie', '2609']],
  turn: reklamasjonTurn,
}
