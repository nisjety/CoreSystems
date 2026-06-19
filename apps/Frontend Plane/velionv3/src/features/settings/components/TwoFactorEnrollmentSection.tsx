import { Check, Copy, Download, ShieldCheck } from 'lucide-solid'
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
        setError('Two-factor enrollment did not return a setup key. Try again.')
        return
      }
      setTotpUri(enrollment.totpURI)
      setBackupCodes(enrollment.backupCodes)
      setStep('scan')
    } catch (err) {
      setError(errorMessage(err, 'Could not start two-factor enrollment.'))
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
      setError(errorMessage(err, 'That code did not match. Check your authenticator and retry.'))
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
      setError('Could not copy to clipboard. Select and copy the key manually.')
    }
  }

  const downloadBackupCodes = () => {
    const blob = new Blob([`${backupCodes().join('\n')}\n`], { type: 'text/plain' })
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = 'velion-backup-codes.txt'
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    URL.revokeObjectURL(url)
  }

  return (
    <section id="two-factor" class="velion-settings-section">
      <SectionHeader
        title="Two-factor authentication"
        description="Add a time-based one-time code (TOTP) from an authenticator app to protect sign-in."
      />

      <Show when={error()}>
        {(message) => (
          <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={step() === 'idle'}>
        <form class="velion-settings-list-card velion-twofa-step" onSubmit={begin}>
          <div class="velion-settings-list-row">
            <div>
              <p>Authenticator app</p>
              <span>Confirm your password to generate a setup key for your authenticator.</span>
            </div>
            <ShieldCheck class="size-5" aria-hidden="true" />
          </div>
          <div class="velion-twofa-form-row">
            <SettingsField
              id="twofa-password"
              label="Account password"
              type="text"
              value={password()}
              onInput={(event) => setPassword(event.currentTarget.value)}
            />
            <SettingsButton
              type="submit"
              variant="primary"
              settingsSize="sm"
              disabled={busy() || password().length === 0}
            >
              {busy() ? 'Starting…' : 'Begin setup'}
            </SettingsButton>
          </div>
        </form>
      </Show>

      <Show when={step() === 'scan'}>
        <div class="velion-settings-list-card velion-twofa-step">
          <div class="velion-twofa-scan">
            <p class="velion-twofa-lead">
              Add this account to your authenticator app, then enter the 6-digit code it shows.
            </p>
            <div class="velion-twofa-key">
              <span class="velion-twofa-key__label">Setup key</span>
              <code class="velion-twofa-key__value">{secret() || totpUri()}</code>
              <SettingsButton settingsSize="xs" onClick={() => void copySecret()}>
                <Show when={copied()} fallback={<><Copy size={13} aria-hidden="true" /> Copy</>}>
                  <Check size={13} aria-hidden="true" /> Copied
                </Show>
              </SettingsButton>
            </div>
            <p class="velion-settings-subnote">
              No QR scanner here yet — paste the setup key into your authenticator's "enter key
              manually" option, or open the otpauth link on the device with the app installed.
            </p>
            <a class="velion-twofa-otpauth" href={totpUri()}>
              Open in authenticator
            </a>
          </div>

          <form class="velion-twofa-form-row" onSubmit={confirm}>
            <SettingsField
              id="twofa-code"
              label="6-digit code"
              type="text"
              value={code()}
              onInput={(event) => setCode(event.currentTarget.value)}
              placeholder="123456"
            />
            <div class="velion-twofa-actions">
              <SettingsButton
                type="submit"
                variant="primary"
                settingsSize="sm"
                disabled={busy() || code().replace(/\s/g, '').length === 0}
              >
                {busy() ? 'Verifying…' : 'Confirm & enable'}
              </SettingsButton>
              <SettingsButton settingsSize="sm" onClick={reset} disabled={busy()}>
                Cancel
              </SettingsButton>
            </div>
          </form>
        </div>
      </Show>

      <Show when={step() === 'done'}>
        <div class="velion-settings-list-card velion-twofa-step">
          <div class="velion-settings-list-row">
            <div>
              <p>Two-factor authentication is on</p>
              <span>You'll be asked for a code from your authenticator at each sign-in.</span>
            </div>
            <ShieldCheck class="size-5" aria-hidden="true" />
          </div>

          <Show when={backupCodes().length > 0}>
            <div class="velion-twofa-backup">
              <div class="velion-twofa-backup__header">
                <div>
                  <p>Backup codes</p>
                  <span>Store these somewhere safe. Each code works once if you lose your device.</span>
                </div>
                <SettingsButton settingsSize="sm" onClick={downloadBackupCodes}>
                  <Download size={14} aria-hidden="true" /> Download
                </SettingsButton>
              </div>
              <ul class="velion-twofa-codes">
                <For each={backupCodes()}>{(backupCode) => <li>{backupCode}</li>}</For>
              </ul>
            </div>
          </Show>
        </div>
      </Show>
    </section>
  )
}
