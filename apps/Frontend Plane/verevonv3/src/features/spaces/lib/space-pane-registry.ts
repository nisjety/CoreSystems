import { readSpaceThreadParam, spaceThreadPaneEnabled } from './space-thread-deep-link'

/**
 * The room's right-hand pane registry (P-SPA1). SpaceAuxPane is the host; this
 * list says which panes exist and which URL parameter opens each. A Space slice
 * that adds a pane (private panel, browser view, files preview) adds one entry
 * here and renders it next to the thread pane in SpacePage.
 *
 * A parameter is only a request to open a pane: the pane resolves its content
 * through the owning plane, which decides access. `enabled` keeps an unfinished
 * pane closed and is never an authorisation input.
 */

export type SpacePaneId = 'thread'

export interface SpacePaneDefinition {
  readonly id: SpacePaneId
  /** The search parameter that opens the pane. */
  readonly searchParam: string
  /** Normalise the raw parameter; null means "not a valid request". */
  readonly readParam: (value: string | readonly string[] | null | undefined) => string | null
  /** Closed while false. */
  readonly enabled: () => boolean
}

export const SPACE_PANES: readonly SpacePaneDefinition[] = [
  {
    id: 'thread',
    searchParam: 'thread_id',
    readParam: readSpaceThreadParam,
    enabled: spaceThreadPaneEnabled,
  },
]

export interface OpenSpacePane {
  readonly id: SpacePaneId
  readonly value: string
}

/** The pane the URL asks for: the first registered, enabled pane with a valid parameter. */
export function resolveOpenSpacePane(
  params: Readonly<Record<string, string | readonly string[] | null | undefined>>,
): OpenSpacePane | null {
  for (const pane of SPACE_PANES) {
    if (!pane.enabled()) continue
    const value = pane.readParam(params[pane.searchParam])
    if (value !== null) return { id: pane.id, value }
  }
  return null
}
