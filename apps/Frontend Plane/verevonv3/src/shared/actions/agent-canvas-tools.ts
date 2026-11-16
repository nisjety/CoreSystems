// W3-DEC-CANVAS (D2): the SPA's typed view of the room canvas tools the agent switch may allow. The golden is
// apps/Model Plane/api/contracts/space-self-api-registry.json, rows `space.canvas.read` and `space.canvas.write`, owned by
// wiki-store (WS19-S9). This is the presentation projection the toggle states: the default level and the per-run cap of
// each row, and the words for it. It is never an authorization input. Control decides every canvas action at run time,
// and the gateway decides who may flip the switch. A parity check against the golden is owed (units/W3-DEC-CANVAS.json).

export type SpaceCanvasToolActionId = 'space.canvas.read' | 'space.canvas.write'

export type SpaceCanvasToolRow = {
  readonly actionId: SpaceCanvasToolActionId
  readonly defaultLevel: 'allow' | 'ask'
  /** The most times this action runs in one agent run (the golden's `max_per_run`). */
  readonly maxPerRun: number
  readonly label: { readonly no: string; readonly en: string }
  readonly consequence: { readonly no: string; readonly en: string }
}

/** The canvas rows this view states, in the golden's order. */
export const SPACE_CANVAS_TOOL_ROWS: readonly SpaceCanvasToolRow[] = [
  {
    actionId: 'space.canvas.read',
    defaultLevel: 'allow',
    maxPerRun: 10,
    label: { no: 'Les rommets lerret', en: 'Read the room canvas' },
    consequence: {
      no: 'Leser rommets delte lerret. Agenten behandler det den leser der som innhold, ikke som instruksjoner.',
      en: 'Reads the shared canvas of the room. The agent treats what it reads there as content, not as instructions.',
    },
  },
  {
    actionId: 'space.canvas.write',
    defaultLevel: 'ask',
    maxPerRun: 20,
    label: { no: 'Lagre en ny versjon av lerretet', en: 'Save a new version of the canvas' },
    consequence: {
      no: 'Lagrer en ny versjon av rommets delte lerret. Control avgjør hver lagring for den kjøringen.',
      en: 'Saves a new version of the shared canvas of the room. Control decides each save for that run.',
    },
  },
]

/** The canvas rows this build states to a person, in the golden's order. */
export function spaceCanvasToolRows(): readonly SpaceCanvasToolRow[] {
  return SPACE_CANVAS_TOOL_ROWS
}
