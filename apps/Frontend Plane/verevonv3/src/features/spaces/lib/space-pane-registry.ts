import { readSpaceThreadParam, spaceThreadPaneEnabled } from './space-thread-deep-link'
import { spaceRoomFeature } from './space-room-features'

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

export type SpacePaneId = 'thread' | 'private_panel'

export interface SpacePaneDefinition {
  readonly id: SpacePaneId
  /** The search parameter that opens the pane. */
  readonly searchParam: string
  /** Normalise the raw parameter; null means "not a valid request". */
  readonly readParam: (value: string | readonly string[] | null | undefined) => string | null
  /** Closed while false. */
  readonly enabled: () => boolean
}

/** `?panel=private` asks for the owner's private panel (WS17-S6). Any other value names nothing. */
function readSpacePrivatePanelParam(value: string | readonly string[] | null | undefined): string | null {
  return typeof value === 'string' && value === 'private' ? 'private' : null
}

export const SPACE_PANES: readonly SpacePaneDefinition[] = [
  {
    id: 'thread',
    searchParam: 'thread_id',
    readParam: readSpaceThreadParam,
    enabled: spaceThreadPaneEnabled,
  },
  {
    // WS17-S6: the owner's private panel. The room's `features.private_panel` from the context read; the
    // gateway decides what the panel may show. The thread pane and this pane never open together.
    id: 'private_panel',
    searchParam: 'panel',
    readParam: readSpacePrivatePanelParam,
    enabled: () => spaceRoomFeature('private_panel'),
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
