/**
 * Unified-diff production and parsing.
 *
 * # Why this exists
 *
 * The tool-presentation intents borrowed from the reference set include `diff`,
 * and the first pass left it out on the grounds that **nothing in this platform
 * produces a patch** — no file-editing tool, and no tool result carrying
 * before/after state. Shipping a card nothing can populate is the
 * dead-but-visible pattern the adoption plan rejects.
 *
 * Two real producers do exist, and this module serves both:
 *
 * 1. **Artifact revisions.** `ChatArtifact.history` already retains every
 *    version's full content client-side, so "what changed when I asked for a
 *    revision" is computable with no backend work at all — and it is the
 *    question a reader most often has about a rewritten document.
 * 2. **Tool output that already IS a patch.** Third-party MCP tools (a git
 *    server, a codemod runner) return unified diffs today. Detecting that from
 *    the OUTPUT rather than from a tool NAME is what makes the intent
 *    recomputed rather than a hardcoded list — and it means a tool we have never
 *    heard of gets the right card.
 *
 * The diff itself is computed here rather than pulled in as a dependency: the
 * only thing needed is a line-level LCS, the inputs are one document, and a
 * dependency for ~60 lines of well-understood algorithm is not worth the supply
 * chain.
 */

export type DiffLineKind = 'context' | 'added' | 'removed'

export type DiffLine = {
  kind: DiffLineKind
  text: string
}

export type DiffHunk = {
  /** 1-based start line in the OLD text, and how many lines it covers. */
  oldStart: number
  oldLines: number
  /** 1-based start line in the NEW text, and how many lines it covers. */
  newStart: number
  newLines: number
  lines: DiffLine[]
}

export type DiffStat = {
  added: number
  removed: number
}

/** Lines of unchanged context kept either side of a change. */
const CONTEXT_LINES = 3

/**
 * Upper bound on lines compared. Above it the LCS table dominates the main
 * thread — this runs during render, and a 5,000-line artifact revision would
 * otherwise freeze the tab. Past the cap the caller gets a stat-only summary,
 * which is honest and still useful.
 */
export const MAX_DIFF_LINES = 2_000

function splitLines(text: string): string[] {
  if (text === '') return []
  // Trailing newline must not create a phantom empty last line, which would show
  // as a spurious change whenever only one side has one.
  return text.replace(/\n$/, '').split('\n')
}

/**
 * Longest-common-subsequence table over lines, walked back into an edit script.
 *
 * Returns `null` when either side exceeds [`MAX_DIFF_LINES`], so the caller can
 * fall back rather than block the render.
 */
function editScript(
  before: string[],
  after: string[],
): Array<{ kind: DiffLineKind; text: string }> | null {
  if (before.length > MAX_DIFF_LINES || after.length > MAX_DIFF_LINES) return null

  const rows = before.length + 1
  const cols = after.length + 1
  // Flat array: a nested one allocates `rows` sub-arrays for no benefit.
  const lcs = new Uint32Array(rows * cols)
  for (let i = before.length - 1; i >= 0; i -= 1) {
    for (let j = after.length - 1; j >= 0; j -= 1) {
      // `noUncheckedIndexedAccess` types Uint32Array reads as possibly
      // undefined; every index here is in range by construction, and `?? 0`
      // keeps that provable to the compiler without an assertion.
      const skipBoth = lcs[(i + 1) * cols + (j + 1)] ?? 0
      const skipBefore = lcs[(i + 1) * cols + j] ?? 0
      const skipAfter = lcs[i * cols + (j + 1)] ?? 0
      lcs[i * cols + j] =
        before[i] === after[j] ? skipBoth + 1 : Math.max(skipBefore, skipAfter)
    }
  }

  const script: Array<{ kind: DiffLineKind; text: string }> = []
  let i = 0
  let j = 0
  while (i < before.length && j < after.length) {
    if (before[i] === after[j]) {
      script.push({ kind: 'context', text: before[i]! })
      i += 1
      j += 1
    } else if ((lcs[(i + 1) * cols + j] ?? 0) >= (lcs[i * cols + (j + 1)] ?? 0)) {
      script.push({ kind: 'removed', text: before[i]! })
      i += 1
    } else {
      script.push({ kind: 'added', text: after[j]! })
      j += 1
    }
  }
  while (i < before.length) {
    script.push({ kind: 'removed', text: before[i]! })
    i += 1
  }
  while (j < after.length) {
    script.push({ kind: 'added', text: after[j]! })
    j += 1
  }
  return script
}

export type DiffResult = {
  hunks: DiffHunk[]
  stat: DiffStat
  /**
   * True when the inputs were too large to diff line-by-line. `stat` is then a
   * line-count delta rather than a real add/remove count, and `hunks` is empty.
   * Surfaced so a card can say so instead of implying "no changes".
   */
  truncated: boolean
}

/** Compute the hunks between two texts. */
export function diffText(before: string, after: string): DiffResult {
  const beforeLines = splitLines(before)
  const afterLines = splitLines(after)
  const script = editScript(beforeLines, afterLines)

  if (!script) {
    // Honest degradation: report the size change, do not pretend to a diff.
    const delta = afterLines.length - beforeLines.length
    return {
      hunks: [],
      stat: { added: Math.max(delta, 0), removed: Math.max(-delta, 0) },
      truncated: true,
    }
  }

  const stat: DiffStat = { added: 0, removed: 0 }
  for (const entry of script) {
    if (entry.kind === 'added') stat.added += 1
    if (entry.kind === 'removed') stat.removed += 1
  }

  // Group changes into hunks with bounded context, so an unchanged document
  // does not render thousands of context lines.
  const hunks: DiffHunk[] = []
  let oldLine = 1
  let newLine = 1
  let current: DiffHunk | null = null
  let trailingContext = 0
  const pending: DiffLine[] = []

  const flush = () => {
    if (current) hunks.push(current)
    current = null
    trailingContext = 0
  }

  for (const entry of script) {
    if (entry.kind === 'context') {
      if (current) {
        if (trailingContext < CONTEXT_LINES) {
          current.lines.push(entry)
          current.oldLines += 1
          current.newLines += 1
          trailingContext += 1
        } else {
          flush()
        }
      }
      if (!current) {
        pending.push(entry)
        if (pending.length > CONTEXT_LINES) pending.shift()
      }
      oldLine += 1
      newLine += 1
      continue
    }

    if (!current) {
      // Open a hunk with up to CONTEXT_LINES of the context we just passed.
      const lead = pending.slice()
      pending.length = 0
      current = {
        oldStart: oldLine - lead.length,
        oldLines: lead.length,
        newStart: newLine - lead.length,
        newLines: lead.length,
        lines: lead,
      }
    }
    trailingContext = 0
    current.lines.push(entry)
    if (entry.kind === 'removed') {
      current.oldLines += 1
      oldLine += 1
    } else {
      current.newLines += 1
      newLine += 1
    }
  }
  flush()

  return { hunks, stat, truncated: false }
}

/**
 * Whether `text` already looks like a unified diff.
 *
 * Used to give a `diff` intent to tool output that IS a patch — an MCP git
 * server, a codemod runner — without knowing the tool's name. Deliberately
 * strict: it requires the `@@ -a,b +c,d @@` hunk header, because `+`/`-` line
 * prefixes alone match ordinary prose, lists and log output, and a wrong diff
 * card is more confusing than a generic one.
 */
export function looksLikeUnifiedDiff(text: string): boolean {
  if (!text) return false
  return /^@@ -\d+(?:,\d+)? \+\d+(?:,\d+)? @@/m.test(text)
}

/**
 * Parse an already-unified diff into hunks, so a tool's own patch renders with
 * the same card as a computed one.
 *
 * Returns an empty hunk list for anything without a recognisable hunk header —
 * the caller should have gated on [`looksLikeUnifiedDiff`] first.
 */
export function parseUnifiedDiff(patch: string): DiffResult {
  const hunks: DiffHunk[] = []
  const stat: DiffStat = { added: 0, removed: 0 }
  let current: DiffHunk | null = null

  for (const line of patch.split('\n')) {
    const header = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(line)
    if (header) {
      if (current) hunks.push(current)
      current = {
        oldStart: Number(header[1]),
        // An omitted count means 1, per the unified-diff format.
        oldLines: header[2] === undefined ? 1 : Number(header[2]),
        newStart: Number(header[3]),
        newLines: header[4] === undefined ? 1 : Number(header[4]),
        lines: [],
      }
      continue
    }
    if (!current) continue
    // `---`/`+++` are file headers, not content, and must not be counted as
    // changed lines.
    if (line.startsWith('+++') || line.startsWith('---')) continue
    if (line.startsWith('+')) {
      current.lines.push({ kind: 'added', text: line.slice(1) })
      stat.added += 1
    } else if (line.startsWith('-')) {
      current.lines.push({ kind: 'removed', text: line.slice(1) })
      stat.removed += 1
    } else if (line.startsWith(' ') || line === '') {
      current.lines.push({ kind: 'context', text: line.slice(1) })
    }
    // Anything else (e.g. `\ No newline at end of file`) is metadata; skipped.
  }
  if (current) hunks.push(current)
  return { hunks, stat, truncated: false }
}
