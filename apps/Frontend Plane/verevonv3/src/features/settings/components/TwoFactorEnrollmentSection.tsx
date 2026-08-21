import { Check, Copy, Download, ShieldCheck } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import {
  enableTwoFactor,
  regenerateBackupCodes,
  verifyTotpEnrollment,
} from '@/shared/api/auth-client'
import { ApiError } from '@/shared/api/http'
import {
  SectionHeader,
  SettingsButton,
  SettingsField,
} from '@/features/settings/components/settings-ui'
import { useI18n } from '@/shared/i18n'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'

/**
 * Two-factor (TOTP) enrollment.
 *
 * Step 1 (password): Better Auth requires re-auth to enable 2FA. Submitting the
 *   password starts enrollment and returns the otpauth URI + initial backup codes.
 * Step 2 (scan + confirm): we render the otpauth URI as a manual setup key (no QR
 *   library is bundled, so we surface the secret + full URI to copy into an
 *   authenticator), then verify a TOTP code to activate the factor.
 * Step 3 (backup codes): show the recovery codes once and let the user download them.
 *
 * The existing verify-at-login flow is untouched; this only adds enrollment.
 */

type EnrollmentStep = 'idle' | 'scan' | 'done'

/** Extract the human-enterable secret from an otpauth:// URI for manual setup. */
function secretFromUri(uri: string): string {
  if (!uri) return ''
  const match = uri.match(/[?&]secret=([^&]+)/i)
  return match?.[1] ? decodeURIComponent(match[1]) : ''
}

function errorMessage(err: unknown, fallback: string): string {
  if (err instanceof ApiError) return err.message
  if (err instanceof Error) return err.message
  return fallback
}

export function TwoFactorEnrollmentSection() {
  const i18n = useI18n()
  const [step, setStep] = createSignal<EnrollmentStep>('idle')
  const [password, setPassword] = createSignal('')
  const [totpUri, setTotpUri] = createSignal('')
  const [code, setCode] = createSignal('')
  const [backupCodes, setBackupCodes] = createSignal<string[]>([])
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [copied, setCopied] = createSignal(false)

  const secret = createMemo(() => secretFromUri(totpUri()))

  const begin = async (event: SubmitEvent) => {
    event.preventDefault()
    if (busy() || password().length === 0) return
    setBusy(true)
    setError(null)
    try {
      const enrollment = await enableTwoFactor({ password: password() })
      if (!enrollment.totpURI) {
        setError(i18n.tr('Registrering av tofaktorautentisering returnerte ingen oppsettsnøkkel. Prøv igjen.', 'Two-factor enrollment did not return a setup key. Try again.'))
        return
      }
      setTotpUri(enrollment.totpURI)
      setBackupCodes(enrollment.backupCodes)
      setStep('scan')
    } catch (err) {
      setError(errorMessage(err, i18n.tr('Kunne ikke starte registrering av tofaktorautentisering.', 'Could not start two-factor enrollment.')))
    } finally {
      setBusy(false)
    }
  }

  const confirm = async (event: SubmitEvent) => {
    event.preventDefault()
    const trimmed = code().replace(/\s/g, '')
    if (busy() || trimmed.length === 0) return
    setBusy(true)
    setError(null)
    try {
      await verifyTotpEnrollment(trimmed)
      // Refresh backup codes if enable didn't return them, so the user always
      // leaves enrollment with a usable recovery set.
      if (backupCodes().length === 0) {
        try {
          const codes = await regenerateBackupCodes({ password: password() })
          setBackupCodes(codes)
        } catch {
          // Non-fatal: 2FA is active; the user can regenerate codes later.
        }
      }
      setStep('done')
    } catch (err) {
      setError(errorMessage(err, i18n.tr('Koden stemte ikke. Sjekk autentiseringsappen din og prøv igjen.', 'That code did not match. Check your authenticator and retry.')))
    } finally {
      setBusy(false)
    }
  }

  const reset = () => {
    setStep('idle')
    setPassword('')
    setTotpUri('')
    setCode('')
    setBackupCodes([])
    setError(null)
    setCopied(false)
  }

  const copySecret = async () => {
    const value = secret() || totpUri()
    if (!value) return
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1800)
    } catch {
      setError(i18n.tr('Kunne ikke kopiere til utklippstavlen. Merk og kopier nøkkelen manuelt.', 'Could not copy to clipboard. Select and copy the key manually.'))
    }
  }

  const downloadBackupCodes = () => {
    const blob = new Blob([`${backupCodes().join('\n')}\n`], { type: 'text/plain' })
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = 'verevon-backup-codes.txt'
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    URL.revokeObjectURL(url)
  }

  return (
    <section id="two-factor" class="verevon-settings-section">
      <SectionHeader
        title={i18n.tr('Tofaktorautentisering', 'Two-factor authentication')}
        description={i18n.tr(
          'Legg til en tidsbasert engangskode (TOTP) fra en autentiseringsapp for å beskytte innloggingen.',
          'Add a time-based one-time code (TOTP) from an authenticator app to protect sign-in.',
        )}
      />

      <Show when={error()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={step() === 'idle'}>
        <form class="verevon-settings-list-card verevon-twofa-step" onSubmit={begin}>
          <div class="verevon-settings-list-row">
            <div>
              <p>{i18n.tr('Autentiseringsapp', 'Authenticator app')}</p>
              <span>{i18n.tr('Bekreft passordet ditt for å generere en oppsettsnøkkel for autentiseringsappen din.', 'Confirm your password to generate a setup key for your authenticator.')}</span>
            </div>
            <ShieldCheck class="size-5" aria-hidden="true" />
          </div>
          <div class="verevon-twofa-form-row">
            <label for="twofa-password" class="verevon-settings-field">
              <span class="verevon-settings-label">{i18n.tr('Kontopassord', 'Account password')}</span>
              <span class="verevon-settings-input-wrap">
                <VerevonInput
                  id="twofa-password"
                  type="password"
                  autocomplete="current-password"
                  value={password()}
                  onInput={(event) => setPassword(event.currentTarget.value)}
                  class="verevon-settings-input"
                />
              </span>
            </label>
            <SettingsButton
              type="submit"
              variant="primary"
              settingsSize="sm"
              disabled={busy() || password().length === 0}
            >
              {busy() ? i18n.tr('Starter…', 'Starting…') : i18n.tr('Start oppsett', 'Begin setup')}
            </SettingsButton>
          </div>
        </form>
      </Show>

      <Show when={step() === 'scan'}>
        <div class="verevon-settings-list-card verevon-twofa-step">
          <div class="verevon-twofa-scan">
            <p class="verevon-twofa-lead">
              {i18n.tr(
                'Legg til denne kontoen i autentiseringsappen din, og skriv deretter inn den 6-sifrede koden den viser.',
                'Add this account to your authenticator app, then enter the 6-digit code it shows.',
              )}
            </p>
            <div class="verevon-twofa-key">
              <span class="verevon-twofa-key__label">{i18n.tr('Oppsettsnøkkel', 'Setup key')}</span>
              <code class="verevon-twofa-key__value">{secret() || totpUri()}</code>
              <SettingsButton settingsSize="xs" onClick={() => void copySecret()}>
                <Show when={copied()} fallback={<><Copy size={13} aria-hidden="true" /> {i18n.tr('Kopier', 'Copy')}</>}>
                  <Check size={13} aria-hidden="true" /> {i18n.tr('Kopiert', 'Copied')}
                </Show>
              </SettingsButton>
            </div>
            <p class="verevon-settings-subnote">
              {i18n.tr(
                'Ingen QR-skanner her ennå — lim inn oppsettsnøkkelen i autentiseringsappens alternativ for manuell nøkkelinnføring, eller åpne otpauth-lenken på enheten der appen er installert.',
                'No QR scanner here yet — paste the setup key into your authenticator\'s "enter key manually" option, or open the otpauth link on the device with the app installed.',
              )}
            </p>
            <a class="verevon-twofa-otpauth" href={totpUri()}>
              {i18n.tr('Åpne i autentiseringsapp', 'Open in authenticator')}
            </a>
          </div>

          <form class="verevon-twofa-form-row" onSubmit={confirm}>
            <SettingsField
              id="twofa-code"
              label={i18n.tr('6-sifret kode', '6-digit code')}
              type="text"
              value={code()}
              onInput={(event) => setCode(event.currentTarget.value)}
              placeholder="123456"
            />
            <div class="verevon-twofa-actions">
              <SettingsButton
                type="submit"
                variant="primary"
                settingsSize="sm"
                disabled={busy() || code().replace(/\s/g, '').length === 0}
              >
                {busy() ? i18n.tr('Verifiserer…', 'Verifying…') : i18n.tr('Bekreft og aktiver', 'Confirm & enable')}
              </SettingsButton>
              <SettingsButton settingsSize="sm" onClick={reset} disabled={busy()}>
                {i18n.tr('Avbryt', 'Cancel')}
              </SettingsButton>
            </div>
          </form>
        </div>
      </Show>

      <Show when={step() === 'done'}>
        <div class="verevon-settings-list-card verevon-twofa-step">
          <div class="verevon-settings-list-row">
            <div>
              <p>{i18n.tr('Tofaktorautentisering er på', 'Two-factor authentication is on')}</p>
              <span>{i18n.tr('Du vil bli bedt om en kode fra autentiseringsappen din ved hver innlogging.', "You'll be asked for a code from your authenticator at each sign-in.")}</span>
            </div>
            <ShieldCheck class="size-5" aria-hidden="true" />
          </div>

          <Show when={backupCodes().length > 0}>
            <div class="verevon-twofa-backup">
              <div class="verevon-twofa-backup__header">
                <div>
                  <p>{i18n.tr('Reservekoder', 'Backup codes')}</p>
                  <span>{i18n.tr('Oppbevar disse på et trygt sted. Hver kode virker én gang hvis du mister enheten din.', 'Store these somewhere safe. Each code works once if you lose your device.')}</span>
                </div>
                <SettingsButton settingsSize="sm" onClick={downloadBackupCodes}>
                  <Download size={14} aria-hidden="true" /> {i18n.tr('Last ned', 'Download')}
                </SettingsButton>
              </div>
              <ul class="verevon-twofa-codes">
                <For each={backupCodes()}>{(backupCode) => <li>{backupCode}</li>}</For>
              </ul>
            </div>
          </Show>
        </div>
      </Show>
    </section>
  )
}
