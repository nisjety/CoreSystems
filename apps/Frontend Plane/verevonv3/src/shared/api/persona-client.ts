import { requestJson } from './http'

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null
}

export type PersonaInstructions = {
  instructions: string
  enabled: boolean
}

/**
 * Persona spec — the user layer of the authored-instruction hierarchy,
 * beneath org/Space (`organization-client.ts`'s `getOrganizationInstructions`
 * is the direct precedent this mirrors). Self-owned: no org/admin gate,
 * always the signed-in caller's own row (`apps/gateway/src/domains/persona.rs`).
 */
export async function getPersonaInstructions(): Promise<PersonaInstructions> {
  const payload = await requestJson<unknown>('/api/v1/me/persona')
  const data = record(payload)
  const instructions = typeof data?.instructions === 'string' ? data.instructions : ''
  const enabled = typeof data?.enabled === 'boolean' ? data.enabled : true
  return { instructions, enabled }
}

export async function updatePersonaInstructions(
  instructions: string,
  enabled: boolean,
): Promise<PersonaInstructions> {
  const payload = await requestJson<unknown>('/api/v1/me/persona', {
    method: 'PATCH',
    body: JSON.stringify({ instructions, enabled }),
  })
  const data = record(payload)
  const savedInstructions = typeof data?.instructions === 'string' ? data.instructions : instructions
  const savedEnabled = typeof data?.enabled === 'boolean' ? data.enabled : enabled
  return { instructions: savedInstructions, enabled: savedEnabled }
}
