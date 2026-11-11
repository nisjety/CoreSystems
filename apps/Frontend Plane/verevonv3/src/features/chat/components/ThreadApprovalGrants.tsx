import { createMemo, createSignal, For, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  listApprovalGrants,
  revokeApprovalGrant,
  type ApprovalGrant,
} from '@/shared/api/orchestration-client'

/** A grant is live while it is not revoked and its expiry is still ahead. */
export function isLiveApprovalGrant(grant: ApprovalGrant, now: number = Date.now()): boolean {
  if (grant.revokedAt !== null) return false
  const expires = Date.parse(grant.expiresAt)
  return Number.isFinite(expires) && expires > now
}

/**
 * WS13-S6: the standing approvals that are live in this thread, each with a Revoke control. The
 * list is the caller's own, read through the BFF route; the SPA keeps nothing locally. A revoke
 * only narrows authority, so it is one click with no confirmation. Renders nothing when there is
 * no live grant (and nothing for a zero-retention principal, whose list the server answers empty).
 */
export function ThreadApprovalGrants(props: { threadId: string }) {
  const i18n = useI18n()
  const [grants, { refetch }] = createResource(
    () => props.threadId,
    async (threadId: string): Promise<ApprovalGrant[]> => listApprovalGrants(threadId),
  )
  const [revoking, setRevoking] = createSignal<string | null>(null)
  const [failure, setFailure] = createSignal<string | null>(null)
  const live = createMemo(() => (grants() ?? []).filter((grant) => isLiveApprovalGrant(grant)))

  const scopeLabel = (grant: ApprovalGrant) =>
    grant.scope === 'run'
      ? i18n.tr('For denne kjøringen', 'For this run')
      : i18n.tr('For denne tråden', 'For this thread')

  const expiryLabel = (grant: ApprovalGrant) => {
    const when = new Date(grant.expiresAt)
    let text = grant.expiresAt
    try {
      text = new Intl.DateTimeFormat(i18n.localeCode(), {
        dateStyle: 'medium',
        timeStyle: 'short',
      }).format(when)
    } catch {
      text = when.toLocaleString()
    }
    return i18n.tr(`Utløper ${text}`, `Expires ${text}`)
  }

  const fallback = {
    no: 'Kunne ikke hente eller trekke tilbake godkjenningen. Prøv igjen.',
    en: 'Could not load or revoke the approval. Try again.',
  }

  const revoke = async (grant: ApprovalGrant) => {
    if (revoking() !== null) return
    setRevoking(grant.id)
    setFailure(null)
    try {
      await revokeApprovalGrant(grant.id)
      await refetch()
    } catch (error) {
      setFailure(translateApiError(error, i18n.tr, fallback))
    } finally {
      setRevoking(null)
    }
  }

  return (
    <Show when={live().length > 0 || failure() !== null || grants.error != null}>
      <section class="verevon-chat-steps-tools" aria-label={i18n.tr('Aktive godkjenninger i denne tråden', 'Active approvals in this thread')}>
        <h3>{i18n.tr('Aktive godkjenninger i denne tråden', 'Active approvals in this thread')}</h3>
        <Show when={failure() ?? (grants.error != null ? translateApiError(grants.error, i18n.tr, fallback) : null)}>
          {(message) => <p role="alert">{message()}</p>}
        </Show>
        <ul>
          <For each={live()}>
            {(grant) => (
              <li>
                <strong>{grant.toolName}</strong>
                <span>{scopeLabel(grant)}</span>
                <time datetime={grant.expiresAt}>{expiryLabel(grant)}</time>
                <button type="button" disabled={revoking() !== null} onClick={() => void revoke(grant)}>
                  {i18n.tr('Trekk tilbake', 'Revoke')}
                </button>
              </li>
            )}
          </For>
        </ul>
      </section>
    </Show>
  )
}
