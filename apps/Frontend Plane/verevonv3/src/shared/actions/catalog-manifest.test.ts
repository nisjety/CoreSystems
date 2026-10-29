import { describe, expect, it } from 'vitest'
import { actionRegistry } from '@/shared/actions/action-registry'
import {
  ACTION_CATALOG_SCHEMA_VERSION,
  actionCatalogSha256,
  buildActionCatalogManifest,
  canonicalActionCatalogJson,
} from './catalog-manifest'

describe('provisional Action Catalog manifest', () => {
  it('is deterministic, versioned, and sorted independently of source order', () => {
    const forward = buildActionCatalogManifest(actionRegistry)
    const reverse = buildActionCatalogManifest([...actionRegistry].reverse())

    expect(forward.schemaVersion).toBe(ACTION_CATALOG_SCHEMA_VERSION)
    expect(forward).toEqual(reverse)
    expect(forward.actions.map((action) => action.id)).toEqual(
      [...forward.actions.map((action) => action.id)].sort(),
    )
    expect(canonicalActionCatalogJson(actionRegistry)).toBe(canonicalActionCatalogJson([...actionRegistry].reverse()))
  })

  it('records browser-only actions as human-only until their owner adapter is live', () => {
    const manifest = buildActionCatalogManifest()
    expect(manifest.actions).not.toHaveLength(0)
    expect(manifest.actions.every((action) => action.allowedActorTypes.join(',') === 'human')).toBe(true)
  })

  it('does not confuse legacy dispatch with an owner-issued operation receipt', () => {
    const manifest = buildActionCatalogManifest()
    const ticketCreate = manifest.actions.find((action) => action.id === 'tickets.create')
    const ticketUpdate = manifest.actions.find((action) => action.id === 'tickets.update')

    expect(ticketCreate).toMatchObject({
      executionContract: 'owner_operation_receipt',
      idempotency: 'caller_supplied',
    })
    expect(ticketUpdate).toMatchObject({
      executionContract: 'legacy_direct',
      idempotency: 'not_yet_contractual',
    })
  })

  it('produces a stable SHA-256 digest', async () => {
    await expect(actionCatalogSha256()).resolves.toMatch(/^[a-f0-9]{64}$/)
    await expect(actionCatalogSha256()).resolves.toBe(await actionCatalogSha256())
  })
})
