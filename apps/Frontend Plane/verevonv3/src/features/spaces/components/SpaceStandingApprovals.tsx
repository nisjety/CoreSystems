import { createSignal, For, Show } from 'solid-js'

import { ApiError } from '@/shared/api/http'
import {
  listSpaceStandingApprovals,
  revokeSpaceStandingApproval,
  type SpaceStandingApproval,
} from '@/shared/api/space-approvals-client'
import { announce } from '@/shared/a11y/announcer'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'

/**
 * The room's standing approvals, in the Agent tab (build plan WS10 S4).
 *
 * A standing rule lets an agent use one MCP tool without asking again. It is Control's record:
 * this list reads it and revokes it, and nothing here can create or widen one. Managers and
 * owners see the list; the server enforces that, and the section simply stays out of the way
 * for everyone else, and when standing approvals are switched off (`feature_disabled`).
 */
export interface SpaceStandingApprovalsProps {
  readonly spaceRef: string
  /** Presentation only: managers and owners. Control decides. */
  readonly canGovern: boolean
}

function absent(error: unknown): boolean {
  // Flag off, or a role Control does not let list rules: no section, not an error banner.
  return error instanceof ApiError && (error.code === 'feature_disabled' || error.status === 403)
}

export function SpaceStandingApprovals(props: SpaceStandingApprovalsProps) {
  const i18n = useI18n()
  const [rules, { refetch }] = createResource(
    () => (props.canGovern ? props.spaceRef : undefined),
    (spaceRef) => listSpaceStandingApprovals(spaceRef),
  )
  const [revokingId, setRevokingId] = createSignal<string | undefined>(undefined)
  const [failed, setFailed] = createSignal(false)

  async function revoke(rule: SpaceStandingApproval) {
    if (revokingId()) return
    setRevokingId(rule.ruleId)
    setFailed(false)
    try {
      await revokeSpaceStandingApproval(props.spaceRef, rule.ruleId)
      announce(i18n.tr('Fast godkjenning trukket tilbake', 'Standing approval revoked'), {
        politeness: 'polite',
        key: `standing-approval-revoked:${rule.ruleId}`,
      })
    } catch {
      setFailed(true)
    } finally {
      setRevokingId(undefined)
      // Re-read either way: the list shows what Control holds, never what we expect.
      await refetch().catch(() => undefined)
    }
  }

  const visible = () => props.canGovern && !(rules.error && absent(rules.error))

  return (
    <Show when={visible()}>
      <section class="verevon-space-standing-approvals" aria-labelledby={`standing-approvals-${props.spaceRef}`}>
        <h3 id={`standing-approvals-${props.spaceRef}`}>
          {i18n.tr('Faste godkjenninger', 'Standing approvals')}
        </h3>
        <p class="verevon-room-approval__note">
          {i18n.tr(
            'Verktøy en agent kan bruke uten å spørre hver gang. Trekk en regel tilbake, så spør agenten igjen neste gang.',
            'Tools an agent may use without asking each time. Revoke a rule and the agent asks again next time.',
          )}
        </p>
        <Show when={rules.loading}>
          <p class="verevon-space-inline-status" role="status">
            {i18n.tr('Henter faste godkjenninger …', 'Loading standing approvals…')}
          </p>
        </Show>
        <Show when={rules.error}>
          <p class="verevon-space-projection-error" role="alert">
            {i18n.tr(
              'Vi fikk ikke hentet de faste godkjenningene. Ingenting er endret.',
              'We could not load the standing approvals. Nothing was changed.',
            )}
          </p>
        </Show>
        <Show when={failed()}>
          <p class="verevon-space-projection-error" role="alert">
            {i18n.tr(
              'Vi fikk ikke trukket regelen tilbake. Sjekk listen før du prøver igjen.',
              'We could not revoke the rule. Check the list before trying again.',
            )}
          </p>
        </Show>
        <Show when={!rules.loading && !rules.error && (rules() ?? []).length === 0}>
          <p class="verevon-room-approval__note">
            {i18n.tr('Ingen faste godkjenninger i dette rommet.', 'No standing approvals in this room.')}
          </p>
        </Show>
        <ul class="verevon-room-approval__list">
          <For each={rules.error ? [] : rules() ?? []}>
            {(rule) => (
              <li class="verevon-room-approval__item">
                <p class="verevon-room-approval__detail">{rule.toolKey}</p>
                <p class="verevon-room-approval__requester">
                  {i18n.tr('Gjennomgås senest ', 'Review by ')}
                  {rule.reviewBy.slice(0, 10)}
                  {' · '}
                  {i18n.tr('brukt ', 'used ')}
                  {rule.useCount}
                  {i18n.tr(' ganger', ' times')}
                </p>
                <div class="verevon-room-approval__actions">
                  <button
                    type="button"
                    class="verevon-room-approval__deny"
                    disabled={revokingId() !== undefined}
                    onClick={() => void revoke(rule)}
                  >
                    {revokingId() === rule.ruleId
                      ? i18n.tr('Trekker tilbake …', 'Revoking…')
                      : i18n.tr('Trekk tilbake', 'Revoke')}
                  </button>
                </div>
              </li>
            )}
          </For>
        </ul>
      </section>
    </Show>
  )
}
