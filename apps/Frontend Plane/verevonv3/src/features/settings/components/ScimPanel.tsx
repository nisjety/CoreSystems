import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  createScimToken,
  getScimStatus,
  listScimTokens,
  revokeScimToken,
  type ScimStatus,
  type ScimToken,
} from '@/shared/api/org-identity-client'
import { ApiError } from '@/shared/api/api-error'
import { translateApiError, useI18n } from '@/shared/i18n'
import { DataRow, FeaturePanel, SettingsButton, SettingsField } from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * SCIM-provisjonering (SCIM provisioning) token panel (WS05-S8).
 *
 * An owner or admin creates a bearer token and pastes it, with the SCIM base URL,
 * into the identity provider. The token is shown once and never again; this panel
 * keeps it in a signal only until the admin dismisses it. org-core owns the tokens
 * and the SCIM surface, the gateway enforces the owner/admin split, so a member
 * sees the plain note and sends no request. While either switch is off (gateway
 * 404 `feature_disabled`, org-core 404 `not_found`) the panel falls back to the
 * same note. Nothing here is model-eligible.
 */

type PanelData = { status: ScimStatus; tokens: ScimToken[] } | 'disabled'

function isDisabled(err: unknown): boolean {
  return (
    err instanceof ApiError &&
    err.status === 404 &&
    (err.code === 'feature_disabled' || err.code === 'not_found')
  )
}

const LIFETIMES = [30, 90, 180, 365] as const

export function ScimPanel() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [data, { refetch }] = createResource(
    () => (isAdmin() && orgId() ? orgId() : undefined),
    async (id): Promise<PanelData> => {
      try {
        const [status, tokens] = await Promise.all([getScimStatus(id), listScimTokens(id)])
        return { status, tokens }
      } catch (err) {
        if (isDisabled(err)) return 'disabled'
        throw err
      }
    },
  )

  const [adding, setAdding] = createSignal(false)
  const [name, setName] = createSignal('')
  const [days, setDays] = createSignal<number>(90)
  const [busy, setBusy] = createSignal<string | null>(null)
  const [problem, setProblem] = createSignal<string | null>(null)
  const [issued, setIssued] = createSignal<string | null>(null)

  const disabled = createMemo(() => data() === 'disabled')
  const live = createMemo(() => isAdmin() && !disabled())
  const panel = createMemo(() => {
    const current = data()
    return current && current !== 'disabled' ? current : null
  })
  const baseUrl = createMemo(() => `${window.location.origin}/api/v1/scim/v2`)

  const fail = (err: unknown, fallback: { no: string; en: string }) => {
    if (err instanceof ApiError && err.code === 'no_verified_domain') {
      setProblem(
        i18n.tr(
          'Verifiser minst ett domene før du oppretter et SCIM-token.',
          'Verify at least one domain before creating a SCIM token.',
        ),
      )
      return
    }
    if (err instanceof ApiError && err.code === 'scim_token_limit') {
      setProblem(
        i18n.tr(
          'En organisasjon kan ha opptil 3 aktive SCIM-tokens. Tilbakekall et først.',
          'An organization can have up to 3 active SCIM tokens. Revoke one first.',
        ),
      )
      return
    }
    setProblem(translateApiError(err, i18n.tr, fallback))
  }

  const run = async (key: string, task: () => Promise<void>, fallback: { no: string; en: string }) => {
    setBusy(key)
    setProblem(null)
    try {
      await task()
      await refetch()
    } catch (err) {
      fail(err, fallback)
    } finally {
      setBusy(null)
    }
  }

  const create = () => {
    const value = name().trim()
    if (!value) return
    void run(
      'create',
      async () => {
        const result = await createScimToken(orgId(), { name: value, expiresInDays: days() })
        setIssued(result.bearer_token)
        setName('')
        setAdding(false)
      },
      { no: 'Kunne ikke opprette tokenet.', en: 'Could not create the token.' },
    )
  }

  const revoke = (tokenId: string) =>
    void run(
      `revoke:${tokenId}`,
      async () => {
        await revokeScimToken(orgId(), tokenId)
      },
      { no: 'Kunne ikke tilbakekalle tokenet.', en: 'Could not revoke the token.' },
    )

  const tokenState = (token: ScimToken): string => {
    if (token.revoked_at) return i18n.tr('Tilbakekalt', 'Revoked')
    if (new Date(token.expires_at).getTime() <= Date.now()) return i18n.tr('Utløpt', 'Expired')
    return i18n.tr('Aktivt', 'Active')
  }

  return (
    <FeaturePanel
      title={i18n.tr('SCIM-provisjonering', 'SCIM provisioning')}
      description={i18n.tr(
        'La identitetsleverandøren din opprette og deaktivere medlemmer automatisk.',
        'Let your identity provider create and deactivate members automatically.',
      )}
      actionLabel={i18n.tr('Opprett token', 'Create token')}
      onAction={live() ? () => setAdding((open) => !open) : undefined}
    >
      <Show
        when={live()}
        fallback={
          <p class="verevon-settings-panel-note">
            {i18n.tr('SCIM-provisjonering er ikke aktivert for denne organisasjonen.', 'SCIM provisioning is not enabled for this organization.')}
          </p>
        }
      >
        <Show when={issued()}>
          {(token) => (
            <div class="verevon-settings-panel-note" role="status" data-testid="scim-token-issued">
              <p>
                <strong>
                  {i18n.tr(
                    'Kopier tokenet nå. Det vises ikke igjen.',
                    'Copy the token now. It will not be shown again.',
                  )}
                </strong>
              </p>
              <p>
                <code>{token()}</code>
              </p>
              <p>
                <strong>{i18n.tr('SCIM-adresse', 'SCIM base URL')}:</strong> <code>{baseUrl()}</code>
              </p>
              <SettingsButton type="button" settingsSize="sm" onClick={() => setIssued(null)}>
                {i18n.tr('Jeg har kopiert det', 'I have copied it')}
              </SettingsButton>
            </div>
          )}
        </Show>

        <Show when={adding()}>
          <form
            class="verevon-settings-field-grid verevon-settings-field-grid--spaced"
            data-testid="scim-add-form"
            onSubmit={(event) => {
              event.preventDefault()
              create()
            }}
          >
            <SettingsField
              id="scim-token-name"
              label={i18n.tr('Navn', 'Name')}
              placeholder="Entra ID"
              value={name()}
              onInput={(event) => setName(event.currentTarget.value)}
            />
            <label class="verevon-settings-field">
              <span>{i18n.tr('Gyldig i', 'Valid for')}</span>
              <select value={days()} onChange={(event) => setDays(Number(event.currentTarget.value))}>
                <For each={LIFETIMES}>
                  {(option) => (
                    <option value={option}>
                      {option} {i18n.tr('dager', 'days')}
                    </option>
                  )}
                </For>
              </select>
            </label>
            <span>
              <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={busy() !== null || !name().trim()}>
                {i18n.tr('Opprett', 'Create')}
              </SettingsButton>
            </span>
          </form>
        </Show>

        <Show when={problem()}>
          {(text) => (
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {text()}
            </p>
          )}
        </Show>

        <Show when={panel()}>
          {(current) => (
            <p class="verevon-settings-panel-note" data-testid="scim-status">
              {i18n.tr(
                `${current().status.active_tokens} av ${current().status.max_tokens} tokens aktive. ${current().status.active_users} aktive SCIM-brukere.`,
                `${current().status.active_tokens} of ${current().status.max_tokens} tokens active. ${current().status.active_users} active SCIM users.`,
              )}
            </p>
          )}
        </Show>

        <Show
          when={(panel()?.tokens.length ?? 0) > 0}
          fallback={
            <p class="verevon-settings-panel-note" role="status" data-testid="scim-empty">
              {data.loading
                ? i18n.tr('Laster tokens …', 'Loading tokens...')
                : i18n.tr('Ingen SCIM-tokens er opprettet.', 'No SCIM tokens have been created.')}
            </p>
          }
        >
          <div class="verevon-settings-row-divider" data-testid="scim-tokens">
            <For each={panel()?.tokens ?? []}>
              {(token) => (
                <>
                  <DataRow
                    primary={`${token.name} (${token.prefix}…)`}
                    secondary={tokenState(token)}
                    meta={new Date(token.expires_at).toLocaleDateString()}
                  />
                  <Show when={!token.revoked_at}>
                    <p>
                      <SettingsButton
                        type="button"
                        settingsSize="sm"
                        disabled={busy() !== null}
                        data-testid={`scim-revoke-${token.id}`}
                        onClick={() => revoke(token.id)}
                      >
                        {i18n.tr('Tilbakekall', 'Revoke')}
                      </SettingsButton>
                    </p>
                  </Show>
                </>
              )}
            </For>
          </div>
        </Show>
      </Show>
    </FeaturePanel>
  )
}
