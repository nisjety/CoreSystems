import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import {
  isZdrWaiverUiEnabled,
  listOrgZdrAcknowledgements,
  type OrgZdrAcknowledgementItem,
  type OrgZdrAcknowledgements,
} from '@/shared/api/connection-zdr-client'
import { useI18n } from '@/shared/i18n'
import { en } from '@/shared/i18n/en'
import { no } from '@/shared/i18n/no'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'
import { getSession } from '@/shared/session/session-store'
import { DataRow } from '@/features/settings/components/settings-ui'

// The organisation's record of zero data retention acknowledgements (ZDR Z-21b): which member confirmed that which
// connection cannot give ZDR, against which text version, and when. Owners and admins only; Integration Core and the
// gateway decide who may read it, this only draws it. Ids, versions, states and times: never the text itself and
// never a request id. The count of runs that were waived is not shown: there is no read path for it yet (D-ZDR-13),
// so nothing stands in its place. Behind VITE_ZDR_WAIVER_UI; with the switch off it renders nothing and reads nothing.
// The SPA keeps nothing of what it reads here.

/** The time as the reader's locale writes it, or an empty string for a time that does not parse. */
function stamp(iso: string | null, locale: string): string {
  if (!iso) return ''
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' })
}

export function OrgZdrAcknowledgementsSection() {
  const i18n = useI18n()
  const session = getSession()
  const copy = () => (i18n.locale() === 'no' ? no : en).zdr.waiver.admin
  // Presentation only: the gateway refuses anyone who is not an owner or admin of the organisation.
  const admin = createMemo(() => isZdrWaiverUiEnabled() && hasWorkspaceAdminAccess(session))
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const [data, setData] = createSignal<OrgZdrAcknowledgements | null>(null)
  const [state, setState] = createSignal<'loading' | 'ready' | 'failed'>('loading')
  let attempt = 0

  createEffect(
    () => (admin() ? orgId() : ''),
    (id) => {
      const current = ++attempt
      setData(null)
      if (!id) return
      setState('loading')
      listOrgZdrAcknowledgements()
        .then((result) => {
          if (current !== attempt) return
          setData(result)
          setState('ready')
        })
        .catch(() => {
          if (current !== attempt) return
          setState('failed')
        })
      return () => {
        attempt++
      }
    },
  )

  const localeTag = () => (i18n.locale() === 'no' ? 'nb-NO' : 'en-US')
  const statusLabel = (item: OrgZdrAcknowledgementItem) =>
    item.status === 'active' ? copy().status_active : copy().status_revoked
  const secondary = (item: OrgZdrAcknowledgementItem) =>
    [
      `${copy().provider}: ${item.providerKey}`,
      `${copy().capability}: ${item.capability}`,
      `${copy().text_version}: ${item.textVersion}`,
      `${copy().locale}: ${item.locale}`,
      `${copy().status}: ${statusLabel(item)}`,
    ].join(' · ')
  const meta = (item: OrgZdrAcknowledgementItem) =>
    [
      `${copy().acknowledged_at}: ${stamp(item.acknowledgedAt, localeTag())}`,
      item.revokedAt ? `${copy().revoked_at}: ${stamp(item.revokedAt, localeTag())}` : '',
    ]
      .filter(Boolean)
      .join(' · ')

  return (
    <Show when={admin()}>
      <section class="verevon-settings-zdr-acknowledgements" aria-labelledby="verevon-settings-zdr-ack-title">
        <h3 id="verevon-settings-zdr-ack-title">{copy().title}</h3>
        <p class="verevon-settings-panel-note">{copy().intro}</p>
        <Show
          when={state() === 'ready' ? data() : undefined}
          fallback={
            <p class="verevon-settings-panel-note" role={state() === 'failed' ? 'alert' : 'status'}>
              {state() === 'failed' ? copy().load_failed : copy().loading}
            </p>
          }
        >
          {(list) => (
            <>
              <p class="verevon-settings-panel-note">{copy().counts(list().counts.active, list().counts.revoked)}</p>
              <Show when={list().items.length > 0} fallback={<p class="verevon-settings-panel-note">{copy().empty}</p>}>
                <div class="verevon-settings-row-divider">
                  <For each={list().items}>
                    {(item) => <DataRow primary={`${copy().member}: ${item.userId}`} secondary={secondary(item)} meta={meta(item)} />}
                  </For>
                </div>
              </Show>
              <Show when={list().truncated}>
                <p class="verevon-settings-panel-note">{copy().truncated}</p>
              </Show>
            </>
          )}
        </Show>
      </section>
    </Show>
  )
}
