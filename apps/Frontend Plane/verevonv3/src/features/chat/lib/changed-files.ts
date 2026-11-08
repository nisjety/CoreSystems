/**
 * The files a run saved to its Space workspace (F-t3-076): the changed-files card's rows, read from the transcript.
 *
 * Pure, and recomputed from the tool calls the transcript already holds, the way the tool intents are
 * (`shared/chat-nodes/tool-presentation`), so nothing is stored or sent. Only the two workspace write tools (WS24 S4)
 * name the file they changed, and only a successful write says it went to this run's overlay
 * (`durability: "run_overlay"`). A refused write, a write still running and every other tool are not changes.
 *
 * Not listed: a change made by `code_interpreter` or a process. It reaches the overlay too, but its result names no
 * file. A server list of the overlay rows would cover it; until one exists, this is the transcript's view.
 */

import type { ChatToolCall } from '@/features/chat/components/chat-types'
import { envelope } from '@/shared/chat-nodes/tool-presentation'

/** The workspace tools that save a file (WS24 S4). */
const WRITE_TOOLS = new Set(['workspace_edit', 'workspace_put'])

/** The `durability` a successful write reports: the change lives in this run's overlay and nowhere else. */
const OVERLAY_DURABILITY = 'run_overlay'

export type ChangedFile = {
  /** The path as the write named it, relative to the Space workspace root. */
  path: string
  /** The first save to the path made a file that did not exist before the run. */
  created: boolean
  /** Successful saves to the path in these calls. */
  saves: number
}

/** One row per path, in the order the paths were first saved. */
export function changedFiles(calls: readonly ChatToolCall[]): ChangedFile[] {
  const byPath = new Map<string, ChangedFile>()
  for (const call of calls) {
    if (!WRITE_TOOLS.has(call.name.trim().toLowerCase())) continue
    const save = savedWrite(call.output)
    if (!save) continue
    const seen = byPath.get(save.path)
    if (seen) seen.saves += 1
    else byPath.set(save.path, { path: save.path, created: save.created, saves: 1 })
  }
  return [...byPath.values()]
}

/** The path and whether it was new, from a successful write's result. Anything else is not a save. */
function savedWrite(output: string | undefined): { path: string; created: boolean } | null {
  const body = envelope(output)
  if (!body || body.durability !== OVERLAY_DURABILITY) return null
  const { path, version_before: before, version_after: after } = body
  if (typeof path !== 'string' || !path.trim() || typeof after !== 'string') return null
  // The content token the write replaced. Null when the file did not exist.
  if (before !== null && typeof before !== 'string') return null
  return { path: path.trim(), created: before === null }
}
