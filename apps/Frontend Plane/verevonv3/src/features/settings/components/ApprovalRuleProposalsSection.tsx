import { createSignal, Show } from 'solid-js'
import { translateApiError, useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import { ApiError } from '@/shared/api/http'
import {
  confirmApprovalRule,
  dismissApprovalRule,
  listApprovalRuleProposals,
  type ApprovalRuleProposal,
} from '@/shared/api/orchestration-client'
import { ApprovalRuleProposals } from '@/features/settings/components/ApprovalRuleProposals'

/** The typed refusals of a confirm or dismiss (WS13 S7) that get their own line. */
export type RuleProposalFailure = 'stale' | 'busy' | 'incomplete'

/**
 * The refusal an error carries, or null for any other failure. A 409 that is not `rule_proposal_busy`
 * is the tool having changed since the person approved it: `rule_proposal_stale` from the server, or
 * capability-core's own digest refusal, which says the same thing.
 */
export function ruleProposalFailureOf(error: unknown): RuleProposalFailure | null {
  if (!(error instanceof ApiError)) return null
  if (error.code === 'rule_proposal_busy') return 'busy'
  if (error.code === 'rule_confirm_incomplete' && error.status === 503) return 'incomplete'
  if (error.code === 'rule_proposal_stale' || error.code === 'tool_definition_changed') return 'stale'
  if (error.status === 409) return 'stale'
  return null
}

/**
 * WS13-S7: the settings section behind VITE_APPROVAL_RULE_PROPOSALS. It lists the caller's open rule
 * proposals and confirms or dismisses one by its id; the server forms the rule from its own record, so
 * nothing about the rule is chosen or kept here. The list is empty for a zero-retention principal and
 * the section then renders nothing.
 */
export function ApprovalRuleProposalsSection() {
  const i18n = useI18n()
  const [proposals, { refetch }] = createResource(() => listApprovalRuleProposals())
  const [busy, setBusy] = createSignal(false)
  const [failure, setFailure] = createSignal<string | null>(null)

  const failureLine = (error: unknown): string => {
    switch (ruleProposalFailureOf(error)) {
      case 'stale':
        return i18n.tr(
          'Verktøyet er endret siden du godkjente det. Godkjenn det på nytt i en samtale.',
          'The tool changed since you approved it. Approve it again in a conversation.',
        )
      case 'busy':
        return i18n.tr(
          'Regelen bekreftes akkurat nå. Prøv igjen om et minutt.',
          'This rule is being confirmed right now. Try again in a minute.',
        )
      case 'incomplete':
        return i18n.tr('Bekreftelsen ble ikke fullført. Prøv igjen.', 'The confirmation did not finish. Try again.')
      default:
        return translateApiError(error, i18n.tr, {
          no: 'Kunne ikke oppdatere regelforslaget. Prøv igjen.',
          en: 'Could not update the rule proposal. Try again.',
        })
    }
  }

  const act = async (action: (proposal: ApprovalRuleProposal) => Promise<void>, proposal: ApprovalRuleProposal) => {
    if (busy()) return
    setBusy(true)
    setFailure(null)
    try {
      await action(proposal)
    } catch (error) {
      setFailure(failureLine(error))
    }
    try {
      // The list is re-read either way: a stale or dismissed proposal should leave it, a busy one stays.
      await refetch()
    } catch (error) {
      setFailure((current) => current ?? failureLine(error))
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <Show when={failure()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show when={proposals.error != null && failure() === null}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {translateApiError(proposals.error, i18n.tr, {
            no: 'Kunne ikke laste regelforslagene.',
            en: 'Could not load the rule proposals.',
          })}
        </p>
      </Show>
      <ApprovalRuleProposals
        proposals={proposals() ?? []}
        onConfirm={(proposal) => void act(confirmApprovalRule, proposal)}
        onDismiss={(proposal) => void act(dismissApprovalRule, proposal)}
      />
    </>
  )
}
