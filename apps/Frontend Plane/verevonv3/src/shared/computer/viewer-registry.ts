/**
 * Computer panel viewer registry (EP-1 W4-B, P1, F-t3-035).
 *
 * One table that says which viewer draws a tool call, keyed on the call's recomputed intent
 * (`shared/chat-nodes/tool-presentation`). Pure and JSX-free, like the artifact registry beside it: the components
 * stay in `features/chat/components/computer/`, and this table only answers `pickComputerViewer(intent)`.
 *
 *   - `terminal`: a command or a process the agent ran, as the transcript the tool returned. Read only.
 *   - `file`: a workspace file or listing, as the text the tool returned. Read only, never rendered as markup.
 *
 * A browser step has no entry: its live page belongs to the run panel's browser view, so its card keeps the raw
 * output. A tool with no entry keeps its raw output too, so the registry adds a view and never hides one.
 */

import type { ToolIntent } from '@/shared/chat-nodes/tool-presentation'

export type ComputerViewerKind = 'terminal' | 'file'

export type ComputerViewerEntry = {
  kind: ComputerViewerKind
  /** The intents this viewer draws. Each intent belongs to at most one entry. */
  intents: readonly ToolIntent[]
}

export const COMPUTER_VIEWERS: readonly ComputerViewerEntry[] = [
  { kind: 'terminal', intents: ['terminal', 'process'] },
  { kind: 'file', intents: ['file'] },
]

/** The viewer a tool call of this intent is drawn in, or `undefined` when its card keeps the raw output. */
export function pickComputerViewer(intent: ToolIntent): ComputerViewerEntry | undefined {
  return COMPUTER_VIEWERS.find((entry) => entry.intents.includes(intent))
}
