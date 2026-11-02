/**
 * The room's tab registry (P-SPA1). The cockpit renders whatever is listed
 * here, in this order; a Space slice that adds a tab (Files, Canvas, Artifacts)
 * adds one entry to SPACE_TABS and passes its content through the cockpit's
 * `tabs` prop. Nothing else in the shell changes.
 *
 * `enabled` is a presentation switch only (a feature flag that keeps an
 * unfinished tab out of the strip). It is never an authorisation input: what a
 * tab shows is decided by the owning plane's decision, not by this list.
 */

import { roomArtifactsEnabled } from './room-artifacts'
import { roomFilesTabEnabled } from './room-files'
import { DM_SPACE_FEATURE } from './space-kind'
import { spaceRoomFeature } from './space-room-features'

export interface SpaceTabDefinition {
  readonly id: string
  readonly labelNo: string
  readonly labelEn: string
  /** What this tab answers, per the plan's six questions. */
  readonly purposeNo: string
  readonly purposeEn: string
  /**
   * Which plane must publish a Space projection before this tab can show
   * anything. Named in the unavailable state so the gap is attributable rather
   * than mysterious. Plane names are not translated: they are the system's own
   * proper nouns, same as everywhere else this codebase names a plane.
   */
  readonly owner: string
  /** Hide the tab while false. Omitted means always shown. */
  readonly enabled?: () => boolean
}

export const SPACE_TABS = [
  {
    id: 'chat',
    labelNo: 'Samtaler',
    labelEn: 'Chat',
    purposeNo: 'Alle tråder i rommet.',
    purposeEn: 'All threads in the Space.',
    owner: 'Model Plane',
  },
  {
    id: 'arbeid',
    labelNo: 'Arbeid',
    labelEn: 'Work',
    purposeNo: 'Arbeid i kø eller under kjøring, planer, overvåkinger og gjenforsøk.',
    purposeEn: 'Work queued or running, plans, monitors, and retries.',
    owner: 'Model Plane',
  },
  {
    id: 'kunnskap',
    labelNo: 'Kunnskap',
    labelEn: 'Knowledge',
    purposeNo: 'Dokumenter, kilder, filer og minne som gjelder her.',
    purposeEn: 'Documents, sources, files, and memory relevant here.',
    owner: 'Data Plane',
  },
  {
    id: 'artefakter',
    labelNo: 'Artefakter',
    labelEn: 'Artifacts',
    purposeNo: 'Dokumenter og kode agentene har skrevet i rommet.',
    purposeEn: 'Documents and code agents have written in the Space.',
    owner: 'Model Plane',
    // Hidden with the rest of the room artifact surfaces (WS07, features.room_artifacts).
    enabled: roomArtifactsEnabled,
  },
  {
    id: 'filer',
    labelNo: 'Filer',
    labelEn: 'Files',
    purposeNo: 'Tekstfiler medlemmer har lagt i rommet.',
    purposeEn: 'Text files members have added to the Space.',
    owner: 'Data Plane',
    // Hidden until the gateway reports the library (WS14, features.room_files.files_tab).
    enabled: roomFilesTabEnabled,
  },
  {
    id: 'aktivitet',
    labelNo: 'Aktivitet',
    labelEn: 'Activity',
    purposeNo: 'Hva som har skjedd, hva det kostet, og beviset for det.',
    purposeEn: 'What happened, what it cost, and the proof of it.',
    owner: 'Model Plane',
  },
  {
    id: 'agent',
    labelNo: 'Agent',
    labelEn: 'Agent',
    purposeNo: 'Aktiv modell, ferdigheter og hvilke handlinger som er tillatt her.',
    purposeEn: 'The active model, its skills, and which actions are permitted here.',
    owner: 'Model Plane',
    // A direct message has no agents (WS12-S3).
    enabled: () => !spaceRoomFeature(DM_SPACE_FEATURE),
  },
  {
    id: 'medlemmer',
    labelNo: 'Medlemmer',
    labelEn: 'Members',
    purposeNo: 'Hvem som ser og styrer rommet, og med hvilken rolle.',
    purposeEn: 'Who can see and manage the Space, and with what role.',
    owner: 'Control Plane',
    // A direct message has a fixed roster that is not edited or listed as members (WS12-S3).
    enabled: () => !spaceRoomFeature(DM_SPACE_FEATURE),
  },
] as const satisfies readonly SpaceTabDefinition[]

export type SpaceTabId = (typeof SPACE_TABS)[number]['id']

export type SpaceTabEntry = SpaceTabDefinition & { readonly id: SpaceTabId }

export const DEFAULT_SPACE_TAB: SpaceTabId = 'chat'

const LEGACY_HASH_ALIASES: Readonly<Record<string, SpaceTabId>> = {
  members: 'medlemmer',
  work: 'arbeid',
  activity: 'aktivitet',
}

/** The tabs to show now: the registry minus any whose `enabled` says no. */
export function visibleSpaceTabs(): readonly SpaceTabEntry[] {
  return (SPACE_TABS as readonly SpaceTabEntry[]).filter((tab) => tab.enabled?.() !== false)
}

/** Resolve a URL hash to a registered, visible tab; anything else is the default. */
export function spaceTabFromHash(hash: string): SpaceTabId {
  const value = hash.replace(/^#/, '').trim().toLowerCase()
  const visible = visibleSpaceTabs()
  if (visible.some((tab) => tab.id === value)) return value as SpaceTabId
  const alias = LEGACY_HASH_ALIASES[value]
  if (alias && visible.some((tab) => tab.id === alias)) return alias
  return DEFAULT_SPACE_TAB
}
