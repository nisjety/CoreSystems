import { Building2, CheckCircle2, ChevronDown, Eye, KeyRound, Lock, Mail, Phone, UserRound } from 'lucide-solid'
import { createMemo, createSignal, For, onCleanup, onMount, Show, type Accessor } from 'solid-js'
import type { AuthCopy, AuthMode, Locale } from '@/features/auth/lib/model'
import type { SocialProvider } from '@/features/auth/lib/model'
import { TurnstileChallenge } from '@/features/auth/components/sections/TurnstileChallenge'
import { Button } from '@/shared/ui/Button'
import { VerevonBackButton } from '@/shared/ui/verevon/VerevonBackButton'
import { VerevonField } from '@/shared/ui/verevon/VerevonField'
import { VerevonIconButton } from '@/shared/ui/verevon/VerevonIconButton'
import { VerevonLanguageButton } from '@/shared/ui/verevon/VerevonLanguageButton'
import { VerevonProviderButton } from '@/shared/ui/verevon/VerevonProviderButton'
import { useI18n } from '@/shared/i18n'

const PHONE_COUNTRY_OPTIONS = [
  { iso: 'NO', dialCode: '+47', nameNb: 'Norge', nameEn: 'Norway', placeholder: '123 45 678' },
  { iso: 'SE', dialCode: '+46', nameNb: 'Sverige', nameEn: 'Sweden', placeholder: '70 123 45 67' },
  { iso: 'DK', dialCode: '+45', nameNb: 'Danmark', nameEn: 'Denmark', placeholder: '20 12 34 56' },
  { iso: 'FI', dialCode: '+358', nameNb: 'Finland', nameEn: 'Finland', placeholder: '40 123 4567' },
  { iso: 'US', dialCode: '+1', nameNb: 'USA', nameEn: 'United States', placeholder: '555 010 1234' },
  { iso: 'GB', dialCode: '+44', nameNb: 'Storbritannia', nameEn: 'United Kingdom', placeholder: '7400 123456' },
  { iso: 'DE', dialCode: '+49', nameNb: 'Tyskland', nameEn: 'Germany', placeholder: '151 23456789' },
  { iso: 'FR', dialCode: '+33', nameNb: 'Frankrike', nameEn: 'France', placeholder: '6 12 34 56 78' },
] as const

type AuthFormPanelProps = {
  mode: Accessor<AuthMode>
  locale: Accessor<Locale>
  copy: Accessor<AuthCopy>
  email: Accessor<string>
  password: Accessor<string>
  name: Accessor<string>
  phoneCountryCode: Accessor<string>
  phoneNumber: Accessor<string>
  captchaSiteKey: Accessor<string>
  showPassword: Accessor<boolean>
  submitting: Accessor<boolean>
  contentHeight?: number
  passkeyEnabled?: boolean
  socialProviders: readonly SocialProvider[]
  onSocialSignIn: (provider: SocialProvider) => void
  ssoEmail: Accessor<string>
  ssoSubmitting: Accessor<boolean>
  onSsoEmailInput: (value: string) => void
  onSsoSignIn: () => void
  onSetMode: (mode: AuthMode) => void
  onToggleLocale: () => void
  onEmailInput: (value: string) => void
  onPasswordInput: (value: string) => void
  onNameInput: (value: string) => void
  onPhoneCountryChange: (dialCode: string) => void
  onPhoneInput: (value: string) => void
  onCaptchaToken: (value: string) => void
  onTogglePassword: () => void
  onForgotPassword: () => void
  onCompleteAuth: () => void
  onSubmit: (event: SubmitEvent) => void
  onContentRef: (element: HTMLDivElement) => void
}

export function AuthFormPanel(props: AuthFormPanelProps) {
  const i18n = useI18n()
  const selectedPhoneCountry = createMemo(() => (
    PHONE_COUNTRY_OPTIONS.find((country) => country.dialCode === props.phoneCountryCode()) ?? PHONE_COUNTRY_OPTIONS[0]
  ))

  const primaryActionLabel = () => {
    if (!props.submitting()) return props.copy().primaryAction
    if (props.mode() === 'signin') return i18n.tr('Logger inn …', 'Signing in...')
    return i18n.tr('Oppretter konto …', 'Creating account...')
  }

  return (
    <div class="auth-card__content">
      <div class="auth-card__topbar">
        <VerevonBackButton href="/" label={props.copy().back} class="auth-back-link" />
        <div class="auth-topbar__actions">
          <AuthLanguageMenu
            locale={props.locale()}
            onSelect={(locale) => {
              if (locale !== props.locale()) props.onToggleLocale()
            }}
          />
          <span class="auth-topbar__dot" aria-hidden="true" />
        </div>
      </div>

      <div class="auth-tabs" role="tablist" aria-label={i18n.tr('Autentiseringsmodus', 'Authentication mode')}>
        <button
          type="button"
          class={`auth-tab ${props.mode() === 'signin' ? 'auth-tab--active' : ''}`}
          onClick={() => props.onSetMode('signin')}
        >
          {props.copy().signin}
        </button>
        <button
          type="button"
          class={`auth-tab ${props.mode() === 'signup' ? 'auth-tab--active' : ''}`}
          onClick={() => props.onSetMode('signup')}
        >
          {props.copy().signup}
        </button>
      </div>

      <div class="auth-hero-copy">
        <h1>{props.copy().title}</h1>
        <p>{props.copy().description}</p>
      </div>

      <div
        class="auth-animated-content"
        style={{
          height: props.contentHeight ? `${props.contentHeight}px` : undefined,
        }}
      >
        <div ref={props.onContentRef} class="auth-animated-content__inner">
          <form class="auth-form" onSubmit={(event) => props.onSubmit(event)}>
            <Show when={props.mode() === 'signup'}>
              <VerevonField class="auth-field" label={props.copy().nameLabel}>
                <div class="auth-field__control">
                  <UserRound size={18} />
                  <input
                    value={props.name()}
                    onInput={(event) => props.onNameInput(event.currentTarget.value)}
                    type="text"
                    autocomplete="name"
                    placeholder={i18n.tr('Ola Nordmann', 'Jane Doe')}
                  />
                </div>
              </VerevonField>
            </Show>

            <VerevonField class="auth-field" label={props.copy().emailLabel}>
              <div class="auth-field__control">
                <Mail size={18} />
                <input
                  value={props.email()}
                  onInput={(event) => props.onEmailInput(event.currentTarget.value)}
                  type="email"
                  autocomplete="email"
                  placeholder={i18n.tr('navn@eksempel.no', 'name@example.com')}
                />
              </div>
            </VerevonField>

            <Show when={props.mode() === 'signup'}>
              <div class="auth-field">
                <label for="auth-phone-number">{i18n.tr('Telefonnummer *', 'Phone number *')}</label>
                <div class="auth-field__control auth-field__control--phone">
                  <PhoneCountryMenu
                    locale={props.locale()}
                    selected={selectedPhoneCountry()}
                    value={props.phoneCountryCode()}
                    onChange={props.onPhoneCountryChange}
                  />
                  <span class="auth-phone-prefix" aria-hidden="true">{props.phoneCountryCode()}</span>
                  <input
                    id="auth-phone-number"
                    value={props.phoneNumber()}
                    onInput={(event) => props.onPhoneInput(event.currentTarget.value)}
                    type="tel"
                    inputmode="tel"
                    autocomplete="tel"
                    placeholder={selectedPhoneCountry().placeholder}
                  />
                </div>
              </div>
            </Show>

            <VerevonField class="auth-field" label={props.copy().passwordLabel}>
              <div class="auth-field__control">
                <Lock size={18} />
                <input
                  value={props.password()}
                  onInput={(event) => props.onPasswordInput(event.currentTarget.value)}
                  type={props.showPassword() ? 'text' : 'password'}
                  autocomplete={props.mode() === 'signin' ? 'current-password' : 'new-password'}
                  placeholder="••••••••"
                />
                <VerevonIconButton
                  tone="ghost"
                  size="sm"
                  shape="rounded"
                  class="auth-field__toggle"
                  onClick={() => props.onTogglePassword()}
                  aria-label={i18n.tr('Vis/skjul passord', 'Toggle password visibility')}
                >
                  <Eye size={18} />
                </VerevonIconButton>
              </div>
            </VerevonField>

            <Show when={props.mode() === 'signin'}>
              <button
                type="button"
                class="auth-forgot-password"
                disabled={props.submitting()}
                onClick={() => props.onForgotPassword()}
              >
                {i18n.tr('Glemt passord?', 'Forgot password?')}
              </button>
            </Show>

            <Show when={props.mode() === 'signup' && props.captchaSiteKey()}>
              {(siteKey) => (
                <TurnstileChallenge
                  siteKey={siteKey()}
                  locale={props.locale()}
                  onToken={props.onCaptchaToken}
                />
              )}
            </Show>

            <Button type="submit" variant="primary" size="lg" fullWidth disabled={props.submitting()}>
              {primaryActionLabel()}
            </Button>
          </form>

          <div class="auth-divider" aria-hidden="true">
            <span>{props.copy().divider}</span>
          </div>

          <div class="auth-social-row">
            <For each={props.socialProviders}>
              {(provider) => (
                <VerevonProviderButton
                  label={provider.name}
                  icon={provider.icon}
                  active={provider.active}
                  onClick={() => props.onSocialSignIn(provider)}
                />
              )}
            </For>
          </div>

          <Show when={props.mode() === 'signin'}>
            <form
              class="auth-sso"
              onSubmit={(event) => {
                event.preventDefault()
                props.onSsoSignIn()
              }}
            >
              <VerevonField class="auth-field" label={props.copy().ssoTitle}>
                <div class="auth-field__control">
                  <Building2 size={18} />
                  <input
                    value={props.ssoEmail()}
                    onInput={(event) => props.onSsoEmailInput(event.currentTarget.value)}
                    type="email"
                    autocomplete="email"
                    inputmode="email"
                    placeholder={props.copy().ssoPlaceholder}
                  />
                </div>
              </VerevonField>
              <Button
                type="submit"
                variant="secondary"
                size="lg"
                fullWidth
                disabled={props.ssoSubmitting() || props.ssoEmail().trim().length === 0}
              >
                <span>{props.ssoSubmitting() ? props.copy().ssoSubmitting : props.copy().ssoAction}</span>
              </Button>
            </form>
          </Show>

          <Button
            type="button"
            variant="secondary"
            size="lg"
            fullWidth
            disabled={!props.passkeyEnabled}
            onClick={() => {
              if (props.passkeyEnabled) props.onCompleteAuth()
            }}
          >
            <KeyRound size={16} />
            <span>{props.copy().passkey}</span>
          </Button>

          <p class="auth-terms">{props.copy().terms}</p>
        </div>
      </div>

      <p class="auth-support">{props.copy().support}</p>
    </div>
  )
}

type PhoneCountryOption = (typeof PHONE_COUNTRY_OPTIONS)[number]

function AuthLanguageMenu(props: {
  locale: Locale
  onSelect: (locale: Locale) => void
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  let rootRef: HTMLDivElement | undefined
  const options: Array<{ code: Locale; label: string; shortLabel: string }> = [
    { code: 'nb', label: 'Norsk bokmal', shortLabel: 'NB' },
    { code: 'en', label: 'English', shortLabel: 'EN' },
  ]

  useAuthDropdownDismiss(() => rootRef, () => setOpen(false))

  return (
    <div ref={rootRef} class="auth-language-menu">
      <VerevonLanguageButton
        code={props.locale.toUpperCase()}
        class="auth-language-switcher"
        onClick={() => setOpen((current) => !current)}
        ariaLabel={i18n.tr('Bytt språk', 'Switch language')}
        ariaExpanded={open()}
        ariaControls={open() ? 'auth-language-menu' : undefined}
      />
      <Show when={open()}>
        <menu id="auth-language-menu" class="verevon-popover auth-dropdown-menu auth-language-menu__menu" aria-label={i18n.tr('Språkvalg', 'Language options')}>
          <For each={options}>
            {(option) => {
              const selected = () => option.code === props.locale
              return (
                <li role="presentation">
                  <button
                    type="button"
                    role="menuitemradio"
                    aria-checked={selected()}
                    class="auth-dropdown-option"
                    classList={{ 'auth-dropdown-option--selected': selected() }}
                    onClick={() => {
                      props.onSelect(option.code)
                      setOpen(false)
                    }}
                  >
                    <span class="auth-dropdown-option__label">{option.label}</span>
                    <span class="auth-dropdown-option__meta">{option.shortLabel}</span>
                    <Show when={selected()}>
                      <CheckCircle2 class="auth-dropdown-option__check" size={14} />
                    </Show>
                  </button>
                </li>
              )
            }}
          </For>
        </menu>
      </Show>
    </div>
  )
}

function PhoneCountryMenu(props: {
  locale: Locale
  selected: PhoneCountryOption
  value: string
  onChange: (dialCode: string) => void
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  let rootRef: HTMLDivElement | undefined

  useAuthDropdownDismiss(() => rootRef, () => setOpen(false))

  const countryName = (country: PhoneCountryOption) => props.locale === 'nb' ? country.nameNb : country.nameEn

  return (
    <div ref={rootRef} class="auth-phone-country">
      <button
        type="button"
        class="auth-phone-country__chrome"
        aria-label={i18n.tr('Landskode', 'Country code')}
        aria-haspopup="menu"
        aria-expanded={open()}
        aria-controls={open() ? 'auth-phone-country-menu' : undefined}
        onClick={() => setOpen((current) => !current)}
      >
        <Phone size={14} />
        <span>{props.selected.iso}</span>
        <ChevronDown size={12} class={open() ? 'rotate-180' : undefined} />
      </button>
      <Show when={open()}>
        <menu id="auth-phone-country-menu" class="verevon-popover auth-dropdown-menu auth-phone-country__menu" aria-label={i18n.tr('Landskode', 'Country code')}>
          <For each={PHONE_COUNTRY_OPTIONS}>
            {(country) => {
              const selected = () => country.dialCode === props.value
              return (
                <li role="presentation">
                  <button
                    type="button"
                    role="menuitemradio"
                    aria-checked={selected()}
                    class="auth-dropdown-option"
                    classList={{ 'auth-dropdown-option--selected': selected() }}
                    onClick={() => {
                      props.onChange(country.dialCode)
                      setOpen(false)
                    }}
                  >
                    <span class="auth-dropdown-option__label">{countryName(country)}</span>
                    <span class="auth-dropdown-option__meta">{country.iso} {country.dialCode}</span>
                    <Show when={selected()}>
                      <CheckCircle2 class="auth-dropdown-option__check" size={14} />
                    </Show>
                  </button>
                </li>
              )
            }}
          </For>
        </menu>
      </Show>
    </div>
  )
}

function useAuthDropdownDismiss(root: () => HTMLDivElement | undefined, close: () => void) {
  onMount(() => {
    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (root()?.contains(event.target as Node)) return
      close()
    }
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') close()
    }

    document.addEventListener('pointerdown', closeOnOutsidePointer, true)
    window.addEventListener('keydown', closeOnEscape)
    onCleanup(() => {
      document.removeEventListener('pointerdown', closeOnOutsidePointer, true)
      window.removeEventListener('keydown', closeOnEscape)
    })
  })
}
