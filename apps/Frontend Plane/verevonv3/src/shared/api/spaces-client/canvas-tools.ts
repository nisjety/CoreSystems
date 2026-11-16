// W3-DEC-CANVAS (D2): the agent canvas-tools switch's reply. The switch is written through the action registry,
// `executeAction('spaces.set_agent_canvas_tools')` (apps/gateway/src/domains/actions/dispatchers_spaces/ws18.rs), whose
// `result` is the gateway's `data`: `{binding_ref, canvas_tools_enabled, projection_version, changed}`. A reply that
// does not match fails closed: the caller shows its error and writes nothing based on a value it could not read. A
// refusal arrives as the ApiError the gateway sent, with its typed code (features/spaces/lib/space-canvas-tools.ts).

export type SpaceCanvasToolsSwitch = {
  readonly bindingRef: string
  readonly canvasToolsEnabled: boolean
  /** The binding's projection version after the write; null when the gateway sent none. */
  readonly projectionVersion: number | null
  /** Whether the write changed the stored value. */
  readonly changed: boolean
}

const INVALID_CANVAS_TOOLS_REPLY = 'invalid_canvas_tools_response'

/** Reads the switch's reply. Anything outside the contract throws, so no value is shown that was not read. */
export function spaceCanvasToolsSwitchFrom(value: unknown): SpaceCanvasToolsSwitch {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(INVALID_CANVAS_TOOLS_REPLY)
  const row = value as Record<string, unknown>
  const bindingRef = typeof row.binding_ref === 'string' ? row.binding_ref.trim() : ''
  const canvasToolsEnabled = row.canvas_tools_enabled
  const changed = row.changed
  if (!bindingRef || typeof canvasToolsEnabled !== 'boolean' || typeof changed !== 'boolean') {
    throw new Error(INVALID_CANVAS_TOOLS_REPLY)
  }
  const raw = row.projection_version ?? null
  if (raw !== null && (typeof raw !== 'number' || !Number.isFinite(raw))) {
    throw new Error(INVALID_CANVAS_TOOLS_REPLY)
  }
  const projectionVersion: number | null = typeof raw === 'number' ? raw : null
  return { bindingRef, canvasToolsEnabled, projectionVersion, changed }
}
