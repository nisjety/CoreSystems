// WS18-S6: the SPA's typed view of the Space agent self-API registry. The golden is
// apps/Model Plane/api/contracts/space-self-api-registry.json. This app keeps a byte-identical copy at
// ./api/contracts/space-self-api-registry.json, pinned by the sha256 beside it. This file is the presentation
// projection the toggle states: the fields a person needs to read what the switch allows (risk, default level,
// the per-run cap) and the words for each row. It is never an authorization input. Control decides every action
// at run time, and the gateway decides who may flip the switch. A parity test (owed, units/WS18-S6.json) checks
// the rows here against the golden and the pin.

/** The sha256 of the vendored golden. Changing the golden changes this too. */
export const REGISTRY_GOLDEN_DIGEST = '635587a25114dcd34d183a7de0ff43e13ce935a990d849a57a063a6640c09f8d'

export type SpaceSelfApiActionId = 'space.thread.rename' | 'space.thread.pin' | 'space.thread.post_card'

export type SpaceSelfApiRow = {
  readonly actionId: SpaceSelfApiActionId
  readonly risk: 'low' | 'medium' | 'high'
  readonly defaultLevel: 'allow' | 'ask'
  readonly phase: number
  /** The most times this action runs in one agent run (the golden's `max_per_run`). */
  readonly maxPerRun: number
  /** Whether this build offers the action to a room agent. post_card waits for WS18-S7 (the card slot rule). */
  readonly offered: boolean
  readonly label: { readonly no: string; readonly en: string }
  readonly consequence: { readonly no: string; readonly en: string }
}

/**
 * The WS18 rows of the golden that this view states, in its order (phase 1). Not in this view: the phase 2 and 3 rows
 * (`space.agent.pause_self`, `space.schedule.follow_up`), the WS19 canvas rows (`space.canvas.read`, `space.canvas.write`,
 * owned by wiki-store) and `tickets.create` (the Conversation Core row). This view does not state any of these.
 */
export const SPACE_SELF_API_ROWS: readonly SpaceSelfApiRow[] = [
  {
    actionId: 'space.thread.rename',
    risk: 'low',
    defaultLevel: 'allow',
    phase: 1,
    maxPerRun: 2,
    offered: true,
    label: { no: 'Gi tråden nytt navn', en: 'Rename the thread' },
    consequence: {
      no: 'Setter tittelen på tråden den kjører i. En tittel satt av et menneske blir aldri endret.',
      en: 'Sets the title of the thread it runs in. A title a person set is never changed.',
    },
  },
  {
    actionId: 'space.thread.pin',
    risk: 'low',
    defaultLevel: 'allow',
    phase: 1,
    maxPerRun: 2,
    offered: true,
    label: { no: 'Fest eller løsne tråden', en: 'Pin or unpin the thread' },
    consequence: {
      no: 'Fester tråden den kjører i, eller løsner den. En festing satt av et menneske blir aldri endret.',
      en: 'Pins the thread it runs in, or unpins it. A pin a person set is never changed.',
    },
  },
  {
    actionId: 'space.thread.post_card',
    risk: 'low',
    defaultLevel: 'allow',
    phase: 1,
    maxPerRun: 3,
    offered: false,
    label: { no: 'Legg inn et kort i tråden', en: 'Post a card in the thread' },
    consequence: {
      no: 'Legger inn et kort med tittel og tekst i tråden den kjører i.',
      en: 'Posts a card with a title and text in the thread it runs in.',
    },
  },
]

/** The rows this build offers a room agent, in the golden's order. */
export function spaceSelfApiOfferedRows(): readonly SpaceSelfApiRow[] {
  return SPACE_SELF_API_ROWS.filter((row) => row.offered)
}
