import { Check, ChevronDown } from 'lucide-solid'
import { For, splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'
import { VelionInput } from '@/shared/ui/velion/VelionInput'
import { VelionSelect } from '@/shared/ui/velion/VelionSelect'
import { VelionSwitch } from '@/shared/ui/velion/VelionSwitch'
import { VelionTextarea } from '@/shared/ui/velion/VelionTextarea'

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
      class="velion-settings-surface"
    >
      <div
        data-testid="settings-top-scroll-fade"
        aria-hidden="true"
        class="velion-settings-fade velion-settings-fade--top"
      />

      <div
        data-testid={props.contentTestId}
        class={cn(
          'velion-settings-content',
          props.contentVariant === 'account' && 'velion-settings-content--account',
        )}
      >
        <main class="velion-settings-main">{props.children}</main>
      </div>

      <div
        data-testid="settings-bottom-scroll-fade"
        aria-hidden="true"
        class="velion-settings-fade velion-settings-fade--bottom"
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
    <div class="velion-settings-hero">
      <p class="velion-settings-eyebrow">{props.eyebrow}</p>
      <h1 class="velion-settings-title">{props.title}</h1>
      {props.description ? (
        <p class="velion-settings-hero-description">{props.description}</p>
      ) : null}
      {props.signals && props.signals.length > 0 ? (
        <dl class="velion-settings-signal-grid">
          <For each={props.signals}>
            {(signal) => (
              <div class="velion-settings-signal">
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
    <div class="velion-settings-section-header">
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
    <label for={props.id} class="velion-settings-field">
      <span class="velion-settings-label">{props.label}</span>
      <span class="velion-settings-input-wrap">
        {props.prefix ? <span class="velion-settings-prefix">{props.prefix}</span> : null}
        <VelionInput
          id={props.id}
          type={props.type ?? 'text'}
          value={props.value}
          disabled={props.disabled}
          readOnly={props.readOnly}
          placeholder={props.placeholder}
          onInput={props.onInput}
          aria-describedby={helpId()}
          class={cn('velion-settings-input', props.prefix && 'velion-settings-input--prefixed')}
        />
      </span>
      {props.helpText ? (
        <span id={helpId()} class="velion-settings-help">
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
    <label for={props.id} class="velion-settings-field">
      <span class="velion-settings-label">{props.label}</span>
      <VelionTextarea
        id={props.id}
        value={props.value}
        onInput={props.onInput}
        rows={4}
        class="velion-settings-input velion-settings-textarea"
      />
    </label>
  )
}

export function SettingsSelect(props: {
  id: string
  label: string
  onChange?: JSX.EventHandler<HTMLSelectElement, Event>
  options: Array<{ value: string; label: string }>
  value: string
}) {
  return (
    <label for={props.id} class="velion-settings-field">
      <span class="velion-settings-label">{props.label}</span>
      <span class="velion-settings-input-wrap">
        <VelionSelect
          id={props.id}
          value={props.value}
          onChange={props.onChange}
          class="velion-settings-input velion-settings-select"
        >
          <For each={props.options}>
            {(option) => <option value={option.value}>{option.label}</option>}
          </For>
        </VelionSelect>
        <ChevronDown
          aria-hidden="true"
          class="velion-settings-select-icon size-4"
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
  onChange?: (checked: boolean) => void
}) {
  return (
    <div class="velion-settings-toggle-row">
      <div>
        <p>{props.title}</p>
        <span>{props.description}</span>
      </div>
      <VelionSwitch
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
        'velion-settings-button',
        local.variant === 'primary' && 'velion-settings-button--primary',
        local.settingsSize === 'xs' && 'velion-settings-button--xs',
        local.settingsSize === 'sm' && 'velion-settings-button--sm',
        local.danger && 'velion-settings-button--danger',
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
    <div class="velion-settings-actions">
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
    <div class="velion-settings-status-grid">
      <For each={props.cards}>
        {(card) => (
          <div class="velion-settings-status-card">
            <div>
              <span
                class={cn(
                  'velion-settings-status-dot',
                  card.tone === 'ok' && 'velion-settings-status-dot--ok',
                  card.tone === 'warn' && 'velion-settings-status-dot--warn',
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
    <div class={cn('velion-settings-feature-panel', props.class)}>
      <div class="velion-settings-feature-panel__header">
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
    <div class="velion-settings-data-row">
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
    <div class="velion-settings-metric">
      <p>{props.label}</p>
      <strong>{props.value}</strong>
      <span>{props.detail}</span>
    </div>
  )
}
