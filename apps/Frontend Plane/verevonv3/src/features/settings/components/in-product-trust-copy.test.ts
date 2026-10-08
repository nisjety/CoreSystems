import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

// Two sentences in the product made claims the code does not back (docs/compliance/evidence/claims-vs-code.md,
// rows P5 and P6): the erase dialog said Model Plane and Data Plane data "is removed via the separate erasure
// fan-out", and the disconnect note said disconnecting "revokes access and purges cached data". They were
// narrowed on 2026-10-03. This test pins the narrowing to the copy and to the code it rests on, so the old
// sentences cannot come back and the new ones cannot outlive their evidence.
//
// The components are Solid and render through i18n, so the test reads their source text.

const here = dirname(fileURLToPath(import.meta.url))

function repoRoot(): string {
  // TRUST_REPO_ROOT lets the mutation check run this file from a scratch folder against the real evidence.
  if (process.env.TRUST_REPO_ROOT) return process.env.TRUST_REPO_ROOT
  let dir = here
  for (;;) {
    if (existsSync(join(dir, 'docs', 'compliance', 'evidence', 'claims-vs-code.md'))) return dir
    const parent = dirname(dir)
    if (parent === dir) throw new Error('repository root not found')
    dir = parent
  }
}

const squash = (text: string) => text.replace(/\s+/g, ' ')
const read = (...parts: string[]) => readFileSync(join(...parts), 'utf8')

const privacy = squash(read(here, 'PrivacyDataSection.tsx'))
const trust = squash(read(here, 'TrustCenterSection.tsx'))

describe('erase dialog (P5)', () => {
  it('no longer says other planes are erased through a separate fan-out', () => {
    expect(privacy).not.toContain('fjernes gjennom den separate slettingsprosessen')
    expect(privacy).not.toContain('removed via the separate erasure fan-out')
  })

  it('says, in both languages, that conversations, memory and documents are not deleted', () => {
    expect(privacy).toContain('Det sletter ikke samtaler, minne eller dokumenter i andre deler av Verevon')
    expect(privacy).toContain('It does not delete conversations, memory or documents held in other parts of Verevon')
    expect(privacy).toContain('slett samtalene og minnene du vil fjerne før du sletter kontoen')
    expect(privacy).toContain('delete the conversations and memories you want removed before you erase the account')
  })

  it('rests on session-core acknowledging a user erasure without deleting anything', () => {
    const root = repoRoot()
    const gdpr = read(root, 'apps', 'Model Plane', 'rust', 'services', 'session-core', 'src', 'gdpr.rs')
    expect(gdpr).toContain('NOT a request to erase that org')
    expect(gdpr).toContain('return Ok(None)')
    const nats = read(root, 'apps', 'Model Plane', 'rust', 'services', 'session-core', 'src', 'gdpr_nats.rs')
    expect(nats).toContain('Ok(Outcome::Skipped)')
  })
})

describe('disconnect note (P6)', () => {
  it('no longer says disconnecting revokes access and purges cached data', () => {
    expect(trust).not.toContain('tilbakekaller tilgangen og sletter mellomlagrede data')
    expect(trust).not.toContain('revokes its access and purges its cached data')
  })

  it('says, in both languages, that Microsoft access is not revoked and that content deletion is unverified', () => {
    expect(trust).toContain('Hos noen leverandører, blant annet Microsoft, trekkes ikke tilgangen tilbake hos leverandøren')
    expect(trust).toContain('At some providers, Microsoft among them, the access is not revoked at the provider')
    expect(trust).toContain('Vi har ikke verifisert at innhold som allerede er hentet inn, slettes ved frakobling')
    expect(trust).toContain('We have not verified that content already fetched is deleted when you disconnect')
  })

  it('rests on the broker refusing to revoke at Microsoft and on no consumer purging fetched content', () => {
    const root = repoRoot()
    const oauth = join(root, 'apps', 'Ingestion Plane', 'integration-corev2', 'internal', 'oauth')
    const clients = read(oauth, 'clients.go')
    const microsoft = /func \(c \*MicrosoftProviderClient\) Revoke\([^)]*\) error \{\s*return ErrRevocationUnsupported\s*\}/
    expect(clients).toMatch(microsoft)
    expect(read(oauth, 'service.go')).toContain('unsupported_provider_policy')
  })
})

describe('ZDR settings copy (docs/compliance/evidence/claims-vs-code.md, T2c)', () => {
  const settings = squash(read(here, 'WorkspaceSettingsPage.tsx'))
  const onboarding = squash(read(here, '..', '..', 'onboarding', 'components', 'steps', 'OrganizationStep.tsx'))

  it('no longer says ZDR content does not leave the service', () => {
    for (const text of [settings, onboarding]) {
      expect(text).not.toContain('forlater ikke tjenesten')
      expect(text).not.toContain('does not leave the service')
    }
  })

  it('says a ZDR request goes only to a provider with a recorded attestation and that the content still leaves Verevon', () => {
    expect(settings).toContain('sendes interaktive forespørsler bare til en leverandør vi har en registrert ZDR-godkjenning for')
    expect(settings).toContain('Innholdet forlater likevel Verevon og går til den leverandøren')
    expect(settings).toContain('The content still leaves Verevon and goes to that provider')
    expect(onboarding).toContain('sendes forespørsler bare til en leverandør med registrert ZDR-godkjenning')
    expect(onboarding).toContain('innholdet går fortsatt til den leverandøren')
  })

  it('says ZDR is plan-gated, and org-core enforces it', () => {
    expect(settings).toContain('Pro- og Enterprise-planer')
    const root = repoRoot()
    const org = join(root, 'apps', 'Control Plane', 'org-core', 'internal', 'org')
    expect(read(org, 'service_enhanced.go')).toContain('PlanAllowsZeroDataRetention(current.Plan)')
    expect(read(org, 'types.go')).toContain('var zeroDataRetentionPlans')
  })

  it('rests on inference-core refusing a ZDR request that no attested provider can serve', () => {
    const root = repoRoot()
    const fallback = read(root, 'apps', 'Model Plane', 'rust', 'services', 'inference-core', 'src', 'provider', 'fallback.rs')
    expect(fallback).toContain('if req.zdr && !caps.supports_zdr')
  })
})
