/**
 * Case 3 · Dokumentasjon til Nordhavn kommune (Ask). A tender case, not a film.
 *
 * A municipality asks for environmental documents (FSC and EPD) with its tender,
 * by Friday. Verevon reads the requirement, checks the validity of each document
 * in the knowledge base and finds the chair EPD that ran out on 1 September. It
 * then drafts the reply to the municipality and a task for purchasing, to renew
 * that EPD. Nothing is saved or created before a person says yes.
 *
 * Timeline, ms after the POST:
 *   10          connected (run id); 20 the title (first turn only)
 *   270–630     four citations, 120 ms apart: the sources open
 *   1010–1685   the answer streams; step «Leser kravene i anbudet» done at 1010
 *   1410        step «Sjekker gyldighet» done
 *   1500        revision 1 of the document (version 1): each document's validity
 *   1800        step «Skriver svar og oppgave», running (its id is the run id)
 *   2000        revision 2 (version 2): the actions
 *   2400        revision 3 (version 3): the reply to the municipality
 *   2500–3220   the second paragraph streams
 *   3300        the gate: the same step with status awaiting_approval (the card)
 *   3500        done; the chat channel stays open until a person decides
 *
 * Approve, ms after the click: 120 step done; 260–890 the completion line, which
 * starts with a paragraph break; 1000 revision 4 (version 4): the document gains a
 * final note. Reject, ms after the click: 120 step done with the reject detail; 260–620
 * the line; 740 revision 4 (version 4): the document gains a note that the reply is
 * discarded. Nothing is saved or created on reject.
 *
 * Each revision has its own version number: the product keeps the history by
 * version, so a revision sent again under the same number replaces the one before.
 */
import {
  anbudAnswerChunks,
  anbudApproval,
  anbudArtifactMarkdown,
  anbudArtifactTitle,
  anbudFollowChunks,
  anbudReplyStep,
  anbudRequirementStep,
  anbudSources,
  anbudSuggestion,
  anbudTitle,
  anbudValidityStep,
} from '../../fixtures/fjordform-anbud'
import { artifact, chunkTextOf, citation, connected, done, paced, step, titleFrame } from '../frames'
import type { Gate, Scenario, StreamFrame, TurnContext, TurnScript } from '../scenario-types'

const SOURCE_GROUP = { id: 'kb:fjordform', label: 'Fjordform AS' }

const APPROVE_TEXT = '\n\n**Godkjent.** Svaret er lagt som utkast til Nordhavn kommune. Oppgaven «Forny EPD for FS-310» er opprettet for innkjøp. Ingenting er sendt – du leser gjennom og sender selv.'
const REJECT_TEXT = '\n\n**Avvist.** Utkastet er forkastet. Ingenting er lagret, og ingen oppgave er opprettet. Ingenting er sendt til kommunen.'
const FILED_NOTE = '*Svar lagt som utkast til Nordhavn kommune, og oppgave opprettet for innkjøp. Ikke sendt.*'
const DISCARDED_NOTE = '*Svaret er forkastet. Ingenting er lagret eller sendt.*'

const APPROVED_DETAIL = 'Svaret er lagt i utkast, og oppgaven er opprettet.'
const REJECTED_DETAIL = 'Avvist. Ingenting er lagret, og ingen oppgave er opprettet.'

function anbudTurn(ctx: TurnContext): TurnScript {
  const { requestId, runId, artifactId } = ctx
  const revision = (at: number, stage: 1 | 2 | 3): StreamFrame => artifact(at, {
    id: artifactId,
    title: anbudArtifactTitle,
    content: anbudArtifactMarkdown(stage),
    version: stage,
  })

  const chat: StreamFrame[] = [
    connected(10, { requestId, threadId: ctx.threadId, model: ctx.model, runId }),
    ...(ctx.firstTurn ? [titleFrame(20, { requestId, title: anbudTitle })] : []),
    ...anbudSources.map((source, index) => citation(270 + index * 120, source, SOURCE_GROUP)),
    ...paced(1_010, anbudAnswerChunks.join(''), requestId),
    step(1_010, { id: anbudRequirementStep.id, title: anbudRequirementStep.title, detail: anbudRequirementStep.detail, status: 'done' }),
    step(1_410, { id: anbudValidityStep.id, title: anbudValidityStep.title, detail: anbudValidityStep.detail, status: 'done' }),
    revision(1_500, 1),
    step(1_800, { id: runId, title: anbudReplyStep.title, detail: anbudReplyStep.detail, status: 'running' }),
    revision(2_000, 2),
    revision(2_400, 3),
    ...paced(2_500, anbudFollowChunks.join(''), requestId),
    step(3_300, { id: runId, title: anbudReplyStep.title, detail: anbudReplyStep.detail, status: 'awaiting_approval' }),
    done(3_500, { requestId, model: ctx.model }),
  ].sort((a, b) => a.at - b.at)

  const gate: Gate = {
    kind: anbudApproval.kind,
    detail: anbudApproval.detail,
    approve: {
      chat: [
        step(120, { id: runId, title: anbudReplyStep.title, detail: APPROVED_DETAIL, status: 'done' }),
        ...paced(260, APPROVE_TEXT, requestId),
        artifact(1_000, {
          id: artifactId,
          title: anbudArtifactTitle,
          content: [anbudArtifactMarkdown(3), '', FILED_NOTE].join('\n'),
          version: 4,
        }),
      ],
      run: [],
      text: APPROVE_TEXT,
      runOutcome: 'completed',
    },
    reject: {
      chat: [
        step(120, { id: runId, title: anbudReplyStep.title, detail: REJECTED_DETAIL, status: 'done' }),
        ...paced(260, REJECT_TEXT, requestId),
        artifact(740, {
          id: artifactId,
          title: anbudArtifactTitle,
          content: [anbudArtifactMarkdown(3), '', DISCARDED_NOTE].join('\n'),
          version: 4,
        }),
      ],
      run: [],
      text: REJECT_TEXT,
      runOutcome: 'cancelled',
    },
  }

  return { chat, run: [], answer: chunkTextOf(chat), gate }
}

export const anbudScenario: Scenario = {
  id: 'anbud-miljodokumentasjon',
  mode: 'ask',
  title: anbudTitle,
  label: 'Dokumentasjon til Nordhavn kommune',
  suggestion: {
    text: anbudSuggestion.text,
    source: anbudSuggestion.source,
    time: anbudSuggestion.time,
    icon: 'receipt',
  },
  // Free text answers a case only when it names it: the customer, as a stem so that Nordhavns counts
  // too. The words FSC, EPD and anbud do not name it: a question about them for another customer would
  // otherwise get this tender's answer, whose approval names Nordhavn kommune.
  triggers: [['nordhavn*']],
  turn: anbudTurn,
}
