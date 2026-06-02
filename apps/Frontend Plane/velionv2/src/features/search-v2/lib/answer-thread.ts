/**
 * answer-thread.ts — pure helpers for the search answer-engine follow-up thread.
 *
 * The follow-up composer in SearchAnswerView turns a single search answer into a
 * conversational thread. Each follow-up is sent to the chat stream
 * (`streamChat`) with a compact, source-grounded context block so the model
 * answers in the context of the original search rather than starting cold.
 *
 * These helpers are framework-free and unit-testable.
 */

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** A single turn in the follow-up conversation thread (turn 0 = initial search). */
export type ThreadTurn = {
  id: string;
  role: "user" | "assistant";
  /** Rendered text. For a streaming assistant turn this grows token-by-token. */
  text: string;
  /** Assistant turns only: true while tokens are still arriving. */
  streaming?: boolean;
  /** Assistant turns only: a terminal error message, if the stream failed. */
  error?: string | null;
};

/** Minimal shape of the source links used to ground a follow-up. */
export type GroundingSource = {
  url: string;
  title?: string;
};

/** Everything the grounding block needs from the current answer + thread. */
export type GroundingInput = {
  /** The original search query (turn 0). */
  query: string;
  /** The AI summary text for the original query (may be empty). */
  answer: string;
  /** Top source links from the original answer (citations and/or results). */
  sources: GroundingSource[];
  /** Prior follow-up turns (excludes turn 0), oldest-first. */
  priorTurns: ThreadTurn[];
  /** The new follow-up question being asked. */
  question: string;
};

// ---------------------------------------------------------------------------
// Bounds — keep the grounding block from blowing the prompt budget.
// ---------------------------------------------------------------------------

const MAX_ANSWER_CHARS = 1200;
const MAX_SOURCES = 6;
const MAX_TITLE_CHARS = 120;
const MAX_PRIOR_TURNS = 8; // last 4 Q/A pairs
const MAX_PRIOR_TURN_CHARS = 600;

/** Collapse whitespace and hard-truncate with an ellipsis. */
function truncate(value: string, max: number): string {
  const collapsed = value.replace(/\s+/g, " ").trim();
  if (collapsed.length <= max) return collapsed;
  return `${collapsed.slice(0, max - 1).trimEnd()}…`;
}

/**
 * De-duplicate sources by URL (preserving order) and cap the count, so the
 * grounding block stays compact even when citations and results overlap.
 */
export function dedupeSources(sources: GroundingSource[]): GroundingSource[] {
  const seen = new Set<string>();
  const out: GroundingSource[] = [];
  for (const source of sources) {
    const url = source.url?.trim();
    if (!url || seen.has(url)) continue;
    seen.add(url);
    out.push(source);
    if (out.length >= MAX_SOURCES) break;
  }
  return out;
}

/**
 * Build the bounded, Norwegian-language grounding `content` string sent to
 * `streamChat`. Shape:
 *
 *   Du svarer på oppfølgingsspørsmål om et søk.
 *
 *   Søk: <query>
 *   Sammendrag: <answer>
 *   Kilder:
 *   - <title> <url>
 *   ...
 *
 *   Tidligere:
 *   Spørsmål: <q>
 *   Svar: <a>
 *   ...
 *
 *   Oppfølging: <question>
 */
export function buildGroundingContent(input: GroundingInput): string {
  const lines: string[] = [
    "Du svarer på oppfølgingsspørsmål om et søk. Bruk konteksten under og svar kort og presist på norsk.",
    "",
    `Søk: ${truncate(input.query, MAX_TITLE_CHARS)}`,
  ];

  const answer = truncate(input.answer ?? "", MAX_ANSWER_CHARS);
  if (answer) {
    lines.push(`Sammendrag: ${answer}`);
  }

  const sources = dedupeSources(input.sources ?? []);
  if (sources.length > 0) {
    lines.push("Kilder:");
    for (const source of sources) {
      const title = truncate(source.title ?? "", MAX_TITLE_CHARS);
      lines.push(title ? `- ${title} ${source.url}` : `- ${source.url}`);
    }
  }

  const prior = (input.priorTurns ?? []).slice(-MAX_PRIOR_TURNS);
  if (prior.length > 0) {
    lines.push("", "Tidligere:");
    for (const turn of prior) {
      const text = truncate(turn.text ?? "", MAX_PRIOR_TURN_CHARS);
      if (!text) continue;
      lines.push(`${turn.role === "user" ? "Spørsmål" : "Svar"}: ${text}`);
    }
  }

  lines.push("", `Oppfølging: ${truncate(input.question, MAX_PRIOR_TURN_CHARS)}`);

  return lines.join("\n");
}
