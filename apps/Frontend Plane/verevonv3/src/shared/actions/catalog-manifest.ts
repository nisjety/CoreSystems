import { z } from 'zod'
import { actionRegistry } from '@/shared/actions/action-registry'
import { isModelExecutableAction } from '@/shared/actions/model-eligibility'
import {
  MODEL_ACCESS,
  type ModelAccess,
  type ModelAccessEntry,
  type ModelExemptReason,
  type ModelSideEffectClass,
} from '@/shared/actions/model-access'
import type { ActionDescriptor } from '@/shared/actions/types'

export const ACTION_CATALOG_SCHEMA_VERSION = 3 as const

export type ActionExecutionContract =
  | 'legacy_direct'
  | 'owner_operation_receipt'

export type ActionCatalogManifest = Readonly<{
  actions: readonly ActionCatalogEntry[]
  schemaVersion: typeof ACTION_CATALOG_SCHEMA_VERSION
}>

export type ActionCatalogEntry = Readonly<{
  allowedActorTypes: readonly ('human' | 'model')[]
  description: string
  executionContract: ActionExecutionContract
  exemptReason: ModelExemptReason | null
  id: string
  idempotency: 'caller_supplied' | 'not_yet_contractual'
  inputSchema: unknown
  label: string
  modelAccess: ModelAccess
  outputSchema: unknown
  ownerPlane: string
  requiresApproval: boolean
  reversible: boolean
  risk: string
  schemaHash: string
  sideEffectClass: ModelSideEffectClass
}>

function executionContractFor(action: ActionDescriptor): Pick<ActionCatalogEntry, 'executionContract' | 'idempotency'> {
  // This is deliberately a tiny, reviewable migration map. The generic
  // registry is not permitted to imply that a direct legacy dispatcher has an
  // owner-issued receipt merely because it returns an HTTP success response.
  if (action.id === 'tickets.create') {
    return {
      executionContract: 'owner_operation_receipt',
      idempotency: 'caller_supplied',
    }
  }
  return {
    executionContract: 'legacy_direct',
    idempotency: 'not_yet_contractual',
  }
}

function schemaJson(schema: z.ZodType): unknown {
  return z.toJSONSchema(schema)
}

function modelAccessFor(actionId: string): ModelAccessEntry {
  // An id with no row fails closed: the catalog never invents an access level.
  const entry = (MODEL_ACCESS as Readonly<Record<string, ModelAccessEntry | undefined>>)[actionId]
  if (!entry) throw new Error(`Action catalog has no model access row for: ${actionId}`)
  return entry
}

async function sha256Hex(text: string): Promise<string> {
  if (!globalThis.crypto?.subtle) {
    throw new Error('Web Crypto is required to hash the Action Catalog')
  }
  const digest = await globalThis.crypto.subtle.digest('SHA-256', new TextEncoder().encode(text))
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('')
}

/**
 * An owner-pinned row keeps the owner's digest (DEC-25). Any other row hashes its own
 * id and schemas, so a changed input or output schema changes the admission contract.
 */
export async function actionSchemaHash(action: ActionDescriptor): Promise<string> {
  const access = modelAccessFor(action.id)
  if (access.ownerSchemaHash) return access.ownerSchemaHash
  const schemas = canonicalJson({
    id: action.id,
    inputSchema: schemaJson(action.inputSchema),
    outputSchema: schemaJson(action.outputSchema),
  })
  return `sha256:${await sha256Hex(schemas)}`
}

async function entryFor(action: ActionDescriptor): Promise<ActionCatalogEntry> {
  const access = modelAccessFor(action.id)
  return {
    allowedActorTypes: isModelExecutableAction(action.id) ? ['human', 'model'] : ['human'],
    description: action.description,
    ...executionContractFor(action),
    exemptReason: access.access === 'none' ? access.exemptReason : null,
    id: action.id,
    inputSchema: schemaJson(action.inputSchema),
    label: action.label,
    modelAccess: access.access,
    outputSchema: schemaJson(action.outputSchema),
    ownerPlane: action.ownerPlane,
    requiresApproval: action.requiresApproval,
    reversible: action.reversible,
    risk: action.risk,
    schemaHash: await actionSchemaHash(action),
    sideEffectClass: access.sideEffectClass,
  }
}

/**
 * The release artifact is sorted by immutable action id rather than source
 * order. This makes catalog review, hashing, and owner-plane code generation
 * deterministic. It is deliberately marked provisional until owner planes
 * contribute signatures and runtime availability attestations.
 */
export async function buildActionCatalogManifest(
  actions: readonly ActionDescriptor[] = actionRegistry,
): Promise<ActionCatalogManifest> {
  const entries = (await Promise.all(actions.map(entryFor))).sort((left, right) => left.id.localeCompare(right.id))
  const duplicate = entries.find((entry, index) => entry.id === entries[index - 1]?.id)
  if (duplicate) throw new Error(`Action catalog contains duplicate id: ${duplicate.id}`)
  return {
    actions: entries,
    schemaVersion: ACTION_CATALOG_SCHEMA_VERSION,
  }
}

function canonicalJson(value: unknown): string {
  if (value === undefined) return 'null'
  if (value === null || typeof value !== 'object') return JSON.stringify(value)
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`
  const record = value as Record<string, unknown>
  return `{${Object.keys(record).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(record[key])}`).join(',')}}`
}

export async function canonicalActionCatalogJson(
  actions: readonly ActionDescriptor[] = actionRegistry,
): Promise<string> {
  return canonicalJson(await buildActionCatalogManifest(actions))
}

export async function actionCatalogSha256(
  actions: readonly ActionDescriptor[] = actionRegistry,
): Promise<string> {
  return sha256Hex(await canonicalActionCatalogJson(actions))
}
