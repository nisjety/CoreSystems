import { For, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import type { ApprovalRuleProposal } from '@/shared/api/orchestration-client'

/**
 * WS13-S7: the rules an "Always" choice on an MCP tool's approval proposed, for the person to confirm
 * here or dismiss (build plan WS13 S7). A proposal is not a rule: nothing applies until the person
 * confirms it, and confirming writes a digest-pinned allow that lapses in ninety days (K3-10). The
 * card shows what was approved; the parent owns the list and performs the write.
 */
export function ApprovalRuleProposals(props: {
  proposals: ApprovalRuleProposal[]
  onConfirm: (proposal: ApprovalRuleProposal) => void
  onDismiss: (proposal: ApprovalRuleProposal) => void
}) {
  const i18n = useI18n()
  return (
    <Show when={props.proposals.length > 0}>
      <section aria-labelledby="approval-rule-proposals">
        <SectionHeader
          title={i18n.tr('Regler foreslått fra godkjenninger', 'Rules proposed from approvals')}
          description={i18n.tr(
            'Du valgte «Alltid» for et verktøy. Regelen gjelder ikke før du bekrefter den, og den utløper etter 90 dager.',
            'You chose "Always" for a tool. The rule does not apply until you confirm it, and it lapses after 90 days.',
          )}
        />
        <ul>
          <For each={props.proposals}>
            {(proposal) => (
              <li>
                <span>{proposal.toolName}</span>
                <SettingsButton type="button" variant="primary" onClick={() => props.onConfirm(proposal)}>
                  {i18n.tr('Bekreft regel', 'Confirm rule')}
                </SettingsButton>
                <SettingsButton type="button" onClick={() => props.onDismiss(proposal)}>
                  {i18n.tr('Avvis', 'Dismiss')}
                </SettingsButton>
              </li>
            )}
          </For>
        </ul>
      </section>
    </Show>
  )
}
