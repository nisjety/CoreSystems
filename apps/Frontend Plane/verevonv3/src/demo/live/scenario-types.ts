/**
 * Types of the live scenario engine (src/demo/live).
 *
 * A scenario is data plus one pure function: `turn(ctx)` returns the frames of
 * one chat turn (ms after the POST), the run events of a Do turn, the approval
 * gate and what each decision sends later. Frames use the product's real event
 * names and payload keys, as read by src/shared/api/chat-client.ts and
 * src/shared/api/run-console-client.ts.
 */

/** One frame of a `text/event-stream`. `at` is ms after its anchor (the POST, or a decision). */
export type StreamFrame = {
  at: number
  event: string
  data: Record<string, unknown>
  id?: string
}

/** The suggestion icons the start screen knows (suggestions.ts). */
export type ScenarioIcon = 'package' | 'receipt' | 'mail'

/** Ids and state of one turn, minted by the responder. A scenario builds its frames from them. */
export type TurnContext = {
  requestId: string
  threadId: string
  runId: string
  approvalId: string
  artifactId: string
  /** The request's `plan_mode` («Utfør»). Do turns stream run events; Ask turns use the chat stream only. */
  planMode: boolean
  /** The model id the answer reports (a fixture id from fixtures/fjordform.ts). */
  model: string
  /** True on the thread's first turn: only that turn sends the `title` frame. */
  firstTurn: boolean
}

/** What a decision («Godkjenn» or «Avvis») sends after the gate, relative to the click. */
export type Decision = {
  /** Chat-stream frames, `at` ms after the decision. */
  chat: StreamFrame[]
  /** Run-stream frames, `at` ms after the decision. */
  run: StreamFrame[]
  /** Markdown the stored assistant message gains, so a reopened thread shows the outcome. */
  text: string
  /** The run's state once decided. */
  runOutcome: 'completed' | 'cancelled'
}

/** An approval gate: the turn pauses until a person decides. Nothing happens before that. */
export type Gate = {
  /** The approval card's title («Godkjenning · {kind}»). */
  kind: string
  /** The approval card's one-line detail. */
  detail: string
  approve: Decision
  reject: Decision
}

/** One turn as the engine plays it. */
export type TurnScript = {
  /** Chat-stream frames, `at` ms after the POST, in order. The turn's `done` is among them. */
  chat: StreamFrame[]
  /** Run-stream frames (Do turns), `at` ms after the POST. */
  run: StreamFrame[]
  /** The assistant text the stored message keeps: the concatenated answer chunks. */
  answer: string
  /** Present when the turn pauses for a person. */
  gate?: Gate
}

export type Scenario = {
  id: string
  /** Ask answers on the chat stream. Do also streams run events and pauses the run at its gate. */
  mode: 'ask' | 'do'
  /** The thread title, sent on the thread's first turn. */
  title: string
  /** Short label for the fallback's links («Tilbud til Solhaug Hotell»). */
  label: string
  /** The start-screen card: what happened, where it came from, when, and the question it fills. */
  suggestion: { text: string; source: string; time: string; icon: ScenarioIcon }
  /**
   * The id the dashboard's seeded conversation for this case uses, if any. When
   * the seed lists that id, its transcript is the question and the case's answer.
   */
  seedThreadId?: string
  /**
   * Matched against a free-text message (match.ts). Each group is an all-of list
   * of terms, and a message scores one point per group whose every term it has.
   * A term is a whole word, or a stem when it ends with `*`. A group is a case
   * identifier on its own (an order or series number, or the customer's name), or at
   * least two words; a group of generic words needs three. Free text answers a case
   * only when it names it (match.test.ts).
   */
  triggers: string[][]
  turn: (ctx: TurnContext) => TurnScript
}
