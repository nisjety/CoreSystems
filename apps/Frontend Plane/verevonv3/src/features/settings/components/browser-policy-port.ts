import { z } from 'zod'
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import { ApiError } from '@/shared/api/http'
import { getSession } from '@/shared/session/session-store'
import {
  BrowserPolicyRevisionConflict,
  type BrowserPolicyLoad,
  type BrowserPolicyPort,
  type BrowserPolicyProposal,
  type BrowserPolicyProposalResult,
} from '@/features/settings/components/BrowserPolicySection'

/**
 * Settings > Browser (E3.2-g1): the production port of BrowserPolicySection. It calls the two
 * action ids of the contract (`browser.get_org_policy`, `browser.set_org_policy`) through
 * executeAction, so the gateway's flag, membership and role checks decide. A build that does not
 * set VITE_BROWSER_POLICY_SETTINGS to the literal `true` never mounts the section.
 *
 * The viewer's per-origin consent rows are not loaded yet (no gateway route, E3.2-e3), so
 * `originConsents` is always empty here.
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

export type BrowserPolicyActorSource = () => ActionActor | null

/** The viewer acting as themselves in the active organization, or null before the session has both. */
export function currentBrowserPolicyActor(): ActionActor | null {
  const session = getSession()
  if (!session.user || !session.activeOrg) return null
  return { type: 'human', userId: session.user.id, orgId: session.activeOrg.id }
}

export function createBrowserPolicyPort(
  actorOf: BrowserPolicyActorSource = currentBrowserPolicyActor,
): BrowserPolicyPort {
  return {
    async load(orgId: string): Promise<BrowserPolicyLoad> {
      const actor = actorOf()
      if (!actor) return { available: false }
      const execution = await executeAction('browser.get_org_policy', actor, { orgId })
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
        originConsents: [],
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
  }
}
