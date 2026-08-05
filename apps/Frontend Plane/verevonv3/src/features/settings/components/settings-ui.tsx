import { Check, ChevronDown, Info } from 'lucide-solid'
import { For, Show, splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { VerevonSelect } from '@/shared/ui/verevon/VerevonSelect'
import { VerevonSwitch } from '@/shared/ui/verevon/VerevonSwitch'
import { VerevonTextarea } from '@/shared/ui/verevon/VerevonTextarea'

export type StatusCard = { label: string; value: string; detail: string; tone?: 'ok' | 'warn' | 'neutral' }

export function SettingsSurface(props: {
  children: JSX.Element
  contentTestId?: string
  contentVariant?: 'account' | 'workspace'
  scrollAttribute?: boolean
}) {
  return (
    <div
      data-account-settings-scroll={props.scrollAttribute ? '' : undefined}
      class="verevon-settings-surface"
    >
      <div
        data-testid="settings-top-scroll-fade"
        aria-hidden="true"
        class="verevon-settings-fade verevon-settings-fade--top"
      />

      <div
        data-testid={props.contentTestId}
        class={cn(
          'verevon-settings-content',
          props.contentVariant === 'account' && 'verevon-settings-content--account',
        )}
      >
        <main class="verevon-settings-main">{props.children}</main>
      </div>

      <div
        data-testid="settings-bottom-scroll-fade"
        aria-hidden="true"
        class="verevon-settings-fade verevon-settings-fade--bottom"
      />
    </div>
  )
}

export function SettingsHero(props: {
  eyebrow: string
  title: string
  description?: string
  signals?: Array<{ label: string; value: string }>
}) {
  return (
    <div class="verevon-settings-hero">
      <p class="verevon-settings-eyebrow">{props.eyebrow}</p>
      <h1 class="verevon-settings-title">{props.title}</h1>
      {props.description ? (
        <p class="verevon-settings-hero-description">{props.description}</p>
      ) : null}
      {props.signals && props.signals.length > 0 ? (
        <dl class="verevon-settings-signal-grid">
          <For each={props.signals}>
            {(signal) => (
              <div class="verevon-settings-signal">
                <dt>{signal.label}</dt>
                <dd>{signal.value}</dd>
              </div>
            )}
          </For>
        </dl>
      ) : null}
    </div>
  )
}

export function SectionHeader(props: { title: string; description: string }) {
  return (
    <div class="verevon-settings-section-header">
      <h2>{props.title}</h2>
      <p>{props.description}</p>
    </div>
  )
}

export function SettingsField(props: {
  disabled?: boolean
  helpText?: string
  id: string
  label: string
  onInput?: JSX.EventHandler<HTMLInputElement, InputEvent>
  placeholder?: string
  prefix?: string
  readOnly?: boolean
  type?: 'email' | 'number' | 'tel' | 'text'
  value?: string
}) {
  const helpId = () => props.helpText ? `${props.id}-help` : undefined

  return (
    <label for={props.id} class="verevon-settings-field">
      <span class="verevon-settings-label">{props.label}</span>
      <span class="verevon-settings-input-wrap">
        {props.prefix ? <span class="verevon-settings-prefix">{props.prefix}</span> : null}
        <VerevonInput
          id={props.id}
          type={props.type ?? 'text'}
          value={props.value}
          disabled={props.disabled}
          readOnly={props.readOnly}
          placeholder={props.placeholder}
          onInput={props.onInput}
          aria-describedby={helpId()}
          class={cn('verevon-settings-input', props.prefix && 'verevon-settings-input--prefixed')}
        />
      </span>
      {props.helpText ? (
        <span id={helpId()} class="verevon-settings-help">
          {props.helpText}
        </span>
      ) : null}
    </label>
  )
}

export function SettingsTextarea(props: {
  id: string
  label: string
  onInput?: JSX.EventHandler<HTMLTextAreaElement, InputEvent>
  value: string
}) {
  return (
    <label for={props.id} class="verevon-settings-field">
      <span class="verevon-settings-label">{props.label}</span>
      <VerevonTextarea
        id={props.id}
        value={props.value}
        onInput={props.onInput}
        rows={4}
        class="verevon-settings-input verevon-settings-textarea"
      />
    </label>
  )
}

export function SettingsSelect(props: {
  disabled?: boolean
  id: string
  label: string
  onChange?: JSX.EventHandler<HTMLSelectElement, Event>
  options: Array<{ value: string; label: string }>
  value: string
}) {
  return (
    <label for={props.id} class="verevon-settings-field">
      <span class="verevon-settings-label">{props.label}</span>
      <span class="verevon-settings-input-wrap">
        <VerevonSelect
          id={props.id}
          value={props.value}
          onChange={props.onChange}
          disabled={props.disabled}
          class="verevon-settings-input verevon-settings-select"
        >
          <For each={props.options}>
            {(option) => <option value={option.value}>{option.label}</option>}
          </For>
        </VerevonSelect>
        <ChevronDown
          aria-hidden="true"
          class="verevon-settings-select-icon size-4"
          strokeWidth={1.7}
        />
      </span>
    </label>
  )
}

export function ToggleRow(props: {
  title: string
  description: string
  enabled: boolean
  disabled?: boolean
  /** Optional hover/focus explanation shown via an "i" icon next to the title. */
  info?: string
  onChange?: (checked: boolean) => void
}) {
  return (
    <div class="verevon-settings-toggle-row">
      <div>
        <p class="verevon-settings-toggle-title">
          {props.title}
          <Show when={props.info}>
            <span
              class="verevon-settings-info"
              tabindex="0"
              role="img"
              aria-label={props.info}
              title={props.info}
            >
              <Info size={13} aria-hidden="true" />
            </span>
          </Show>
        </p>
        <span>{props.description}</span>
      </div>
      <VerevonSwitch
        checked={props.enabled}
        disabled={props.disabled}
        label={props.title}
        onChange={props.onChange}
      />
    </div>
  )
}

export function SettingsButton(allProps: JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  danger?: boolean
  settingsSize?: 'xs' | 'sm' | 'md'
  variant?: 'primary' | 'secondary'
}) {
  const [local, props] = splitProps(allProps, ['children', 'class', 'danger', 'settingsSize', 'type', 'variant'])

  return (
    <button
      {...props}
      type={local.type ?? 'button'}
      class={cn(
        'verevon-settings-button',
        local.variant === 'primary' && 'verevon-settings-button--primary',
        local.settingsSize === 'xs' && 'verevon-settings-button--xs',
        local.settingsSize === 'sm' && 'verevon-settings-button--sm',
        local.danger && 'verevon-settings-button--danger',
        local.class,
      )}
    >
      {local.children}
    </button>
  )
}

export function SettingsSaveActions(props: {
  description?: string
  onCancel?: () => void
  onSave?: () => void
  saveDisabled?: boolean
  saveLabel: string
}) {
  return (
    <div class="verevon-settings-actions">
      {props.description ? <p>{props.description}</p> : <span />}
      <div>
        <SettingsButton onClick={props.onCancel} disabled={props.saveDisabled}>
          Cancel
        </SettingsButton>
        <SettingsButton variant="primary" onClick={props.onSave} disabled={props.saveDisabled}>
          <Check class="size-4" strokeWidth={1.8} />
          {props.saveLabel}
        </SettingsButton>
      </div>
    </div>
  )
}

export function StatusGrid(props: { cards: StatusCard[] }) {
  return (
    <div class="verevon-settings-status-grid">
      <For each={props.cards}>
        {(card) => (
          <div class="verevon-settings-status-card">
            <div>
              <span
                class={cn(
                  'verevon-settings-status-dot',
                  card.tone === 'ok' && 'verevon-settings-status-dot--ok',
                  card.tone === 'warn' && 'verevon-settings-status-dot--warn',
                )}
                aria-hidden="true"
              />
              <p>{card.label}</p>
            </div>
            <strong>{card.value}</strong>
            <span>{card.detail}</span>
          </div>
        )}
      </For>
    </div>
  )
}

export function FeaturePanel(props: {
  actionLabel: string
  children: JSX.Element
  class?: string
  description: string
  title: string
}) {
  return (
    <div class={cn('verevon-settings-feature-panel', props.class)}>
      <div class="verevon-settings-feature-panel__header">
        <div>
          <h3>{props.title}</h3>
          <p>{props.description}</p>
        </div>
        <SettingsButton settingsSize="xs">{props.actionLabel}</SettingsButton>
      </div>
      {props.children}
    </div>
  )
}

export function DataRow(props: {
  meta: string
  primary: string
  secondary: string
}) {
  return (
    <div class="verevon-settings-data-row">
      <div>
        <p>{props.primary}</p>
        <span>{props.secondary}</span>
      </div>
      <em>{props.meta}</em>
    </div>
  )
}

export function Metric(props: { label: string; value: string; detail: string }) {
  return (
    <div class="verevon-settings-metric">
      <p>{props.label}</p>
      <strong>{props.value}</strong>
      <span>{props.detail}</span>
    </div>
  )
}
