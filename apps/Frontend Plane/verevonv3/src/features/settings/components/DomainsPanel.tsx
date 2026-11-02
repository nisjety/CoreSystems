import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  addOrgDomain,
  listOrgDomains,
  revokeOrgDomain,
  verifyOrgDomain,
  type OrgDomain,
  type OrgDomainCheckReason,
} from '@/shared/api/org-identity-client'
import { ApiError } from '@/shared/api/api-error'
import { translateApiError, useI18n } from '@/shared/i18n'
import { DataRow, FeaturePanel, SettingsButton, SettingsField } from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Verifiserte domener (verified domains) panel (WS05-S6).
 *
 * An owner or admin adds a company domain, publishes the TXT challenge in DNS and
 * asks org-core to check it. org-core owns the proof (DNS only: a website or an
 * e-mail address never counts) and the gateway enforces the owner/admin split, so
 * a member sees the plain note and sends no request. While the gateway switch is
 * off (404 `feature_disabled`) the panel falls back to the same note. Nothing here
 * is model-eligible: an agent never administers identity.
 */

const REASON_COPY: Record<OrgDomainCheckReason, { no: string; en: string }> = {
  txt_not_found: {
    no: 'Fant ikke TXT-oppføringen ennå. DNS kan bruke litt tid på å oppdatere seg.',
    en: 'The TXT record was not found yet. DNS can take a while to update.',
  },
  txt_mismatch: {
    no: 'TXT-oppføringen finnes, men verdien stemmer ikke med utfordringen.',
    en: 'A TXT record exists but its value does not match the challenge.',
  },
  dns_unavailable: {
    no: 'DNS svarte ikke akkurat nå. Prøv igjen om litt.',
    en: 'DNS did not answer just now. Try again shortly.',
  },
  challenge_expired: {
    no: 'Utfordringen er utløpt. Fjern domenet og legg det til på nytt for å få en ny.',
    en: 'The challenge has expired. Remove the domain and add it again for a new one.',
  },
}

const ADD_REASON_COPY: Record<string, { no: string; en: string }> = {
  empty: { no: 'Skriv inn et domene.', en: 'Enter a domain.' },
  too_long: { no: 'Domenet er for langt.', en: 'The domain is too long.' },
  malformed: { no: 'Domenet er ikke gyldig.', en: 'The domain is not valid.' },
  ip_literal: { no: 'En IP-adresse kan ikke verifiseres som domene.', en: 'An IP address cannot be verified as a domain.' },
  public_suffix: { no: 'Dette er et offentlig domenesuffiks, ikke et selskapsdomene.', en: 'This is a public suffix, not a company domain.' },
  free_mail: { no: 'E-posttilbydere som Gmail kan ikke verifiseres som selskapsdomene.', en: 'Free mail providers such as Gmail cannot be verified as a company domain.' },
}

function isFeatureDisabled(err: unknown): boolean {
  return err instanceof ApiError && err.status === 404 && err.code === 'feature_disabled'
}

export function DomainsPanel() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [domains, { refetch }] = createResource(
    () => (isAdmin() && orgId() ? orgId() : undefined),
    async (id): Promise<OrgDomain[] | 'disabled'> => {
      try {
        return await listOrgDomains(id)
      } catch (err) {
        if (isFeatureDisabled(err)) return 'disabled'
        throw err
      }
    },
  )

  const [adding, setAdding] = createSignal(false)
  const [draft, setDraft] = createSignal('')
  const [busy, setBusy] = createSignal<string | null>(null)
  const [problem, setProblem] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)

  const rows = createMemo((): OrgDomain[] => {
    const current = domains()
    return Array.isArray(current) ? current : []
  })
  const disabled = createMemo(() => domains() === 'disabled')
  const live = createMemo(() => isAdmin() && !disabled())

  const fail = (err: unknown, fallback: { no: string; en: string }) => {
    if (err instanceof ApiError && err.code === 'invalid_domain') {
      const reason = typeof err.details?.reason === 'string' ? err.details.reason : ''
      const copy = ADD_REASON_COPY[reason]
      setProblem(copy ? i18n.tr(copy.no, copy.en) : i18n.tr('Domenet er ikke gyldig.', 'The domain is not valid.'))
      return
    }
    if (err instanceof ApiError && err.code === 'domain_verified_elsewhere') {
      setProblem(
        i18n.tr(
          'Domenet er allerede verifisert av en annen organisasjon.',
          'This domain is already verified by another organization.',
        ),
      )
      return
    }
    if (err instanceof ApiError && err.code === 'domain_limit_reached') {
      setProblem(i18n.tr('En organisasjon kan ha opptil 10 domener.', 'An organization can have up to 10 domains.'))
      return
    }
    if (err instanceof ApiError && err.code === 'domain_in_use') {
      setProblem(
        i18n.tr(
          'Domenet brukes av organisasjonens påkrevde single sign-on og kan ikke fjernes.',
          'The domain backs the organization’s enforced single sign-on and cannot be removed.',
        ),
      )
      return
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

  const add = () => {
    const value = draft().trim()
    if (!value) return
    void run(
      'add',
      async () => {
        await addOrgDomain(orgId(), value)
        setDraft('')
        setAdding(false)
      },
      { no: 'Kunne ikke legge til domenet.', en: 'Could not add the domain.' },
    )
  }

  const verify = (domain: string) =>
    void run(
      `verify:${domain}`,
      async () => {
        const result = await verifyOrgDomain(orgId(), domain)
        if (result.status === 'verified') {
          setNotice(i18n.tr(`${domain} er verifisert.`, `${domain} is verified.`))
        } else if (result.reason) {
          const copy = REASON_COPY[result.reason]
          setProblem(copy ? i18n.tr(copy.no, copy.en) : i18n.tr('Domenet ble ikke verifisert.', 'The domain was not verified.'))
        }
      },
      { no: 'Kunne ikke verifisere domenet.', en: 'Could not verify the domain.' },
    )

  const revoke = (domain: string) =>
    void run(
      `revoke:${domain}`,
      async () => {
        await revokeOrgDomain(orgId(), domain)
      },
      { no: 'Kunne ikke fjerne domenet.', en: 'Could not remove the domain.' },
    )

  return (
    <FeaturePanel
      title={i18n.tr('Verifiserte domener', 'Verified domains')}
      description={i18n.tr(
        'Bevis eierskap til selskapets domene med en DNS TXT-oppføring.',
        'Prove ownership of your company domain with a DNS TXT record.',
      )}
      actionLabel={i18n.tr('Legg til domene', 'Add domain')}
      onAction={live() ? () => setAdding((open) => !open) : undefined}
    >
      <Show
        when={live()}
        fallback={
          <p class="verevon-settings-panel-note">
            {i18n.tr('Ingen domener er verifisert for denne organisasjonen.', 'No domains have been verified for this organization.')}
          </p>
        }
      >
        <Show when={adding()}>
          <form
            class="verevon-settings-field-grid verevon-settings-field-grid--spaced"
            data-testid="domains-add-form"
            onSubmit={(event) => {
              event.preventDefault()
              add()
            }}
          >
            <SettingsField
              id="org-domain-new"
              label={i18n.tr('Domene', 'Domain')}
              placeholder="example.com"
              value={draft()}
              onInput={(event) => setDraft(event.currentTarget.value)}
            />
            <span>
              <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={busy() !== null || !draft().trim()}>
                {i18n.tr('Legg til', 'Add')}
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
        <Show when={notice()}>
          {(text) => (
            <p class="verevon-settings-status-message" role="status">
              {text()}
            </p>
          )}
        </Show>

        <Show
          when={rows().length > 0}
          fallback={
            <p class="verevon-settings-panel-note" role="status" data-testid="domains-empty">
              {domains.loading
                ? i18n.tr('Laster domener …', 'Loading domains...')
                : i18n.tr('Ingen domener er lagt til for denne organisasjonen.', 'No domains have been added for this organization.')}
            </p>
          }
        >
          <div class="verevon-settings-row-divider" data-testid="domains-rows">
            <For each={rows()}>
              {(row) => (
                <>
                  <DataRow
                    primary={row.domain}
                    secondary={
                      row.status === 'verified'
                        ? i18n.tr('Verifisert', 'Verified')
                        : i18n.tr('Venter på DNS-bevis', 'Waiting for DNS proof')
                    }
                    meta={row.verified_at ? new Date(row.verified_at).toLocaleDateString() : ''}
                  />
                  <Show when={row.status === 'pending' && row.challenge}>
                    {(challenge) => (
                      <div class="verevon-settings-panel-note" data-testid={`domain-challenge-${row.domain}`}>
                        <p>
                          {i18n.tr(
                            'Legg inn denne TXT-oppføringen hos DNS-leverandøren din, og trykk Verifiser.',
                            'Publish this TXT record with your DNS provider, then press Verify.',
                          )}
                        </p>
                        <p>
                          <strong>{i18n.tr('Navn', 'Name')}:</strong> <code>{challenge().name}</code>
                        </p>
                        <p>
                          <strong>{i18n.tr('Verdi', 'Value')}:</strong> <code>{challenge().value}</code>
                        </p>
                        <p>
                          {i18n.tr('Gyldig til', 'Valid until')} {new Date(challenge().expires_at).toLocaleDateString()}
                        </p>
                      </div>
                    )}
                  </Show>
                  <p>
                    <Show when={row.status === 'pending'}>
                      <SettingsButton
                        type="button"
                        variant="primary"
                        settingsSize="sm"
                        disabled={busy() !== null}
                        data-testid={`domain-verify-${row.domain}`}
                        onClick={() => verify(row.domain)}
                      >
                        {i18n.tr('Verifiser', 'Verify')}
                      </SettingsButton>{' '}
                    </Show>
                    <SettingsButton
                      type="button"
                      settingsSize="sm"
                      disabled={busy() !== null}
                      data-testid={`domain-revoke-${row.domain}`}
                      onClick={() => revoke(row.domain)}
                    >
                      {i18n.tr('Fjern', 'Remove')}
                    </SettingsButton>
                  </p>
                </>
              )}
            </For>
          </div>
        </Show>
      </Show>
    </FeaturePanel>
  )
}
