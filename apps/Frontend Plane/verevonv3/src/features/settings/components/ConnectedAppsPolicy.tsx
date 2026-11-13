import { createSignal, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsSelect } from '@/features/settings/components/settings-ui'
import { hasErrorCode } from '@/shared/api/api-error'
import {
  listConnectorPolicies,
  setConnectorPolicy,
  type ConnectorPolicies,
  type ConnectorPolicyInput,
  type ConnectorReadLevel,
  type ConnectorWriteLevel,
} from '@/shared/api/orchestration-client'

/** The anchor a provider approval card links to ("Change for connected apps"). */
export const CONNECTED_APPS_POLICY_ID = 'connected-apps-policy'

const READ_ORDER: readonly ConnectorReadLevel[] = ['use_without_asking', 'ask_first', 'off']
const WRITE_ORDER: readonly ConnectorWriteLevel[] = ['always_ask', 'off']

/** Whether the org's level is stricter than the person's, so the org's is the one in force. */
function orgIsStricter<T extends string>(order: readonly T[], own: T, org: T): boolean {
  return order.indexOf(org) > order.indexOf(own)
}

/**
 * EP-10 S7: how the agent may use the person's connected apps. Reads are used without asking, asked
 * about first, or off; writes are always asked about, or off. The levels only tighten and apply to
 * every connected app and account at once. While the deployment does not enforce tool policies the
 * panel says so, because a level that is stored and not honoured must never look honoured.
 */
export function ConnectedAppsPolicyView(props: {
  policies: ConnectorPolicies
  busy: boolean
  onChange: (input: ConnectorPolicyInput) => void
}) {
  const i18n = useI18n()
  const readLabel = (level: ConnectorReadLevel) => {
    switch (level) {
      case 'ask_first':
        return i18n.tr('Spør først', 'Ask first')
      case 'off':
        return i18n.tr('Av', 'Off')
      default:
        return i18n.tr('Bruk uten å spørre', 'Use without asking')
    }
  }
  const writeLabel = (level: ConnectorWriteLevel) =>
    level === 'off' ? i18n.tr('Av', 'Off') : i18n.tr('Spør alltid', 'Always ask')

  return (
    <section id={CONNECTED_APPS_POLICY_ID}>
      <SectionHeader
        title={i18n.tr('Hvordan agenten bruker tilkoblede apper', 'How the agent uses connected apps')}
        description={i18n.tr(
          'Gjelder alle tilkoblede apper og kontoer samtidig. Du kan bare stramme inn: en sending eller annen endring spør deg alltid først.',
          'Applies to every connected app and account at once. You can only tighten: a send or any other change always asks you first.',
        )}
      />
      <Show when={!props.policies.enforced}>
        {/* A standing notice, not a live update: the page's own status line stays the one status region. */}
        <p class="verevon-settings-status-message" role="note">
          <span class="verevon-trust-chip">{i18n.tr('Ikke håndhevet ennå', 'Not enforced yet')}</span>{' '}
          {i18n.tr(
            'Valgene lagres, men agenten følger dem ikke før håndheving av verktøyregler er slått på.',
            'Your choices are saved, but the agent does not follow them until tool policy enforcement is turned on.',
          )}
        </p>
      </Show>
      <div class="verevon-settings-list-card">
        <SettingsSelect
          id={`${CONNECTED_APPS_POLICY_ID}-read`}
          label={i18n.tr('Lese e-post, kalender og filer', 'Read mail, calendar and files')}
          value={props.policies.read}
          disabled={props.busy}
          options={READ_ORDER.map((level) => ({ value: level, label: readLabel(level) }))}
          onChange={(event) => {
            const level = READ_ORDER.find((candidate) => candidate === event.currentTarget.value)
            if (level && level !== props.policies.read) props.onChange({ kind: 'read', level })
          }}
        />
        <Show when={props.policies.read === 'ask_first'}>
          <p class="verevon-settings-subnote">
            {i18n.tr(
              'Spør først: tilgjengelig i Utfør-modus, der agenten kan vente på svaret ditt. I Spør-modus brukes ikke tilkoblede apper.',
              'Ask first: available in Do mode, where the agent can wait for your answer. Ask mode leaves connected apps out.',
            )}
          </p>
        </Show>
        <Show when={orgIsStricter(READ_ORDER, props.policies.read, props.policies.orgRead)}>
          <p class="verevon-settings-subnote">
            {i18n.tr('Organisasjonen din har satt lesing til', 'Your organisation has set reading to')}{' '}
            {readLabel(props.policies.orgRead).toLowerCase()}. {i18n.tr('Det strengeste valget gjelder.', 'The stricter choice applies.')}
          </p>
        </Show>
        <SettingsSelect
          id={`${CONNECTED_APPS_POLICY_ID}-write`}
          label={i18n.tr('Sende og endre', 'Send and change')}
          value={props.policies.write}
          disabled={props.busy}
          options={WRITE_ORDER.map((level) => ({ value: level, label: writeLabel(level) }))}
          onChange={(event) => {
            const level = WRITE_ORDER.find((candidate) => candidate === event.currentTarget.value)
            if (level && level !== props.policies.write) props.onChange({ kind: 'write', level })
          }}
        />
        <Show when={orgIsStricter(WRITE_ORDER, props.policies.write, props.policies.orgWrite)}>
          <p class="verevon-settings-subnote">
            {i18n.tr('Organisasjonen din har slått av sending og endringer.', 'Your organisation has turned sending and changes off.')}{' '}
            {i18n.tr('Det strengeste valget gjelder.', 'The stricter choice applies.')}
          </p>
        </Show>
      </div>
    </section>
  )
}

/** The panel with its data: the levels are read from the tool-policy list and written one at a time. */
export function ConnectedAppsPolicy() {
  const i18n = useI18n()
  const [policies, { refetch }] = createResource(() => listConnectorPolicies())
  const [busy, setBusy] = createSignal(false)
  const [actionError, setActionError] = createSignal<string | null>(null)
  const [actionNote, setActionNote] = createSignal<string | null>(null)

  const change = async (input: ConnectorPolicyInput) => {
    if (busy()) return
    setBusy(true)
    setActionError(null)
    setActionNote(null)
    try {
      await setConnectorPolicy(input)
      await refetch()
    } catch (err) {
      // Every provider write already asks the person, so the server refuses an ask as already_always_ask.
      // That tells the person what already happens: an info note, not a failure to save the choice.
      if (hasErrorCode(err, 'already_always_ask')) {
        setActionNote(
          i18n.tr(
            'Verevon spør deg allerede før hver handling i denne appen.',
            'Verevon already asks you before every action in this app.',
          ),
        )
      } else {
        setActionError(
          translateApiError(err, i18n.tr, {
            no: 'Kunne ikke lagre valget.',
            en: 'Could not save the choice.',
          }),
        )
      }
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show when={actionNote()}>
        {(message) => (
          <p class="verevon-settings-status-message" role="note">
            {message()}
          </p>
        )}
      </Show>
      <Show when={policies.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste valgene for tilkoblede apper.', 'Could not load the connected app choices.')}
        </p>
      </Show>
      <Show when={policies()}>
        {(current) => <ConnectedAppsPolicyView policies={current()} busy={busy()} onChange={(input) => void change(input)} />}
      </Show>
    </>
  )
}
