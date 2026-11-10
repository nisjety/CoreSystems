import { z } from 'zod'
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import { ApiError } from '@/shared/api/http'
import { getSession } from '@/shared/session/session-store'
import {
  BrowserPolicyRevisionConflict,
  type BrowserOriginConsent,
  type BrowserPolicyLoad,
  type BrowserPolicyPort,
  type BrowserPolicyProposal,
  type BrowserPolicyProposalResult,
} from '@/features/settings/components/BrowserPolicySection'

/**
 * Settings > Browser (E3.2-g1): the production port of BrowserPolicySection. It calls the action
 * ids of the contract (`browser.get_org_policy`, `browser.set_org_policy`) through executeAction, so
 * the gateway's flag, membership and role checks decide. A build that does not set
 * VITE_BROWSER_POLICY_SETTINGS to the literal `true` never mounts the section.
 *
 * The viewer's own per-origin consent rows (E3.2-e3) come from `browser.list_origin_consents`, and
 * one is forgotten through `browser.revoke_origin_consent`. A failed or refused list is empty, so the
 * section shows its empty state and never a made-up row.
 */

export function browserPolicySettingsEnabled(): boolean {
  return import.meta.env.VITE_BROWSER_POLICY_SETTINGS === 'true'
}

const documentShape = z.object({
  caps: z.record(z.string(), z.string()).optional(),
  robots_max: z.string().optional(),
  proxy_max: z.string().optional(),
  domains: z
    .object({ allow: z.array(z.string()).optional(), deny: z.array(z.string()).optional() })
    .optional(),
})

const loadShape = z.object({
  policy: documentShape.nullable(),
  effective: documentShape,
  revision: z.number().int().min(0),
  rev: z.string(),
  entitlements: z.record(z.string(), z.boolean()),
  can_manage: z.boolean(),
})

const saveShape = z.object({
  revision: z.number().int().min(0),
  rev: z.string(),
})

// The viewer's stored decisions, as browser.list_origin_consents answers them (E3.2-e3-reg).
const originConsentsShape = z.object({
  origins: z.array(
    z.object({
      startHost: z.string(),
      hosts: z.array(z.string()),
      decision: z.enum(['allow', 'deny']),
      scope: z.enum(['turn', 'conversation', 'always']),
      conversationId: z.string().optional(),
      turnId: z.string().optional(),
      decidedAt: z.string(),
    }),
  ),
})

export type BrowserPolicyActorSource = () => ActionActor | null

/** The viewer acting as themselves in the active organization, or null before the session has both. */
export function currentBrowserPolicyActor(): ActionActor | null {
  const session = getSession()
  if (!session.user || !session.activeOrg) return null
  return { type: 'human', userId: session.user.id, orgId: session.activeOrg.id }
}

/**
 * The viewer's own stored consent decisions. A refusal (the gateway's or user-core's switch off, or
 * their storage down), a failed call and an answer of the wrong shape are all an empty list, so no
 * row is ever made up.
 */
async function listOriginConsents(actor: ActionActor): Promise<BrowserOriginConsent[]> {
  try {
    const execution = await executeAction('browser.list_origin_consents', actor, {})
    const parsed = originConsentsShape.safeParse(execution.result)
    return parsed.success ? parsed.data.origins : []
  } catch {
    return []
  }
}

/** The revoke input for one stored decision: its key, with each id only when the decision has one. */
function originConsentKey(consent: BrowserOriginConsent) {
  return {
    startHost: consent.startHost,
    scope: consent.scope,
    ...(consent.conversationId !== undefined ? { conversationId: consent.conversationId } : {}),
    ...(consent.turnId !== undefined ? { turnId: consent.turnId } : {}),
  }
}

export function createBrowserPolicyPort(
  actorOf: BrowserPolicyActorSource = currentBrowserPolicyActor,
): BrowserPolicyPort {
  return {
    async load(orgId: string): Promise<BrowserPolicyLoad> {
      const actor = actorOf()
      if (!actor) return { available: false }
      // The two reads are independent, so they run together; the consent list never rejects.
      const [execution, originConsents] = await Promise.all([
        executeAction('browser.get_org_policy', actor, { orgId }),
        listOriginConsents(actor),
      ])
      const parsed = loadShape.safeParse(execution.result)
      if (!parsed.success) return { available: false }
      const data = parsed.data
      return {
        available: true,
        revision: data.revision,
        canManage: data.can_manage,
        document: data.policy,
        effective: data.effective,
        entitlements: data.entitlements,
        originConsents,
      }
    },
    async propose(orgId: string, proposal: BrowserPolicyProposal): Promise<BrowserPolicyProposalResult> {
      const actor = actorOf()
      if (!actor) throw new Error('Browser policy changes are not available yet.')
      try {
        const execution = await executeAction('browser.set_org_policy', actor, {
          orgId,
          ifMatch: proposal.ifMatch,
          document: proposal.document,
        })
        if (execution.status === 'waiting_approval') {
          return { status: 'waiting_approval', runId: execution.runId }
        }
        const saved = saveShape.parse(execution.result)
        return { status: 'applied', revision: saved.revision }
      } catch (err) {
        if (err instanceof ApiError && err.status === 409 && err.code === 'revision_conflict') {
          throw new BrowserPolicyRevisionConflict()
        }
        throw err
      }
    },
    async revokeOriginConsent(consent: BrowserOriginConsent): Promise<void> {
      const actor = actorOf()
      if (!actor) throw new Error('Browser policy changes are not available yet.')
      await executeAction('browser.revoke_origin_consent', actor, originConsentKey(consent))
    },
  }
}
