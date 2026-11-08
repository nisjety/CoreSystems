import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  addSsoProvider,
  getSsoPolicy,
  removeSsoProvider,
  updateSsoPolicy,
  type SsoEnforceReason,
  type SsoPolicy,
  type SsoPolicyMode,
} from '@/shared/api/org-identity-client'
import { requestJson } from '@/shared/api/http'
import { ApiError } from '@/shared/api/api-error'
import { translateApiError, useI18n } from '@/shared/i18n'
import { DataRow, FeaturePanel, SettingsButton, SettingsField, SettingsSelect } from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Single sign-on policy panel (WS05-S7).
 *
 * An owner or admin registers an OpenID Connect provider for a verified domain,
 * rehearses the policy in `report` mode (nothing is blocked; would-be blocks are
 * audited) and only then switches to `enforce`. org-core checks the preconditions
 * (verified domain, working provider, the caller's own recent SSO sign-in,
 * two-factor break-glass owners) and the gateway enforces the owner/admin split,
 * so a member sees the plain note and sends no request. While the gateway switch
 * is off (404 `feature_disabled`) the panel shows the same note. Nothing here is
 * model-eligible.
 */

const MODE_LABEL: Record<SsoPolicyMode, { no: string; en: string }> = {
  off: { no: 'Av', en: 'Off' },
  report: { no: 'Rapporter (blokkerer ikke)', en: 'Report only (blocks nothing)' },
  enforce: { no: 'Krev SSO', en: 'Require SSO' },
}

const REASON_COPY: Record<SsoEnforceReason, { no: string; en: string }> = {
  no_verified_domain: {
    no: 'Verifiser et selskapsdomene først.',
    en: 'Verify a company domain first.',
  },
  no_provider: {
    no: 'Legg til en identitetsleverandør på et verifisert domene.',
    en: 'Add an identity provider on a verified domain.',
  },
  sso_not_proven: {
    no: 'Logg inn med SSO selv (siste 24 timer) for å bevise at leverandøren fungerer.',
    en: 'Sign in with SSO yourself (within the last 24 hours) to prove the provider works.',
  },
  break_glass_invalid: {
    no: 'Velg minst én eier som nødtilgang (maks to).',
    en: 'Pick at least one owner as break-glass access (at most two).',
  },
  break_glass_requires_2fa: {
    no: 'Nødtilgangs-eiere må ha tofaktorautentisering på.',
    en: 'Break-glass owners must have two-factor authentication on.',
  },
  identity_provider_unavailable: {
    no: 'Identitetstjenesten svarer ikke akkurat nå.',
    en: 'The identity service is not answering right now.',
  },
}

const ERROR_COPY: Record<string, { no: string; en: string }> = {
  policy_revision_conflict: {
    no: 'Policyen ble endret av en annen. Last inn på nytt og prøv igjen.',
    en: 'The policy was changed by someone else. Reload and try again.',
  },
  provider_exists: {
    no: 'En leverandør med denne ID-en eller dette domenet finnes allerede.',
    en: 'A provider with this id or domain already exists.',
  },
  invalid_issuer: {
    no: 'Utsteder (issuer) må være en https-adresse.',
    en: 'The issuer must be an https address.',
  },
  domain_not_verified: {
    no: 'Domenet er ikke verifisert for denne organisasjonen.',
    en: 'The domain is not verified for this organization.',
  },
  last_provider_enforced: {
    no: 'Den siste leverandøren kan ikke fjernes mens SSO kreves.',
    en: 'The last provider cannot be removed while SSO is required.',
  },
  identity_unavailable: {
    no: 'SSO-administrasjon er ikke tilgjengelig akkurat nå.',
    en: 'SSO administration is not available right now.',
  },
}

interface OwnerChoice {
  userId: string
  label: string
}

function isFeatureDisabled(err: unknown): boolean {
  return err instanceof ApiError && err.status === 404 && err.code === 'feature_disabled'
}

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {}
}

function text(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

/** Owners of the organization, from the same members route the Members section reads. */
async function loadOwners(orgId: string): Promise<OwnerChoice[]> {
  const data = await requestJson<{ members?: unknown[] }>(`/api/v1/orgs/${encodeURIComponent(orgId)}/members`)
  const list = Array.isArray(data?.members) ? data.members : []
  const owners: OwnerChoice[] = []
  for (const entry of list) {
    const member = record(entry)
    const user = record(member.user)
    const userId = text(member.userId) || text(member.user_id) || text(user.id)
    if (!userId || text(member.role).toLowerCase() !== 'owner') continue
    owners.push({ userId, label: text(member.name) || text(user.name) || text(member.email) || text(user.email) || userId })
  }
  return owners
}

export function SsoPolicyPanel() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [policy, { refetch }] = createResource(
    () => (isAdmin() && orgId() ? orgId() : undefined),
    async (id): Promise<SsoPolicy | 'disabled'> => {
      try {
        return await getSsoPolicy(id)
      } catch (err) {
        if (isFeatureDisabled(err)) return 'disabled'
        throw err
      }
    },
  )
  const [owners] = createResource(
    () => (isAdmin() && orgId() ? orgId() : undefined),
    (id) => loadOwners(id).catch((): OwnerChoice[] => []),
  )

  const current = createMemo((): SsoPolicy | null => {
    const value = policy()
    return value && value !== 'disabled' ? value : null
  })
  const disabled = createMemo(() => policy() === 'disabled')
  const live = createMemo(() => isAdmin() && !disabled())

  const [mode, setMode] = createSignal<SsoPolicyMode>('off')
  const [breakGlass, setBreakGlass] = createSignal<string[]>([])
  const [busy, setBusy] = createSignal<string | null>(null)
  const [problem, setProblem] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  const [adding, setAdding] = createSignal(false)
  const [draft, setDraft] = createSignal({ providerId: '', domain: '', issuer: '', clientId: '', clientSecret: '' })

  // Seed the editable fields from the server each time a fresh policy arrives.
  createEffect(
    () => current(),
    (value) => {
      if (!value) return
      setMode(value.mode)
      setBreakGlass([...value.break_glass_user_ids])
    },
  )

  const fail = (err: unknown, fallback: { no: string; en: string }) => {
    if (err instanceof ApiError) {
      const reason = REASON_COPY[err.code as SsoEnforceReason]
      const known = ERROR_COPY[err.code ?? ''] ?? reason
      if (known) {
        setProblem(i18n.tr(known.no, known.en))
        return
      }
    }
    setProblem(translateApiError(err, i18n.tr, fallback))
  }

  const run = async (key: string, task: () => Promise<void>, fallback: { no: string; en: string }) => {
    setBusy(key)
    setProblem(null)
    setNotice(null)
    try {
      await task()
      await refetch()
    } catch (err) {
      fail(err, fallback)
    } finally {
      setBusy(null)
    }
  }

  const save = () => {
    const value = current()
    if (!value) return
    void run(
      'policy',
      async () => {
        await updateSsoPolicy(orgId(), { mode: mode(), breakGlassUserIds: breakGlass(), expectedRevision: value.revision })
        setNotice(i18n.tr('SSO-policyen er lagret.', 'The SSO policy was saved.'))
      },
      { no: 'Kunne ikke lagre SSO-policyen.', en: 'Could not save the SSO policy.' },
    )
  }

  const addProvider = () => {
    const value = draft()
    void run(
      'provider-add',
      async () => {
        await addSsoProvider(orgId(), value)
        setDraft({ providerId: '', domain: '', issuer: '', clientId: '', clientSecret: '' })
        setAdding(false)
        setNotice(i18n.tr('Leverandøren er lagt til.', 'The provider was added.'))
      },
      { no: 'Kunne ikke legge til leverandøren.', en: 'Could not add the provider.' },
    )
  }

  const removeProvider = (providerId: string) =>
    void run(
      `provider-remove:${providerId}`,
      async () => {
        await removeSsoProvider(orgId(), providerId)
      },
      { no: 'Kunne ikke fjerne leverandøren.', en: 'Could not remove the provider.' },
    )

  const toggleBreakGlass = (userId: string, on: boolean) => {
    setBreakGlass((ids) => {
      const without = ids.filter((id) => id !== userId)
      return on && without.length < 2 ? [...without, userId] : without
    })
  }

  const setField = (key: keyof ReturnType<typeof draft>) => (event: InputEvent & { currentTarget: HTMLInputElement }) =>
    setDraft((value) => ({ ...value, [key]: event.currentTarget.value }))

  const blockers = createMemo((): SsoEnforceReason[] => current()?.can_enforce.reasons ?? [])

  return (
    <FeaturePanel
      title={i18n.tr('Enkel pålogging (SSO)', 'Single sign-on (SSO)')}
      description={i18n.tr(
        'Registrer en OpenID Connect-leverandør for et verifisert domene, test policyen i rapportmodus og krev deretter SSO.',
        'Register an OpenID Connect provider for a verified domain, rehearse the policy in report mode, then require SSO.',
      )}
      actionLabel={i18n.tr('Legg til leverandør', 'Add provider')}
      onAction={live() ? () => setAdding((open) => !open) : undefined}
    >
      <Show
        when={live()}
        fallback={
          <p class="verevon-settings-panel-note">
            {i18n.tr(
              'Enkel pålogging (SSO) er ikke konfigurert for denne organisasjonen. Bare eiere og administratorer kan sette den opp.',
              'Single sign-on is not configured for this organization. Only owners and admins can set it up.',
            )}
          </p>
        }
      >
        <Show when={adding()}>
          <form
            class="verevon-settings-field-grid verevon-settings-field-grid--spaced"
            data-testid="sso-provider-form"
            onSubmit={(event) => {
              event.preventDefault()
              addProvider()
            }}
          >
            <SettingsField id="sso-provider-id" label={i18n.tr('Leverandør-ID', 'Provider id')} placeholder="acme-entra" value={draft().providerId} onInput={setField('providerId')} />
            <SettingsField id="sso-provider-domain" label={i18n.tr('Verifisert domene', 'Verified domain')} placeholder="example.com" value={draft().domain} onInput={setField('domain')} />
            <SettingsField id="sso-provider-issuer" label={i18n.tr('Utsteder (issuer)', 'Issuer')} placeholder="https://login.example.com" value={draft().issuer} onInput={setField('issuer')} />
            <SettingsField id="sso-provider-client-id" label={i18n.tr('Klient-ID', 'Client id')} value={draft().clientId} onInput={setField('clientId')} />
            <SettingsField
              id="sso-provider-client-secret"
              label={i18n.tr('Klienthemmelighet', 'Client secret')}
              helpText={i18n.tr('Lagres kun og vises aldri igjen.', 'Stored once and never shown again.')}
              value={draft().clientSecret}
              onInput={setField('clientSecret')}
            />
            <span>
              <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={busy() !== null}>
                {i18n.tr('Legg til', 'Add')}
              </SettingsButton>
            </span>
          </form>
        </Show>

        <Show when={problem()}>
          {(message) => (
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {message()}
            </p>
          )}
        </Show>
        <Show when={notice()}>
          {(message) => (
            <p class="verevon-settings-status-message" role="status">
              {message()}
            </p>
          )}
        </Show>

        <Show
          when={current()}
          fallback={
            <p class="verevon-settings-panel-note" role="status">
              {i18n.tr('Laster SSO-policy …', 'Loading SSO policy...')}
            </p>
          }
        >
          {(value) => (
            <>
              <div class="verevon-settings-row-divider" data-testid="sso-providers">
                <Show
                  when={value().providers.length > 0}
                  fallback={
                    <p class="verevon-settings-panel-note" data-testid="sso-no-providers">
                      {i18n.tr('Ingen identitetsleverandører er lagt til.', 'No identity providers have been added.')}
                    </p>
                  }
                >
                  <For each={value().providers}>
                    {(provider) => (
                      <>
                        <DataRow
                          primary={provider.provider_id}
                          secondary={`${provider.domain} · ${provider.issuer}`}
                          meta={provider.domain_verified ? i18n.tr('Verifisert domene', 'Verified domain') : i18n.tr('Domenet er ikke verifisert', 'Domain not verified')}
                        />
                        <p>
                          <SettingsButton
                            type="button"
                            settingsSize="sm"
                            disabled={busy() !== null}
                            data-testid={`sso-provider-remove-${provider.provider_id}`}
                            onClick={() => removeProvider(provider.provider_id)}
                          >
                            {i18n.tr('Fjern', 'Remove')}
                          </SettingsButton>
                        </p>
                      </>
                    )}
                  </For>
                </Show>
              </div>

              <div class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
                <SettingsSelect
                  id="sso-policy-mode"
                  label={i18n.tr('Policy', 'Policy')}
                  value={mode()}
                  disabled={busy() !== null}
                  onChange={(event) => setMode(event.currentTarget.value as SsoPolicyMode)}
                  options={(['off', 'report', 'enforce'] as const).map((option) => ({
                    value: option,
                    label: i18n.tr(MODE_LABEL[option].no, MODE_LABEL[option].en),
                  }))}
                />
              </div>

              <fieldset class="verevon-settings-panel-note" data-testid="sso-break-glass">
                <legend>{i18n.tr('Nødtilgang (eiere som alltid kan bruke passord + tofaktor)', 'Break-glass owners (can always use password + two-factor)')}</legend>
                <Show
                  when={(owners() ?? []).length > 0}
                  fallback={<p>{i18n.tr('Ingen eiere funnet.', 'No owners found.')}</p>}
                >
                  <For each={owners() ?? []}>
                    {(owner) => (
                      <label>
                        <input
                          type="checkbox"
                          checked={breakGlass().includes(owner.userId)}
                          disabled={busy() !== null}
                          onChange={(event) => toggleBreakGlass(owner.userId, event.currentTarget.checked)}
                        />{' '}
                        {owner.label}
                      </label>
                    )}
                  </For>
                </Show>
              </fieldset>

              <Show when={mode() === 'enforce' && blockers().length > 0}>
                <div class="verevon-settings-panel-note" data-testid="sso-enforce-blockers">
                  <p>{i18n.tr('Før du kan kreve SSO:', 'Before SSO can be required:')}</p>
                  <ul>
                    <For each={blockers()}>
                      {(reason) => <li>{i18n.tr(REASON_COPY[reason].no, REASON_COPY[reason].en)}</li>}
                    </For>
                  </ul>
                </div>
              </Show>

              <p>
                <SettingsButton type="button" variant="primary" settingsSize="sm" disabled={busy() !== null} data-testid="sso-policy-save" onClick={save}>
                  {i18n.tr('Lagre policy', 'Save policy')}
                </SettingsButton>
              </p>
            </>
          )}
        </Show>
      </Show>
    </FeaturePanel>
  )
}
