import { createResource } from '@/shared/lib/create-resource-compat'
import { createSignal, Show } from 'solid-js'
import { Check, Copy, Mail, RefreshCw } from '@/shared/icons'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import {
  createMailTaskAddress,
  MailTaskNotConfiguredError,
  regenerateMailTaskAddress,
} from '@/shared/api/mail-task-client'
import { useI18n } from '@/shared/i18n'

/**
 * Personal inbound-mail alias (chat-wave2 mail-task spec §3.6, owner ruling
 * 2026-09-28: Postmark inbound, self-forward only). Forwarding an email to
 * this address, from the account's OWN verified email only, starts a new
 * chat thread with a drafted answer.
 *
 * Off by default: the gateway 503s every mail-task route until the owner
 * finishes the Postmark account + DNS setup
 * (user-core's MAIL_TASK_RECEIVING_DOMAIN). This section renders that as an
 * honest "not available yet" state rather than a generic error.
 */
export function MailTaskAddressSection() {
  const i18n = useI18n()
  const [copied, setCopied] = createSignal(false)
  const [regenerating, setRegenerating] = createSignal(false)
  const [actionError, setActionError] = createSignal<string | null>(null)

  const [address, { refetch, mutate }] = createResource(() =>
    createMailTaskAddress().then((result) => result.address),
  )

  const notConfigured = () => address.error instanceof MailTaskNotConfiguredError

  const copyAddress = async () => {
    const value = address()
    if (!value) return
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1800)
    } catch {
      setActionError(
        i18n.tr(
          'Kunne ikke kopiere til utklippstavlen. Merk og kopier adressen manuelt.',
          'Could not copy to clipboard. Select and copy the address manually.',
        ),
      )
    }
  }

  const regenerate = async () => {
    if (regenerating()) return
    if (
      typeof window !== 'undefined' &&
      !window.confirm(
        i18n.tr(
          'Generere en ny adresse? Den gamle slutter å virke med én gang.',
          'Generate a new address? The old one stops working immediately.',
        ),
      )
    ) {
      return
    }
    setRegenerating(true)
    setActionError(null)
    try {
      const result = await regenerateMailTaskAddress()
      mutate(result.address)
    } catch (error) {
      if (error instanceof MailTaskNotConfiguredError) {
        await refetch()
      } else {
        setActionError(
          i18n.tr('Kunne ikke generere en ny adresse.', 'Could not generate a new address.'),
        )
      }
    } finally {
      setRegenerating(false)
    }
  }

  return (
    <section id="mail-task" class="verevon-settings-section">
      <SectionHeader
        title={i18n.tr('E-post til oppgave', 'Email to task')}
        description={i18n.tr(
          'Videresend en e-post til denne adressen for å starte en ny samtale med et utkast til svar. Bare e-post fra din egen verifiserte adresse godtas.',
          'Forward an email to this address to start a new chat thread with a drafted answer. Only email from your own verified address is accepted.',
        )}
      />

      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show
        when={!address.loading}
        fallback={
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            {i18n.tr('Laster adresse…', 'Loading address…')}
          </p>
        }
      >
        <Show
          when={!notConfigured()}
          fallback={
            <div class="verevon-settings-list-card">
              <p class="verevon-settings-empty-row">
                {i18n.tr(
                  'E-post til oppgave er ikke tilgjengelig ennå for denne organisasjonen.',
                  'Email to task is not available yet for this organization.',
                )}
              </p>
            </div>
          }
        >
          <Show
            when={address() && !address.error}
            fallback={
              <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                {i18n.tr('Kunne ikke laste adressen.', 'Could not load the address.')}
              </p>
            }
          >
            <div class="verevon-settings-list-card">
              <div class="verevon-settings-list-row">
                <div>
                  <p>
                    <Mail class="size-4" aria-hidden="true" /> {address()}
                  </p>
                  <span>
                    {i18n.tr(
                      'Kun e-post fra din egen verifiserte adresse godtas — alt annet forkastes stille.',
                      'Only email from your own verified address is accepted — everything else is silently dropped.',
                    )}
                  </span>
                </div>
                <div>
                  <SettingsButton settingsSize="sm" onClick={copyAddress}>
                    <Show when={copied()} fallback={<Copy class="size-4" aria-hidden="true" />}>
                      <Check class="size-4" aria-hidden="true" />
                    </Show>
                    {copied()
                      ? i18n.tr('Kopiert', 'Copied')
                      : i18n.tr('Kopier', 'Copy')}
                  </SettingsButton>{' '}
                  <SettingsButton settingsSize="sm" onClick={regenerate} disabled={regenerating()}>
                    <RefreshCw class="size-4" aria-hidden="true" />
                    {regenerating()
                      ? i18n.tr('Genererer …', 'Generating…')
                      : i18n.tr('Generer på nytt', 'Regenerate')}
                  </SettingsButton>
                </div>
              </div>
            </div>
          </Show>
        </Show>
      </Show>
    </section>
  )
}
