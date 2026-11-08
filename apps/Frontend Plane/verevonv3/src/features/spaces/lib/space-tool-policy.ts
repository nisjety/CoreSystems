import type {
  SpaceMergedToolPolicy,
  SpaceToolLevel,
  SpaceToolPolicyEntry,
  SpaceToolSource,
} from '@/shared/api/spaces-client'
import { spaceRoomFeatureWithDevOverride } from './space-room-features'

// WS10-S5b: the pure half of the per-tool policy editor. Control stores two layers (the Space
// ceiling and one layer per agent) and merges them tighten-only: allow < ask < block, the stricter
// layer wins, and within a layer a specific tool key beats `*`. Everything here is presentation:
// Control re-validates every write and decides who may make it.

type Tr = (no: string, en: string) => string

export const TOOL_POLICY_LEVELS: readonly SpaceToolLevel[] = ['allow', 'ask', 'block']
export const TOOL_POLICY_WILDCARD = '*'
export const TOOL_POLICY_MAX_ENTRIES = 200
export const TOOL_POLICY_MAX_KEY_CHARS = 200

/** Whether the Agent tab offers the editor: `features.tool_policy_editor` from the context read. */
export function toolPolicyEditorEnabled(): boolean {
  return spaceRoomFeatureWithDevOverride('tool_policy_editor', import.meta.env.VITE_SPACE_TOOL_POLICY_EDITOR)
}

export function toolLevelRank(level: SpaceToolLevel): number {
  return level === 'allow' ? 1 : level === 'ask' ? 2 : 3
}

export function toolLevelLabel(level: SpaceToolLevel, tr: Tr): string {
  switch (level) {
    case 'allow':
      return tr('Tillat', 'Allow')
    case 'ask':
      return tr('Spør først', 'Ask first')
    case 'block':
      return tr('Blokker', 'Block')
  }
}

/** A sentence for what the level does; "allow" never waives a default approval gate. */
export function toolLevelHint(level: SpaceToolLevel, tr: Tr): string {
  switch (level) {
    case 'allow':
      return tr(
        'Ingen ekstra regel. Vanlige godkjenninger gjelder fortsatt.',
        'No extra rule. The usual approvals still apply.',
      )
    case 'ask':
      return tr('Kallet venter på en godkjenning hver gang.', 'The call waits for an approval every time.')
    case 'block':
      return tr('Kallet blir nektet.', 'The call is refused.')
  }
}

export function toolSourceLabel(source: SpaceToolSource, tr: Tr): string {
  switch (source) {
    case 'space_ceiling':
      return tr('Fra rommets tak', 'Set by the room ceiling')
    case 'agent':
      return tr('Fra agentens egen regel', 'Set by the agent rule')
    case 'default':
      return tr('Ingen regel', 'No rule')
  }
}

/** A tool key Control accepts: `*` or a printable key without whitespace, at most 200 characters. */
export function validToolKey(key: string): boolean {
  if (key !== key.trim() || key.length === 0 || key.length > TOOL_POLICY_MAX_KEY_CHARS) return false
  // eslint-disable-next-line no-control-regex
  return !/[\s\u0000-\u001f\u007f]/u.test(key)
}

/** One editable row. `id` keeps a row stable while its text changes. */
export type ToolPolicyDraftRow = {
  readonly id: number
  readonly toolKey: string
  readonly level: SpaceToolLevel
}

let nextRowId = 1

export function draftRows(entries: readonly SpaceToolPolicyEntry[]): ToolPolicyDraftRow[] {
  return entries.map((entry) => ({ id: nextRowId++, toolKey: entry.toolKey, level: entry.level }))
}

export function blankRow(toolKey = ''): ToolPolicyDraftRow {
  return { id: nextRowId++, toolKey, level: 'ask' }
}

export type ToolPolicyDraftProblem = 'empty_key' | 'invalid_key' | 'duplicate_key' | 'too_many'

/** The first problem that would make Control refuse the write, or `null`. */
export function draftProblem(rows: readonly ToolPolicyDraftRow[]): ToolPolicyDraftProblem | null {
  if (rows.length > TOOL_POLICY_MAX_ENTRIES) return 'too_many'
  const seen = new Set<string>()
  for (const row of rows) {
    const key = row.toolKey.trim()
    if (key.length === 0) return 'empty_key'
    if (!validToolKey(key)) return 'invalid_key'
    if (seen.has(key)) return 'duplicate_key'
    seen.add(key)
  }
  return null
}

export function draftProblemMessage(problem: ToolPolicyDraftProblem, tr: Tr): string {
  switch (problem) {
    case 'empty_key':
      return tr('Hver regel trenger et verktøynavn, eller * for alle.', 'Every rule needs a tool name, or * for all tools.')
    case 'invalid_key':
      return tr(
        'Verktøynavn kan ikke ha mellomrom og kan være opptil 200 tegn.',
        'Tool names cannot contain spaces and can be up to 200 characters.',
      )
    case 'duplicate_key':
      return tr('Samme verktøy er nevnt to ganger.', 'The same tool appears twice.')
    case 'too_many':
      return tr('En regelliste kan ha opptil 200 regler.', 'A rule list can hold up to 200 rules.')
  }
}

/** The entries to send: trimmed keys, in row order. Call only when {@link draftProblem} is null. */
export function entriesFromDraft(rows: readonly ToolPolicyDraftRow[]): SpaceToolPolicyEntry[] {
  return rows.map((row) => ({ toolKey: row.toolKey.trim(), level: row.level }))
}

/** True when the draft would write the same layer that is stored (order does not matter). */
export function draftMatchesStored(
  rows: readonly ToolPolicyDraftRow[],
  stored: readonly SpaceToolPolicyEntry[],
): boolean {
  if (rows.length !== stored.length) return false
  const byKey = new Map(stored.map((entry) => [entry.toolKey, entry.level]))
  return rows.every((row) => byKey.get(row.toolKey.trim()) === row.level)
}

/**
 * The level the merged view gives a tool: its own entry, else `*`, else allow from no rule. A tool
 * the merged view does not name takes the `*` entry, as Control's `LevelFor` does.
 */
export function mergedLevelFor(
  merged: SpaceMergedToolPolicy,
  toolKey: string,
): { readonly level: SpaceToolLevel; readonly source: SpaceToolSource } {
  return merged.tools[toolKey] ?? merged.tools[TOOL_POLICY_WILDCARD] ?? { level: 'allow', source: 'default' }
}

/**
 * Whether an agent-layer row is held down by the Space ceiling: the merged level is stricter than
 * the row says and the ceiling decided it. The agent layer cannot loosen the ceiling, so the editor
 * says so rather than showing a level that will not apply.
 */
export function rowOverriddenByCeiling(
  merged: SpaceMergedToolPolicy,
  row: Pick<ToolPolicyDraftRow, 'toolKey' | 'level'>,
): boolean {
  const effective = mergedLevelFor(merged, row.toolKey.trim())
  return effective.source === 'space_ceiling' && toolLevelRank(effective.level) > toolLevelRank(row.level)
}

/** The tool keys the merged view names, `*` first, then alphabetical: the table the editor shows. */
export function mergedRows(
  merged: SpaceMergedToolPolicy,
): { readonly toolKey: string; readonly level: SpaceToolLevel; readonly source: SpaceToolSource }[] {
  return Object.keys(merged.tools)
    .sort((a, b) =>
      a === TOOL_POLICY_WILDCARD ? -1 : b === TOOL_POLICY_WILDCARD ? 1 : a.localeCompare(b),
    )
    .map((toolKey) => ({ toolKey, ...merged.tools[toolKey]! }))
}
