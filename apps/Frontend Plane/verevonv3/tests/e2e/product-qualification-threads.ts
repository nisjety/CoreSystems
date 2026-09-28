import { globSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'

export type QualifyingThread = {
  scenario: string
  threadId: string
  startedAt: string
  build?: string
  model?: string
  provider?: string
}

const DEFAULT_CAPTURE_ROOT = 'C:/private/verevon-recordings/q16-20260926T0636Z'

/** The manifest gate's own definition of "qualifying": 5 consecutive accepted passes. */
export const QUALIFYING_STREAK_LENGTH = 5

/**
 * Reproduces the qualifying streak (the last N *accepted* takes per
 * scenario) straight from the private capture folder's own `capture.json`
 * files, without hardcoding take numbers a future freeze will change.
 * `Q16_CAPTURE_ROOT` overrides the default folder for a differently-named
 * freeze. Nothing here reads or copies the capture's prose/report content —
 * only the identifiers (scenario, threadId, startedAt, build) a durability
 * or experience check needs to reopen a thread.
 *
 * Returns `[]` (never throws) when the capture root is unreadable, so a
 * checkout without the private folder simply skips these specs instead of
 * failing the whole run. But once a scenario has ANY accepted takes, its
 * streak must be a real "N consecutive passes on a frozen build" — the
 * manifest gate's own requirement (`consecutivePassesOnReleaseBuild`) — so a
 * streak spanning more than one `build` hash (e.g. a partial rebuild mid
 * freeze) throws rather than being silently treated as qualifying: that is
 * itself a gate failure, not data to quietly filter around.
 */
export function qualifyingThreads(streakLength = QUALIFYING_STREAK_LENGTH): QualifyingThread[] {
  const root = process.env.Q16_CAPTURE_ROOT || DEFAULT_CAPTURE_ROOT
  let files: string[]
  try {
    files = [...globSync('**/capture.json', { cwd: root })]
  } catch {
    return []
  }
  const byScenario = new Map<string, QualifyingThread[]>()
  for (const file of files) {
    let raw: Record<string, unknown>
    try {
      raw = JSON.parse(readFileSync(resolve(root, file), 'utf8')) as Record<string, unknown>
    } catch {
      continue
    }
    if (raw.accepted !== true) continue
    const scenario = raw.scenario
    const threadId = raw.threadId
    if (typeof scenario !== 'string' || typeof threadId !== 'string' || !threadId) continue
    const entry: QualifyingThread = {
      scenario,
      threadId,
      startedAt: typeof raw.startedAt === 'string' ? raw.startedAt : '',
      build: typeof raw.build === 'string' ? raw.build : undefined,
      model: typeof raw.model === 'string' ? raw.model : undefined,
      provider: typeof raw.provider === 'string' ? raw.provider : undefined,
    }
    const list = byScenario.get(scenario) ?? []
    list.push(entry)
    byScenario.set(scenario, list)
  }
  const qualifying: QualifyingThread[] = []
  for (const [scenario, list] of byScenario) {
    list.sort((a, b) => a.startedAt.localeCompare(b.startedAt))
    const streak = list.slice(-streakLength)
    const builds = new Set(streak.map((t) => t.build ?? '(unknown build)'))
    if (builds.size > 1) {
      throw new Error(
        `${scenario}'s last ${streak.length} accepted takes span ${builds.size} different builds `
        + `(${[...builds].join(', ')}) — not a same-build streak, so it cannot satisfy the manifest gate's `
        + `"consecutive passes on a frozen build" requirement. Re-run the missing takes on one build before `
        + 'trusting this streak.',
      )
    }
    qualifying.push(...streak)
  }
  return qualifying.sort((a, b) => a.scenario === b.scenario
    ? a.startedAt.localeCompare(b.startedAt)
    : a.scenario.localeCompare(b.scenario))
}

/** One thread per scenario — the latest qualifying take — for checks that only need a representative sample (e.g. per-viewport experience checks), not the whole streak. */
export function latestQualifyingThreadPerScenario(streakLength = QUALIFYING_STREAK_LENGTH): QualifyingThread[] {
  const byScenario = new Map<string, QualifyingThread>()
  for (const thread of qualifyingThreads(streakLength)) byScenario.set(thread.scenario, thread)
  return [...byScenario.values()].sort((a, b) => a.scenario.localeCompare(b.scenario))
}
