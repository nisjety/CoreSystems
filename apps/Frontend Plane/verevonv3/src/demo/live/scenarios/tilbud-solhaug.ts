/**
 * Case 1 · Tilbud til Solhaug Hotell (Do). A sales case, not a film: a hotel
 * asks Fjordform AS for a quote on 40 chairs and 10 tables before its opening.
 *
 * Verevon checks the stock, the price list and the assembly calendar in three
 * parallel sub-tasks (a Do turn), builds the priced quote and a reply, and waits
 * for a person to approve. Nothing is reserved or sent before that. The case has
 * no browser step and no screenshot.
 *
 * With «Utfør» (plan mode) the sub-tasks and the pause travel on the run channel,
 * as the product's live run panel expects. Without it the same work shows as
 * steps on the chat stream, and the gate is a step, as in Ask turns.
 *
 * Timeline, ms after the POST:
 *   10          connected (run id); the title (first turn); with «Utfør» the tool call
 *   10          three sub-tasks attach in one batch (run channel); without «Utfør» they run as steps
 *   270–630     four citations, 120 ms apart: the sources open
 *   1010        with «Utfør» the tool result; the answer streams
 *   1510–2110   the sub-tasks finish, 300 ms apart
 *   2460        the second paragraph streams: the quote is in place
 *   2510–3010   the quote is revealed in three revisions, 250 ms apart
 *   3560        without «Utfør» only: the gate step (its id is the run id)
 *   3610        done
 *   3760        with «Utfør» only: run_paused_for_approval on the run channel
 *
 * A decision («Godkjenn» or «Avvis») sends its step 120 ms and its text 260 ms
 * after the click. «Godkjenn» adds the quote as version 4, the revision after the
 * three stages (1 to 3), and resumes the run. A version sent again replaces that
 * revision, so every revision of the quote carries a version of its own.
 */
import {
  tilbudAnswerChunks,
  tilbudApproval,
  tilbudArtifactMarkdown,
  tilbudArtifactTitle,
  tilbudFollowChunks,
  tilbudQuoteStep,
  tilbudReference,
  tilbudSources,
  tilbudSubtasks,
  tilbudSuggestion,
  tilbudTitle,
} from '../../fixtures/fjordform-tilbud'
import {
  approvalChanged,
  artifact,
  chunkTextOf,
  citation,
  connected,
  done,
  paced,
  runPaused,
  runResumed,
  step,
  subagentAttached,
  subagentStopped,
  titleFrame,
  toolCall,
  toolResult,
} from '../frames'
import type { Decision, Gate, Scenario, StreamFrame, TurnContext, TurnScript } from '../scenario-types'

const SOURCE_GROUP = { id: 'kb:fjordform', label: 'Fjordform AS' }
/** The protocol name of the tool that starts the sub-tasks (the films use the same name). */
const TOOL_NAME = '3_deloppgaver'
/** The step a decision adds in a Do turn, which has no gate step of its own on the chat stream. */
const STEP_ID_QUOTE = 'steg-tilbud'
/** The quote's approved revision: the fourth, after the three stages (versions 1 to 3). */
const VERSION_APPROVED = 4

/** The run panel's name for each sub-task. The fixture gives each task and its finding, not a name. */
const ROLES: Record<string, string> = {
  'sub-lager': 'Lagerkontroll',
  'sub-pris': 'Prisberegning',
  'sub-montering': 'Monteringsplan',
}

const APPROVE_TEXT = `\n\n**Godkjent.** Tilbud ${tilbudReference} er lagt som utkast til Solhaug Hotell, og 28 stoler og 10 bord er reservert. Ingenting er sendt – du gjennomgår utkastet og sender det selv.`
const REJECT_TEXT = `\n\n**Avvist.** Tilbud ${tilbudReference} er ikke lagt ut. Ingenting er reservert, og ingenting er sendt til Solhaug Hotell.`

/** The rows the quote gains under «Utført» once it is approved. */
const DONE_ROWS = [
  `- [x] Tilbud ${tilbudReference} lagt som utkast`,
  '- [x] 28 stoler og 10 bord reservert',
  '- [x] Ikke sendt til Solhaug Hotell AS',
]

function roleOf(id: string): string {
  return ROLES[id] ?? 'Delagent'
}

/** Sub-task number `index` (0-based) finishes 300 ms after the one before it, from 1 510 ms. */
function finishAt(index: number): number {
  return 1_510 + index * 300
}

function tilbudTurn(ctx: TurnContext): TurnScript {
  const { requestId, runId, artifactId, planMode } = ctx
  const batchId = `${runId}_batch`
  const toolId = `call_${requestId}`
  const subtasks = tilbudSubtasks.map((task) => ({
    id: task.id,
    childRunId: `${runId}_${task.id}`,
    role: roleOf(task.id),
    goal: task.title,
    result: task.result,
  }))

  // Each stage is a revision of one document: a version sent again replaces that
  // revision, so the stages carry versions 1 to 3 and the approval version 4.
  const revision = (at: number, stage: 1 | 2 | 3): StreamFrame => artifact(at, {
    id: artifactId,
    title: tilbudArtifactTitle,
    content: tilbudArtifactMarkdown(stage),
    version: stage,
  })

  // Ask turns have no run panel: the sub-tasks are steps on the chat stream.
  const askSteps: StreamFrame[] = planMode
    ? []
    : subtasks.flatMap((task, index) => [
        step(10, { id: task.childRunId, title: task.role, detail: task.goal, status: 'running' }),
        step(finishAt(index), { id: task.childRunId, title: task.role, detail: task.result, status: 'done' }),
      ])

  const chat: StreamFrame[] = [
    connected(10, { requestId, threadId: ctx.threadId, model: ctx.model, runId }),
    ...(ctx.firstTurn ? [titleFrame(10, { requestId, title: tilbudTitle })] : []),
    ...tilbudSources.map((source, index) => citation(270 + index * 120, source, SOURCE_GROUP)),
    ...(planMode
      ? [
          toolCall(10, { id: toolId, name: TOOL_NAME, args: { deloppgaver: subtasks.length } }),
          toolResult(1_010, { id: toolId, output: `${subtasks.length} deloppgaver startet` }),
        ]
      : []),
    ...askSteps,
    ...paced(1_010, tilbudAnswerChunks.join(''), requestId),
    ...paced(2_460, tilbudFollowChunks.join(''), requestId),
    revision(2_510, 1),
    revision(2_760, 2),
    revision(3_010, 3),
    ...(planMode
      ? []
      : [step(3_560, { id: runId, title: tilbudQuoteStep.title, detail: tilbudQuoteStep.detail, status: 'awaiting_approval' })]),
    done(3_610, { requestId, model: ctx.model }),
  ].sort((a, b) => a.at - b.at)

  const run: StreamFrame[] = planMode
    ? [
        ...subtasks.map((task) => subagentAttached(10, {
          runId,
          childRunId: task.childRunId,
          role: task.role,
          goal: task.goal,
          batchId,
        })),
        ...subtasks.map((task, index) => subagentStopped(finishAt(index), { runId, childRunId: task.childRunId })),
        runPaused(3_760, { runId, approvalId: ctx.approvalId }),
      ]
    : []

  const decide = (approved: boolean): Decision => {
    const text = approved ? APPROVE_TEXT : REJECT_TEXT
    const stepId = planMode ? STEP_ID_QUOTE : runId
    return {
      chat: [
        approved
          ? step(120, {
              id: stepId,
              title: 'Tilbud lagt som utkast',
              detail: `${tilbudReference} · 28 stoler og 10 bord reservert, ikke sendt`,
              status: 'done',
            })
          : step(120, { id: stepId, title: 'Tilbud ikke lagt ut', detail: 'Avvist. Ingenting er reservert eller sendt.', status: 'done' }),
        ...paced(260, text, requestId),
        ...(approved
          ? [artifact(900, {
              id: artifactId,
              title: tilbudArtifactTitle,
              content: [tilbudArtifactMarkdown(3), '', '## Utført', '', ...DONE_ROWS].join('\n'),
              version: VERSION_APPROVED,
            })]
          : []),
      ],
      run: planMode
        ? [
            approvalChanged(60, {
              approvalId: ctx.approvalId,
              runId,
              kind: tilbudApproval.kind,
              to: approved ? 'APPROVAL_STATE_GRANTED' : 'APPROVAL_STATE_DENIED',
              decidedBy: 'Ida Berg',
            }),
            ...(approved ? [runResumed(60, { runId, approvalId: ctx.approvalId })] : []),
          ]
        : [],
      text,
      runOutcome: approved ? 'completed' : 'cancelled',
    }
  }

  const gate: Gate = {
    kind: tilbudApproval.kind,
    detail: tilbudApproval.detail,
    approve: decide(true),
    reject: decide(false),
  }

  return { chat, run, answer: chunkTextOf(chat), gate }
}

export const tilbudScenario: Scenario = {
  id: 'tilbud-solhaug',
  mode: 'do',
  title: tilbudTitle,
  label: 'Tilbud til Solhaug Hotell',
  suggestion: {
    text: tilbudSuggestion.text,
    source: tilbudSuggestion.source,
    time: tilbudSuggestion.time,
    icon: 'mail',
  },
  // Free text answers a case only when it names it: the customer, as a stem so that Solhaugs and
  // Solhaughotellet count too. Generic word pairs such as tilbud + bord would hand a canned quote
  // to every unrelated question about quotes or tables.
  triggers: [['solhaug*']],
  turn: tilbudTurn,
}
