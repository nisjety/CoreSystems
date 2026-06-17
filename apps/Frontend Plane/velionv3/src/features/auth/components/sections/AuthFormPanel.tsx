import { Eye, KeyRound, Lock, Mail, UserRound } from 'lucide-solid'
import { For, Show, type Accessor } from 'solid-js'
import type { AuthCopy, AuthMode, Locale } from '@/features/auth/lib/model'
import type { SocialProvider } from '@/features/auth/lib/model'
import { Button } from '@/shared/ui/Button'
import { VelionBackButton } from '@/shared/ui/velion/VelionBackButton'
import { VelionField } from '@/shared/ui/velion/VelionField'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'
import { VelionLanguageButton } from '@/shared/ui/velion/VelionLanguageButton'
import { VelionProviderButton } from '@/shared/ui/velion/VelionProviderButton'

type AuthFormPanelProps = {
  mode: Accessor<AuthMode>
  locale: Accessor<Locale>
  copy: Accessor<AuthCopy>
  email: Accessor<string>
  password: Accessor<string>
  name: Accessor<string>
  showPassword: Accessor<boolean>
  contentHeight?: number
  passkeyEnabled?: boolean
  socialProviders: readonly SocialProvider[]
  onSocialSignIn: (provider: SocialProvider) => void
  onSetMode: (mode: AuthMode) => void
  onToggleLocale: () => void
  onEmailInput: (value: string) => void
  onPasswordInput: (value: string) => void
  onNameInput: (value: string) => void
  onTogglePassword: () => void
  onCompleteAuth: () => void
  onSubmit: (event: SubmitEvent) => void
  onContentRef: (element: HTMLDivElement) => void
}

export function AuthFormPanel(props: AuthFormPanelProps) {
  return (
    <div class="auth-card__content">
      <div class="auth-card__topbar">
        <VelionBackButton href="/" label={props.copy().back} class="auth-back-link" />
        <div class="auth-topbar__actions">
          <VelionLanguageButton
            code={props.locale().toUpperCase()}
            class="auth-language-switcher"
            onClick={() => props.onToggleLocale()}
            ariaLabel="Switch language"
          />
          <span class="auth-topbar__dot" aria-hidden="true" />
        </div>
      </div>

      <div class="auth-tabs" role="tablist" aria-label="Authentication mode">
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
              <VelionField class="auth-field" label={props.copy().nameLabel}>
                <div class="auth-field__control">
                  <UserRound size={18} />
                  <input
                    value={props.name()}
                    onInput={(event) => props.onNameInput(event.currentTarget.value)}
                    type="text"
                    autocomplete="name"
                    placeholder={props.locale() === 'nb' ? 'Ola Nordmann' : 'Jane Doe'}
                  />
                </div>
              </VelionField>
            </Show>

            <VelionField class="auth-field" label={props.copy().emailLabel}>
              <div class="auth-field__control">
                <Mail size={18} />
                <input
                  value={props.email()}
                  onInput={(event) => props.onEmailInput(event.currentTarget.value)}
                  type="email"
                  autocomplete="email"
                  placeholder={props.locale() === 'nb' ? 'navn@eksempel.no' : 'name@example.com'}
                />
              </div>
            </VelionField>

            <VelionField class="auth-field" label={props.copy().passwordLabel}>
              <div class="auth-field__control">
                <Lock size={18} />
                <input
                  value={props.password()}
                  onInput={(event) => props.onPasswordInput(event.currentTarget.value)}
                  type={props.showPassword() ? 'text' : 'password'}
                  autocomplete={props.mode() === 'signin' ? 'current-password' : 'new-password'}
                  placeholder="••••••••"
                />
                <VelionIconButton
                  tone="ghost"
                  size="sm"
                  shape="rounded"
                  class="auth-field__toggle"
                  onClick={() => props.onTogglePassword()}
                  aria-label="Toggle password visibility"
                >
                  <Eye size={18} />
                </VelionIconButton>
              </div>
            </VelionField>

            <Button type="submit" variant="primary" size="lg" fullWidth>
              {props.copy().primaryAction}
            </Button>
          </form>

          <div class="auth-divider" aria-hidden="true">
            <span>{props.copy().divider}</span>
          </div>

          <div class="auth-social-row">
            <For each={props.socialProviders}>
              {(provider) => (
                <VelionProviderButton
                  label={provider.name}
                  icon={provider.icon}
                  active={provider.active}
                  onClick={() => props.onSocialSignIn(provider)}
                />
              )}
            </For>
          </div>

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
