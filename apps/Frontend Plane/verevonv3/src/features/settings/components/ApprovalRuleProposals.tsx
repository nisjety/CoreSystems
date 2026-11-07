import { For, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import type { ApprovalRuleProposal } from '@/shared/api/orchestration-client'

/**
 * EP-9 E4: a proposal from a shell approval names a narrow rule, a program and its subcommand such
 * as `git status`, which the server reads from closed tables and never from the command's
 * arguments. The card names that rule; a proposal without one names its tool.
 */
export type RuleProposalCard = ApprovalRuleProposal & { ruleText?: string }

/** What the card names for a proposal: its rule when it has one, else its tool. */
export function proposalLabel(proposal: RuleProposalCard): string {
  const rule = proposal.ruleText?.trim()
  return rule ? rule : proposal.toolName
}

/**
 * WS13-S7: the rules an "Always" choice on an MCP tool's approval proposed, for the person to confirm
 * here or dismiss (build plan WS13 S7). A proposal is not a rule: nothing applies until the person
 * confirms it, and confirming writes a digest-pinned allow that lapses in ninety days (K3-10). The
 * card shows what was approved; the parent owns the list and performs the write.
 */
export function ApprovalRuleProposals(props: {
  proposals: RuleProposalCard[]
  onConfirm: (proposal: RuleProposalCard) => void
  onDismiss: (proposal: RuleProposalCard) => void
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
                <span>{proposalLabel(proposal)}</span>
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
