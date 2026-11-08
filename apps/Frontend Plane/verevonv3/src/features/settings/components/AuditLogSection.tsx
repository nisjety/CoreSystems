import { Loader2, ShieldCheck } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { listAuditPage, type AuditEvent } from '@/shared/api/audit-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { DataRow, SectionHeader, SettingsButton, SettingsField } from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'

/**
 * Revisjonslogg (audit log) list view (WS05-S4).
 *
 * Owner/admin only. audit-core and the gateway enforce the same split from the
 * signed org role, so this section is simply not offered to anyone else; a
 * non-admin who reaches the URL sees the note below and sends no request. The
 * view pages the org trail newest-first with a keyset cursor and a few narrowing
 * filters. There is no export here: export is the compliance API in
 * AuditComplianceSection (hash-chained, audited, plan-gated). No action in this
 * view is model-eligible; an agent never reads the audit log.
 *
 * Rows are content-free metadata (event, plane, outcome, subject, request id).
 */

const PAGE_SIZE = 50

type Filters = { event: string; plane: string; outcome: string; subject: string; since: string }
const EMPTY_FILTERS: Filters = { event: '', plane: '', outcome: '', subject: '', since: '' }

function isoFromLocalDate(value: string): string | undefined {
  if (!value) return undefined
  const parsed = new Date(`${value}T00:00:00`)
  return Number.isNaN(parsed.getTime()) ? undefined : parsed.toISOString()
}

export function AuditLogSection() {
  const i18n = useI18n()
  const session = getSession()
  const isAdmin = createMemo(() => {
    const role = (session.activeOrg?.role ?? '').toLowerCase()
    return session.status === 'authenticated' && (role === 'owner' || role === 'admin')
  })

  const [draft, setDraft] = createSignal<Filters>({ ...EMPTY_FILTERS })
  const [applied, setApplied] = createSignal<Filters>({ ...EMPTY_FILTERS })
  const [events, setEvents] = createSignal<AuditEvent[]>([])
  const [nextCursor, setNextCursor] = createSignal<string | undefined>(undefined)
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)

  // Bumped on every new query so a slow earlier page cannot land after a newer one.
  let generation = 0

  const query = (filters: Filters, cursor?: string) => {
    const since = isoFromLocalDate(filters.since)
    return {
      limit: PAGE_SIZE,
      ...(filters.event.trim() ? { event: filters.event.trim() } : {}),
      ...(filters.plane.trim() ? { plane: filters.plane.trim() } : {}),
      ...(filters.outcome.trim() ? { outcome: filters.outcome.trim() } : {}),
      ...(filters.subject.trim() ? { subject: filters.subject.trim() } : {}),
      ...(since ? { since } : {}),
      ...(cursor ? { cursor } : {}),
    }
  }

  const load = async (filters: Filters, cursor?: string) => {
    const mine = ++generation
    setLoading(true)
    setError(null)
    try {
      const page = await listAuditPage(query(filters, cursor))
      if (mine !== generation) return
      setEvents((current) => (cursor ? [...current, ...page.events] : page.events))
      setNextCursor(page.nextCursor)
    } catch (err) {
      if (mine !== generation) return
      if (!cursor) setEvents([])
      setNextCursor(undefined)
      setError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke laste revisjonsloggen.',
          en: 'Could not load the audit log.',
        }),
      )
    } finally {
      if (mine === generation) setLoading(false)
    }
  }

  // First page whenever the applied filters (or the active org) change.
  createEffect(
    () => ({ admin: isAdmin(), org: session.activeOrg?.id, filters: applied() }),
    ({ admin, filters }) => {
      if (!admin) return
      void load(filters)
    },
  )

  const setField = (key: keyof Filters, value: string) => setDraft((current) => ({ ...current, [key]: value }))

  const dateLabel = (event: AuditEvent) => {
    if (!event.occurredAt || Number.isNaN(new Date(event.occurredAt).getTime())) return ''
    return new Date(event.occurredAt).toLocaleString()
  }

  return (
    <>
      <SectionHeader
        title={i18n.tr('Revisjonslogg', 'Audit log')}
        description={i18n.tr(
          'Hvem som gjorde hva og når, for hele organisasjonen. Radene inneholder bare metadata, ikke innhold.',
          'Who did what and when, across the organization. Rows hold metadata only, never content.',
        )}
      />

      <Show
        when={isAdmin()}
        fallback={
          <p class="verevon-settings-subnote" data-testid="audit-log-admin-only">
            <ShieldCheck size={14} aria-hidden="true" />{' '}
            {i18n.tr(
              'Bare eiere og administratorer kan se organisasjonens revisjonslogg.',
              'Only owners and admins can see the organization audit log.',
            )}
          </p>
        }
      >
        <form
          class="verevon-settings-field-grid verevon-settings-field-grid--spaced"
          data-testid="audit-log-filters"
          onSubmit={(event) => {
            event.preventDefault()
            setApplied({ ...draft() })
          }}
        >
          <SettingsField
            id="audit-filter-event"
            label={i18n.tr('Hendelse', 'Event')}
            placeholder="tool_action"
            value={draft().event}
            onInput={(event) => setField('event', event.currentTarget.value)}
          />
          <SettingsField
            id="audit-filter-plane"
            label={i18n.tr('Plan', 'Plane')}
            placeholder="control"
            value={draft().plane}
            onInput={(event) => setField('plane', event.currentTarget.value)}
          />
          <SettingsField
            id="audit-filter-outcome"
            label={i18n.tr('Utfall', 'Outcome')}
            placeholder="ok"
            value={draft().outcome}
            onInput={(event) => setField('outcome', event.currentTarget.value)}
          />
          <SettingsField
            id="audit-filter-subject"
            label={i18n.tr('Gjelder', 'Subject')}
            value={draft().subject}
            onInput={(event) => setField('subject', event.currentTarget.value)}
          />
          <SettingsField
            id="audit-filter-since"
            label={i18n.tr('Fra dato', 'From date')}
            helpText={i18n.tr('Standard er de siste 7 dagene.', 'Defaults to the last 7 days.')}
            placeholder="2026-01-31"
            value={draft().since}
            onInput={(event) => setField('since', event.currentTarget.value)}
          />
          <span>
            <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={loading()}>
              {i18n.tr('Bruk filter', 'Apply filters')}
            </SettingsButton>{' '}
            <SettingsButton
              type="button"
              settingsSize="sm"
              disabled={loading()}
              onClick={() => {
                setDraft({ ...EMPTY_FILTERS })
                setApplied({ ...EMPTY_FILTERS })
              }}
            >
              {i18n.tr('Nullstill', 'Reset')}
            </SettingsButton>
          </span>
        </form>

        <Show when={error()}>
          {(text) => (
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {text()}
            </p>
          )}
        </Show>

        <Show
          when={events().length > 0}
          fallback={
            <p class="verevon-settings-panel-note" role="status" data-testid="audit-log-empty">
              {loading()
                ? i18n.tr('Laster revisjonslogg …', 'Loading audit log...')
                : i18n.tr('Ingen hendelser for dette utvalget.', 'No events for this selection.')}
            </p>
          }
        >
          <div class="verevon-settings-row-divider" data-testid="audit-log-rows">
            <For each={events()}>
              {(event) => (
                <DataRow
                  primary={`${event.event ?? i18n.tr('Hendelse', 'Event')}${event.outcome ? ` - ${event.outcome}` : ''}`}
                  secondary={[event.plane, event.actor || event.userId, event.actorRole, event.subject, event.requestId]
                    .filter(Boolean)
                    .join(' · ')}
                  meta={dateLabel(event) || event.id || ''}
                />
              )}
            </For>
          </div>
          <Show when={nextCursor()}>
            {(cursor) => (
              <p>
                <SettingsButton
                  type="button"
                  settingsSize="sm"
                  disabled={loading()}
                  data-testid="audit-log-load-more"
                  onClick={() => void load(applied(), cursor())}
                >
                  <Show when={loading()} fallback={i18n.tr('Last flere', 'Load more')}>
                    <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Laster …', 'Loading...')}
                  </Show>
                </SettingsButton>
              </p>
            )}
          </Show>
        </Show>
      </Show>
    </>
  )
}
