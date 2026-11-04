import { describe, expect, it } from 'vitest'
import { actionRegistry } from '@/shared/actions/action-registry'
import { buildActionCatalogManifest } from '@/shared/actions/catalog-manifest'
import { MODEL_ACCESS } from '@/shared/actions/model-access'
import { isModelExecutableAction } from '@/shared/actions/model-eligibility'

/**
 * WS05-S4 (audit list), WS05-S6 (domain verification) and WS05-S7 (SSO policy): every
 * identity and audit action is registered, human-only, and outside the model-executable
 * allowlist. An agent never reads the audit log, claims a domain or changes how people sign in.
 */

const AUDIT_IDS = ['audit.list_events']
const DOMAIN_IDS = ['org.add_domain', 'org.verify_domain', 'org.revoke_domain']
const SSO_IDS = ['org.update_sso_policy', 'org.add_sso_provider', 'org.remove_sso_provider']

const registeredIds = () => actionRegistry.map((action) => action.id as string)

describe('identity and audit actions (WS05-S4, WS05-S6, WS05-S7)', () => {
  it('registers every action exactly once', () => {
    const ids = registeredIds()
    for (const id of [...AUDIT_IDS, ...DOMAIN_IDS, ...SSO_IDS]) {
      expect(ids.filter((candidate) => candidate === id), id).toHaveLength(1)
    }
  })

  it('keeps every one of them out of the model-executable allowlist', () => {
    for (const id of [...AUDIT_IDS, ...DOMAIN_IDS, ...SSO_IDS]) {
      expect(isModelExecutableAction(id), id).toBe(false)
    }
  })

  it('closes each model access row with a reason and no eligibility', () => {
    for (const id of [...AUDIT_IDS, ...DOMAIN_IDS, ...SSO_IDS]) {
      const entry = (MODEL_ACCESS as Record<string, (typeof MODEL_ACCESS)[keyof typeof MODEL_ACCESS]>)[id]
      expect(entry, id).toBeDefined()
      expect(entry!.access, id).toBe('none')
      expect(entry!.eligible, id).toBe(false)
      expect('exemptReason' in entry! && entry.exemptReason, id).toBeTruthy()
    }
    // Domain and SSO changes are identity/security decisions; the audit list has no model executor.
    for (const id of [...DOMAIN_IDS, ...SSO_IDS]) {
      expect(MODEL_ACCESS[id as keyof typeof MODEL_ACCESS]).toMatchObject({ exemptReason: 'human_identity_or_security' })
    }
    expect(MODEL_ACCESS['audit.list_events']).toMatchObject({ exemptReason: 'no_owner_executor' })
  })

  it('lists them in the catalog manifest for human actors only', async () => {
    const manifest = await buildActionCatalogManifest()
    for (const id of [...AUDIT_IDS, ...DOMAIN_IDS, ...SSO_IDS]) {
      const entry = manifest.actions.find((action) => action.id === id)
      expect(entry, id).toBeDefined()
      expect(entry!.allowedActorTypes, id).toEqual(['human'])
      expect(entry!.modelAccess, id).toBe('none')
      expect(entry!.exemptReason, id).not.toBeNull()
      expect(entry!.ownerPlane, id).toBe('control')
    }
  })

  it('treats audit.list_events as a low-risk, approval-free read with no export in it', () => {
    const list = actionRegistry.find((action) => action.id === 'audit.list_events')!
    expect(list.risk).toBe('low')
    expect(list.requiresApproval).toBe(false)
    expect(list.description).toMatch(/no export/i)
    // WS05-S4 added no export action; the only export is the separate compliance action.
    const ids = registeredIds()
    expect(ids).not.toContain('audit.export')
    expect(ids).not.toContain('audit.list_events.export')
    expect(ids.filter((id) => id.startsWith('audit.') && /export/.test(id))).toEqual(['audit.export_log'])
  })

  it('gates the destructive identity changes behind approval and leaves claiming a domain light', () => {
    const byId = (id: string) => actionRegistry.find((action) => action.id === id)!
    expect(byId('org.add_domain').requiresApproval).toBe(false)
    expect(byId('org.verify_domain').requiresApproval).toBe(false)
    expect(byId('org.revoke_domain').requiresApproval).toBe(true)
    for (const id of SSO_IDS) {
      expect(byId(id).requiresApproval, id).toBe(true)
      expect(byId(id).risk, id).toBe('high')
    }
    expect(byId('org.remove_sso_provider').reversible).toBe(false)
  })

  it('describes the SSO client secret as write-only', () => {
    const add = actionRegistry.find((action) => action.id === 'org.add_sso_provider')!
    expect(add.description).toMatch(/write-only/i)
  })
})
