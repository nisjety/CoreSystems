import { Check, Sparkles, X } from 'lucide-solid'
import { createEffect, createMemo, createSignal, For, onCleanup, Show } from 'solid-js'
import {
  listAiActions,
  reviewAiAction,
  type AiAction,
  type AiActionFieldEdits,
} from '@/shared/api/inbox-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import type { TicketTeam } from '@/shared/api/tickets-client'

const EDITABLE_FIELD_KEYS = ['category', 'intent', 'work_type', 'priority', 'severity', 'status', 'title', 'customer_impact', 'summary', 'root_cause'] as const
type EditableFieldKey = (typeof EDITABLE_FIELD_KEYS)[number]

function fieldLabel(key: EditableFieldKey, tr: (noText: string, enText: string) => string): string {
  switch (key) {
    case 'category':
      return tr('Kategori', 'Category')
    case 'intent':
      return tr('Hensikt', 'Intent')
    case 'work_type':
      return tr('Arbeidstype', 'Work type')
    case 'priority':
      return tr('Prioritet', 'Priority')
    case 'severity':
      return tr('Alvorlighetsgrad', 'Severity')
    case 'status':
      return tr('Status', 'Status')
    case 'title':
      return tr('Hendelsestittel', 'Incident title')
    case 'customer_impact':
      return tr('Kundepåvirkning', 'Customer impact')
    case 'summary':
      return tr('Sammendrag', 'Summary')
    case 'root_cause':
      return tr('Rotårsak', 'Root cause')
  }
}

function isTicketClassification(action: AiAction): boolean {
  return action.kind === 'ticket_classification' || action.kind === 'ticket.classification'
}

function isTicketUpdate(action: AiAction): boolean {
  return action.kind === 'ticket.update'
}

function isIncidentCreate(action: AiAction): boolean {
  return action.kind === 'incident.create'
}

function isProblemCreate(action: AiAction): boolean {
  return action.kind === 'problem.create'
}

function isTicketFieldProposal(action: AiAction): boolean {
  return isTicketClassification(action) || isTicketUpdate(action)
}

function isDraftReply(action: AiAction): boolean {
  return action.kind === 'draft.reply'
}

function isInternalNote(action: AiAction): boolean {
  return action.kind === 'internal.note'
}

/** A draft.reply is an approval-bound external effect. Render the exact plain
 * text payload the executor will send, never a model summary or an HTML
 * rendering, so the human review screen is the real approval surface. */
function draftReplyBody(action: AiAction): string | null {
  const body = action.payload?.body_text
  return typeof body === 'string' && body.trim() ? body.trim() : null
}

function isAwaitingReview(action: AiAction): boolean {
  return action.status === 'suggested' || action.status === 'suggest_ticket'
}

function isVerifiedExecution(action: AiAction): boolean {
  return action.status === 'executed'
}

function executionReceipt(
  action: AiAction,
  tr: (noText: string, enText: string) => string,
): string {
  if (isTicketClassification(action)) {
    return tr(
      'Utførelse verifisert — support-saken er åpnet og rutet.',
      'Execution verified — the support ticket is open and routed.',
    )
  }
  return tr('Utførelse verifisert i handlingsloggen.', 'Execution verified in the action ledger.')
}

/** conversation-core stores the AI's suggested ticket fields under
 * `suggested_fields` (RecordTicketClassification). `proposed_ticket` is kept
 * as a forward-compat fallback. */
function suggestedFieldsOf(action: AiAction): Record<string, unknown> {
  const payload = action.payload ?? {}
  return (payload.suggested_fields ?? payload.proposed_ticket ?? {}) as Record<string, unknown>
}

function confidenceOf(action: AiAction): number | null {
  const payload = action.payload ?? {}
  return typeof payload.confidence === 'number' ? Math.round(payload.confidence * 100) : null
}

function classificationReasonOf(action: AiAction): string | null {
  if (!isTicketFieldProposal(action) && !isIncidentCreate(action) && !isProblemCreate(action)) return null
  const reason = action.payload?.reason
  return typeof reason === 'string' && reason.trim() ? reason.trim() : null
}

function evidenceMessageIdsOf(action: AiAction): string[] {
  if (!isTicketFieldProposal(action) && !isIncidentCreate(action) && !isProblemCreate(action)) return []
  const raw = action.payload?.evidence_message_ids
  if (!Array.isArray(raw)) return []
  return raw
    .filter((value): value is string => typeof value === 'string' && value.trim() !== '')
    .map((value) => value.trim())
    .slice(0, 25)
}

/** Only the fields the upstream actually suggested as non-empty strings —
 * never fabricates an editable input for a field the AI never proposed. */
function editableKeysOf(action: AiAction): EditableFieldKey[] {
  if (isIncidentCreate(action)) {
    return EDITABLE_FIELD_KEYS.filter((key) => key === 'title' || key === 'severity' || key === 'customer_impact').filter((key) => {
      const value = action.payload?.[key]
      return typeof value === 'string' && value.trim() !== ''
    })
  }
  if (isProblemCreate(action)) {
    return EDITABLE_FIELD_KEYS.filter((key) => key === 'title' || key === 'summary' || key === 'root_cause').filter((key) => {
      const value = action.payload?.[key]
      return typeof value === 'string'
    })
  }
  const suggested = suggestedFieldsOf(action)
  return EDITABLE_FIELD_KEYS.filter((key) => {
    const value = suggested[key]
    return typeof value === 'string' && value.trim() !== ''
  })
}

function teamProposalOf(action: AiAction): { id: string; name: string } | null {
  const suggested = suggestedFieldsOf(action)
  const id = suggested.team_id
  const name = suggested.team_name
  return typeof id === 'string' && id.trim() && typeof name === 'string' && name.trim()
    ? { id: id.trim(), name: name.trim() }
    : null
}

function canonicalTeamFor(action: AiAction, teams: readonly TicketTeam[]): TicketTeam | null {
  const proposed = teamProposalOf(action)
  if (!proposed) return null
  return teams.find((team) => team.active && team.id === proposed.id && team.name === proposed.name) ?? null
}

function canonicalTeamForFields(fields: AiActionFieldEdits, teams: readonly TicketTeam[]): TicketTeam | null {
  const id = fields.team_id?.trim()
  const name = fields.team_name?.trim()
  if (!id || !name) return null
  return teams.find((team) => team.active && team.id === id && team.name === name) ?? null
}

function defaultEditForAction(action: AiAction, teams: readonly TicketTeam[]): AiActionFieldEdits {
  if (isDraftReply(action) || isInternalNote(action)) {
    const bodyText = draftReplyBody(action)
    return bodyText ? { body_text: bodyText } : {}
  }
  if (isIncidentCreate(action)) {
    return {
      title: typeof action.payload?.title === 'string' ? action.payload.title : '',
      severity: typeof action.payload?.severity === 'string' ? action.payload.severity : '',
      customer_impact: typeof action.payload?.customer_impact === 'string' ? action.payload.customer_impact : '',
    }
  }
  if (isProblemCreate(action)) {
    return {
      title: typeof action.payload?.title === 'string' ? action.payload.title : '',
      summary: typeof action.payload?.summary === 'string' ? action.payload.summary : '',
      root_cause: typeof action.payload?.root_cause === 'string' ? action.payload.root_cause : '',
    }
  }
  const suggested = suggestedFieldsOf(action)
  const edits: AiActionFieldEdits = {}
  for (const key of editableKeysOf(action)) {
    if (key === 'work_type') {
      const workType = suggested[key]
      if (workType === 'customer_case' || workType === 'internal_work' || workType === 'incident') {
        edits.work_type = workType
      }
      continue
    }
		if (key === 'status') {
			const status = suggested[key]
			if (status === 'open' || status === 'waiting_customer' || status === 'waiting_team' || status === 'escalated') {
				edits.status = status
			}
			continue
		}
    edits[key] = String(suggested[key])
  }
	const canonicalTeam = canonicalTeamFor(action, teams)
	if (canonicalTeam) {
		edits.team_id = canonicalTeam.id
		edits.team_name = canonicalTeam.name
	}
  return edits
}

/** Label + detail for kinds with no suggested ticket fields. */
function summarizeOther(
  action: AiAction,
  tr: (noText: string, enText: string) => string,
): { label: string; detail: string } {
  const label = action.kind.replace(/[._]/g, ' ')
  const confidence = confidenceOf(action)
  const detail = confidence !== null ? tr(`${confidence}% konfidens`, `${confidence}% confidence`) : ''
  return { label, detail }
}

function actionDisplayLabel(
  action: AiAction,
  tr: (noText: string, enText: string) => string,
): string {
  if (isTicketClassification(action)) return tr('Support-sak', 'Support ticket')
  if (isTicketUpdate(action)) return tr('Saksoppdatering', 'Ticket update')
  if (isIncidentCreate(action)) return tr('Hendelse', 'Incident')
  if (isProblemCreate(action)) return tr('Problem', 'Problem')
  return summarizeOther(action, tr).label
}

/** HITL review queue for a single conversation: model-proposed actions awaiting a
 * human decision. Renders real items from conversation-core, an explicit empty
 * state when there are none, and lets the reviewer edit the AI's suggested ticket
 * fields inline before approving — nothing executes until that explicit approve,
 * and the (possibly edited) fields travel with the decision. Execution is
 * asynchronous, so the UI never equates recorded approval with verified work. */
export function AiActionReviewPanel(props: {
  conversationId: string | undefined
  refreshKey?: number
  onTicketActionVerified?: () => void
  ticketTeams?: readonly TicketTeam[]
}) {
  const i18n = useI18n()
  const [loadError, setLoadError] = createSignal<unknown>(null)
  const [actions, setActions] = createSignal<AiAction[]>([])
  const [actionsLoading, setActionsLoading] = createSignal(false)
  let disposed = false
  onCleanup(() => { disposed = true })

  /**
   * A review ledger must always settle into either content, an honest error, or
   * an empty state. Detail hydration may re-run this loader for the same
   * conversation; matching the captured id keeps a late response for a former
   * selection from changing the current panel without leaving it loading.
   */
  async function loadActions(conversationId: string | undefined): Promise<AiAction[]> {
    if (!conversationId) {
      setActions([])
      setLoadError(null)
      setActionsLoading(false)
      return []
    }

    setActionsLoading(true)
    setLoadError(null)
    try {
      const loaded = await listAiActions({ conversationId, status: 'all' })
      if (!disposed) setActions(loaded)
      return loaded
    } catch (err) {
      if (!disposed) {
        setActions([])
        setLoadError(err)
      }
      return []
    } finally {
      if (!disposed) setActionsLoading(false)
    }
  }

  createEffect(() => {
    const conversationId = props.conversationId
    void (props.refreshKey ?? 0)
    void loadActions(conversationId)
  })

  const pendingActions = createMemo(() => actions().filter(isAwaitingReview))
  const proposalGroups = createMemo(() => {
    const grouped = new Map<string, AiAction[]>()
    for (const action of pendingActions()) {
      const groupId = action.proposal_group_id?.trim()
      if (!groupId) continue
      const members = grouped.get(groupId) ?? []
      members.push(action)
      grouped.set(groupId, members)
    }
    return Array.from(grouped.entries(), ([id, members]) => ({ id, members }))
  })
  const verifiedActions = createMemo(() => actions().filter(isVerifiedExecution))
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [recorded, setRecorded] = createSignal<string | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  const [edits, setEdits] = createSignal<Record<string, AiActionFieldEdits>>({})

  function fieldsFor(action: AiAction): AiActionFieldEdits {
    return edits()[action.id] ?? defaultEditForAction(action, props.ticketTeams ?? [])
  }

  function setField(action: AiAction, key: EditableFieldKey, value: string) {
    setEdits((prev) => ({ ...prev, [action.id]: { ...fieldsFor(action), [key]: value } }))
  }

  function setReviewBody(action: AiAction, bodyText: string) {
    setEdits((previous) => ({
      ...previous,
      [action.id]: { ...fieldsFor(action), body_text: bodyText },
    }))
  }

  function reviewBodyFor(action: AiAction): string | null {
    const bodyText = fieldsFor(action).body_text ?? draftReplyBody(action)
    return typeof bodyText === 'string' && bodyText.trim() ? bodyText : null
  }

	function setTeam(action: AiAction, team: TicketTeam | undefined) {
		if (!team) return
		setEdits((previous) => ({
			...previous,
			[action.id]: { ...fieldsFor(action), team_id: team.id, team_name: team.name },
		}))
	}

  function applyDecisionOutcome(decision: 'approve' | 'reject', latest: AiAction | undefined) {
    if (decision === 'approve' && latest && isVerifiedExecution(latest)) {
      if (isTicketFieldProposal(latest)) props.onTicketActionVerified?.()
      setRecorded(executionReceipt(latest, i18n.tr))
    } else {
      setRecorded(
        decision === 'approve'
          ? i18n.tr('Godkjenningen er registrert. Venter på verifisert utførelse.', 'Approval recorded. Waiting for verified execution.')
          : i18n.tr('Avgjørelse registrert — forslaget ble avvist.', 'Decision recorded — suggestion dismissed.'),
      )
    }
  }

  async function decide(action: AiAction, decision: 'approve' | 'reject') {
		const conversationId = props.conversationId
		if (decision === 'approve' && isTicketFieldProposal(action) && teamProposalOf(action) && !canonicalTeamForFields(fieldsFor(action), props.ticketTeams ?? [])) {
			setError(i18n.tr(
				'Velg et aktivt, kanonisk Ticketing-team før godkjenning.',
				'Select an active canonical Ticketing team before approval.',
			))
			return
		}
    setError(null)
    setRecorded(null)
    setBusyId(action.id)
    try {
      // Reject stays a bare decision — only an approve carries the (possibly
      // edited) ticket fields or exact draft/note body through for execution.
      const editedFields =
        decision === 'approve' && (isTicketFieldProposal(action) || isIncidentCreate(action) || isProblemCreate(action) || isDraftReply(action) || isInternalNote(action))
          ? fieldsFor(action)
          : undefined
      await reviewAiAction(action.id, decision, { editedFields })
      const latestActions = await loadActions(conversationId)
      applyDecisionOutcome(decision, latestActions?.find((item) => item.id === action.id))
    } catch (err) {
      // reviewAiAction can fail (e.g. a transient 502) after the decision was
      // already recorded server-side. Re-fetch and check the action's real
      // status before asserting failure, instead of trusting the network
      // error alone — otherwise a reviewer sees a false "could not record"
      // banner for a decision that already went through.
      const latest = (await loadActions(conversationId))?.find((item) => item.id === action.id)
      if (latest && !isAwaitingReview(latest)) {
        applyDecisionOutcome(decision, latest)
      } else if (latest) {
        setError(translateApiError(err, i18n.tr, { no: 'Kunne ikke registrere avgjørelsen. Prøv igjen.', en: 'Could not record the decision. Please retry.' }))
      } else {
        setError(i18n.tr(
          'Vi fikk ikke bekreftet om avgjørelsen ble registrert. Vent litt før du prøver på nytt.',
          "We couldn't confirm whether the decision went through. Please wait a moment before trying again.",
        ))
      }
    } finally {
      setBusyId(null)
    }
  }

  return (
    <Show when={props.conversationId}>
      <section
        class="verevon-ai-review"
        aria-busy={actionsLoading()}
        aria-label={i18n.tr('AI-forslag som venter på gjennomgang', 'AI suggestions awaiting review')}
      >
        <header class="verevon-ai-review__head">
          <Sparkles size={14} aria-hidden="true" />
          <span>{i18n.tr('AI-forslag', 'AI suggestions')}</span>
        </header>

        <Show
          when={!actionsLoading()}
          fallback={<p class="verevon-ai-review__muted">{i18n.tr('Laster forslag …', 'Loading suggestions…')}</p>}
        >
          <Show
            when={!loadError()}
            fallback={
              <p class="verevon-ai-review__error" role="alert">
                {translateApiError(loadError(), i18n.tr, {
                  no: 'Kunne ikke laste AI-forslagene. Prøv igjen.',
                  en: 'Could not load AI suggestions. Please retry.',
                })}
              </p>
            }
          >
            <Show
              when={pendingActions().length > 0}
              fallback={<p class="verevon-ai-review__muted">{i18n.tr('Ingen AI-forslag venter på gjennomgang for denne samtalen.', 'No AI suggestions awaiting review for this conversation.')}</p>}
            >
              <ul class="verevon-ai-review__list">
                <For each={pendingActions()}>
                  {(action) => {
                    const editableKeys = editableKeysOf(action)
                    const confidence = confidenceOf(action)
                    const reason = classificationReasonOf(action)
                    const evidenceMessageIds = evidenceMessageIdsOf(action)
							const teamProposal = teamProposalOf(action)
							const canonicalTeam = canonicalTeamFor(action, props.ticketTeams ?? [])
							const availableTeams = (props.ticketTeams ?? []).filter((team) => team.active)
                    const other = summarizeOther(action, i18n.tr)
                    const proposalGroup = action.proposal_group_id
                      ? proposalGroups().find((group) => group.id === action.proposal_group_id)
                      : undefined
                    const isProposalGroupStart = proposalGroup?.members[0]?.id === action.id
                    return (
                      <>
                        <Show when={isProposalGroupStart && proposalGroup}>
                          <li
                            class="verevon-ai-review__plan"
                            data-ai-proposal-group={proposalGroup?.id}
                            role="group"
                            aria-label={i18n.tr('AI-løsningsplan', 'AI resolution plan')}
                          >
                            <div>
                              <strong>{i18n.tr('AI-løsningsplan', 'AI resolution plan')}</strong>
                              <span>
                                {proposalGroup?.members.length === 1
                                  ? i18n.tr('1 separat avgjørelse', '1 independent decision')
                                  : i18n.tr(`${proposalGroup?.members.length} separate avgjørelser`, `${proposalGroup?.members.length} independent decisions`)}
                              </span>
                            </div>
                            <p>
                              {i18n.tr(
                                `Forslagene hører sammen, men hver avgjørelse må gjennomgås separat. Det finnes ingen godkjenn-alt-handling. ${proposalGroup?.members.map((member) => actionDisplayLabel(member, i18n.tr)).join(' · ')}`,
                                `These proposals are related, but each decision must be reviewed separately. There is no approve-all action. ${proposalGroup?.members.map((member) => actionDisplayLabel(member, i18n.tr)).join(' · ')}`,
                              )}
                            </p>
                          </li>
                        </Show>
                        <li class="verevon-ai-review__item" data-ai-action-id={action.id}>
                        <div class="verevon-ai-review__body">
                          <span class="verevon-ai-review__kind">
                            {isTicketClassification(action)
                              ? i18n.tr('Foreslått support-sak', 'Proposed support ticket')
                              : isTicketUpdate(action)
                                ? i18n.tr('Foreslått saksoppdatering', 'Proposed ticket update')
                                : isIncidentCreate(action)
                                  ? i18n.tr('Foreslått hendelse', 'Proposed incident')
                                  : isProblemCreate(action)
                                    ? i18n.tr('Foreslått problem', 'Proposed Problem')
                                : other.label}
                          </span>
                          <Show when={action.proposal_group_id}>
                            <span class="verevon-ai-review__detail">
                              {i18n.tr(
                                'Del av én løsningsplan — gjennomgå og avgjør dette forslaget separat.',
                                'Part of one resolution plan — review and decide this proposal separately.',
                              )}
                            </span>
                          </Show>
                          <Show
                            when={editableKeys.length > 0}
                            fallback={
                              <Show when={other.detail}>
                                <span class="verevon-ai-review__detail">{other.detail}</span>
                              </Show>
                            }
                          >
                            <div class="verevon-ai-review__fields">
                              <For each={editableKeys}>
                                {(key) => (
                                  <label class="verevon-ai-review__field">
                                    <span class="verevon-ai-review__field-label">{fieldLabel(key, i18n.tr)}</span>
                                    <Show
                                      when={key === 'work_type' || key === 'status'}
                                      fallback={
                                        <input
                                          type="text"
                                          class="verevon-ai-review__field-input"
                                          aria-label={i18n.tr(`${fieldLabel(key, i18n.tr)} (redigerbar)`, `${fieldLabel(key, i18n.tr)} (editable)`)}
                                          value={fieldsFor(action)[key] ?? ''}
                                          disabled={busyId() !== null}
                                          onInput={(event) => setField(action, key, event.currentTarget.value)}
                                        />
                                      }
                                    >
                                      <select
                                        class="verevon-ai-review__field-input"
                                        aria-label={key === 'status'
                                          ? i18n.tr('Status (redigerbar)', 'Status (editable)')
                                          : i18n.tr('Arbeidstype (redigerbar)', 'Work type (editable)')}
                                        value={key === 'status' ? fieldsFor(action).status ?? 'open' : fieldsFor(action).work_type ?? 'customer_case'}
                                        disabled={busyId() !== null}
                                        onChange={(event) => setField(action, key, event.currentTarget.value)}
                                      >
                                        <Show when={key === 'status'} fallback={
                                          <>
                                            <option value="customer_case">{i18n.tr('Kundesak', 'Customer case')}</option>
                                            <option value="internal_work">{i18n.tr('Internt arbeid', 'Internal work')}</option>
                                            <option value="incident">{i18n.tr('Hendelse', 'Incident')}</option>
                                          </>
                                        }>
                                          <option value="open">{i18n.tr('Åpen', 'Open')}</option>
                                          <option value="waiting_customer">{i18n.tr('Venter på kunde', 'Waiting on customer')}</option>
                                          <option value="waiting_team">{i18n.tr('Venter på team', 'Waiting on team')}</option>
                                          <option value="escalated">{i18n.tr('Eskalert', 'Escalated')}</option>
                                        </Show>
                                      </select>
                                    </Show>
                                  </label>
                                )}
                              </For>
                            </div>
                            <Show when={confidence !== null}>
                              <span class="verevon-ai-review__detail">
                                {i18n.tr(
                                  `${confidence}% konfidens — rediger feltene over før du godkjenner hvis nødvendig`,
                                  `${confidence}% confidence — edit the fields above before approving if needed`,
                                )}
                              </span>
                            </Show>
                          </Show>
                          <Show when={reason}>
                            <div class="verevon-ai-review__rationale">
                              <span class="verevon-ai-review__field-label">{i18n.tr('Begrunnelse', 'Rationale')}</span>
                              <p>{reason}</p>
                            </div>
                          </Show>
							<Show when={teamProposal}>
								<label class="verevon-ai-review__field">
									<span class="verevon-ai-review__field-label">{i18n.tr('Team', 'Team')}</span>
									<Show when={!canonicalTeam}>
										<span class="verevon-ai-review__detail">{i18n.tr('Foreslått team er ikke aktivt i Ticketing-katalogen. Velg et aktivt team for å reparere forslaget.', 'The proposed team is not active in the Ticketing directory. Select an active team to repair the proposal.')}</span>
									</Show>
									<Show
										when={availableTeams.length > 0}
										fallback={<span class="verevon-ai-review__detail">{i18n.tr('Ingen aktive Ticketing-team er tilgjengelige for korrigering.', 'No active Ticketing teams are available to correct this proposal.')}</span>}
									>
										<select
											class="verevon-ai-review__field-input"
											aria-label={i18n.tr('Team (redigerbar)', 'Team (editable)')}
											value={fieldsFor(action).team_id ?? canonicalTeam?.id ?? ''}
											disabled={busyId() !== null}
											onChange={(event) => setTeam(action, availableTeams.find((team) => team.id === event.currentTarget.value))}
										>
											<Show when={!canonicalTeam}><option value="" disabled>{i18n.tr('Velg team', 'Select a team')}</option></Show>
											<For each={availableTeams}>{(team) => <option value={team.id}>{team.name}</option>}</For>
										</select>
									</Show>
								</label>
							</Show>
                          <Show when={evidenceMessageIds.length > 0}>
                            <span class="verevon-ai-review__detail">
                              {i18n.tr(
                                `Meldingsreferanser: ${evidenceMessageIds.join(', ')}`,
                                `Message references: ${evidenceMessageIds.join(', ')}`,
                              )}
                            </span>
                          </Show>
                          <Show when={isDraftReply(action) || isInternalNote(action)}>
                            <div class="verevon-ai-review__draft" aria-label={i18n.tr('Foreslått svar som skal gjennomgås', 'Proposed reply to review')}>
                              <span class="verevon-ai-review__field-label">{isInternalNote(action) ? i18n.tr('Nøyaktig internt notat', 'Exact internal note') : i18n.tr('Nøyaktig svarutkast', 'Exact reply draft')}</span>
                              <textarea
                                class="verevon-ai-review__field-input verevon-ai-review__draft-editor"
                                aria-label={isInternalNote(action)
                                  ? i18n.tr('Internt notat (redigerbar)', 'Internal note (editable)')
                                  : i18n.tr('Svarutkast (redigerbar)', 'Reply draft (editable)')}
                                value={fieldsFor(action).body_text ?? draftReplyBody(action) ?? ''}
                                maxLength={8000}
                                disabled={busyId() !== null}
                                onInput={(event) => setReviewBody(action, event.currentTarget.value)}
                              />
                              <span class="verevon-ai-review__detail">
                                {i18n.tr(
                                  'Gå gjennom og rediger den nøyaktige teksten før godkjenning. Den godkjente teksten blir registrert for separat utførelse.',
                                  'Review and edit the exact text before approval. The approved text is recorded for separate execution.',
                                )}
                              </span>
                            </div>
                          </Show>
                        </div>
                        <div class="verevon-ai-review__actions">
                          <button
                            type="button"
                            class="verevon-ai-review__btn verevon-ai-review__btn--approve"
                            disabled={busyId() !== null || ((isDraftReply(action) || isInternalNote(action)) && !reviewBodyFor(action))}
                            onClick={() => decide(action, 'approve')}
                          >
                            <Check size={14} aria-hidden="true" /> {i18n.tr('Godkjenn', 'Approve')}
                          </button>
                          <button
                            type="button"
                            class="verevon-ai-review__btn verevon-ai-review__btn--reject"
                            disabled={busyId() !== null}
                            onClick={() => decide(action, 'reject')}
                          >
                            <X size={14} aria-hidden="true" /> {i18n.tr('Avvis', 'Reject')}
                          </button>
                        </div>
                        </li>
                      </>
                    )
                  }}
                </For>
              </ul>
            </Show>
          </Show>
        </Show>

        <Show when={verifiedActions().length > 0}>
          <div class="verevon-ai-review__receipts" aria-label={i18n.tr('Verifiserte AI-handlinger', 'Verified AI actions')}>
            <For each={verifiedActions()}>
              {(action) => <p class="verevon-ai-review__receipt" data-ai-action-id={action.id}>{executionReceipt(action, i18n.tr)}</p>}
            </For>
          </div>
        </Show>

        <Show when={recorded()}>
          <p class="verevon-ai-review__recorded" role="status">{recorded()}</p>
        </Show>
        <Show when={error()}>
          <p class="verevon-ai-review__error" role="alert">{error()}</p>
        </Show>
        <p class="verevon-ai-review__note">
          {i18n.tr(
            'Å godkjenne registrerer det viste forslaget for utførelse; å avvise forkaster det. Utførelse og resultat må verifiseres separat. Hver avgjørelse blir revidert.',
            'Approving records the displayed proposal for execution; rejecting dismisses it. Execution and outcome are verified separately. Every decision is audited.',
          )}
        </p>
      </section>
    </Show>
  )
}
