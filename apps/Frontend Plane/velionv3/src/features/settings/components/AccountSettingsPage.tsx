import { createMemo, createSignal, For, onMount, Show } from 'solid-js'
import { PasskeySecuritySection } from '@/features/settings/components/PasskeySecuritySection'
import { TwoFactorEnrollmentSection } from '@/features/settings/components/TwoFactorEnrollmentSection'
import {
  SectionHeader,
  SettingsButton,
  SettingsField,
  SettingsHero,
  SettingsSaveActions,
  SettingsSelect,
  SettingsSurface,
  ToggleRow,
} from '@/features/settings/components/settings-ui'
import {
  getMe,
  getPreferences,
  updateMe,
  updatePreferences,
  type UserPreferences,
  type UserProfile,
} from '@/shared/api/settings-client'

type FormState = {
  displayName: string
  firstName: string
  lastName: string
  position: string
  department: string
  phoneNumber: string
  officeLocation: string
  timezone: string
  language: string
  theme: string
  status: string
  emailNotifications: boolean
  pushNotifications: boolean
}

type LoadState =
  | { type: 'loading' }
  | { type: 'ready' }
  | { type: 'saving' }
  | { type: 'saved' }
  | { type: 'error'; message: string }

const emptyForm: FormState = {
  displayName: '',
  firstName: '',
  lastName: '',
  position: '',
  department: '',
  phoneNumber: '',
  officeLocation: '',
  timezone: 'Europe/Oslo',
  language: 'en-US',
  theme: 'system',
  status: 'online',
  emailNotifications: true,
  pushNotifications: true,
}

const providerRows = [
  { provider: 'Email and password', detail: 'Primary sign-in method' },
  { provider: 'Google', detail: 'Available sign-in provider' },
  { provider: 'Microsoft', detail: 'Available sign-in provider' },
  { provider: 'Apple', detail: 'Planned for production HTTPS origins' },
]

function splitDisplayName(name: string): { firstName: string; lastName: string } {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return { firstName: '', lastName: '' }
  if (parts.length === 1) return { firstName: parts[0] ?? '', lastName: '' }
  return {
    firstName: parts[0] ?? '',
    lastName: parts.slice(1).join(' '),
  }
}

function formFrom(profile: UserProfile, preferences: UserPreferences): FormState {
  const displayName = profile.displayName || profile.name || profile.email.split('@')[0] || ''
  const nameParts = splitDisplayName(displayName)
  return {
    displayName,
    firstName: profile.firstName || nameParts.firstName,
    lastName: profile.lastName || nameParts.lastName,
    position: profile.position,
    department: profile.department,
    phoneNumber: profile.phoneNumber,
    officeLocation: profile.officeLocation,
    timezone: preferences.timezone || profile.timezone || emptyForm.timezone,
    language: preferences.language || emptyForm.language,
    theme: preferences.theme || emptyForm.theme,
    status: profile.status || emptyForm.status,
    emailNotifications: preferences.notifications?.email ?? emptyForm.emailNotifications,
    pushNotifications: preferences.notifications?.push ?? emptyForm.pushNotifications,
  }
}

function initials(name: string, email: string): string {
  const source = (name || email || 'V').trim()
  const parts = source.split(/\s+/).filter(Boolean)
  if (parts.length >= 2) return `${parts[0]?.[0] ?? ''}${parts[1]?.[0] ?? ''}`.toUpperCase()
  return source.slice(0, 2).toUpperCase()
}

export default function AccountSettingsPage() {
  const [profile, setProfile] = createSignal<UserProfile | null>(null)
  const [form, setForm] = createSignal<FormState>(emptyForm)
  const [loadState, setLoadState] = createSignal<LoadState>({ type: 'loading' })

  const loadAccount = async () => {
    setLoadState({ type: 'loading' })
    try {
      const [profileData, preferencesData] = await Promise.all([getMe(), getPreferences()])
      setProfile(profileData)
      setForm(formFrom(profileData, preferencesData))
      setLoadState({ type: 'ready' })
    } catch (error) {
      setLoadState({
        type: 'error',
        message: error instanceof Error ? error.message : 'Could not load profile.',
      })
    }
  }

  onMount(() => {
    void loadAccount()
  })

  const updateField = <K extends keyof FormState>(key: K, value: FormState[K]) => {
    setForm((current) => ({ ...current, [key]: value }))
    if (loadState().type === 'saved') setLoadState({ type: 'ready' })
  }

  const save = async () => {
    if (loadState().type === 'saving') return
    const current = form()
    setLoadState({ type: 'saving' })
    try {
      const [profileData] = await Promise.all([
        updateMe({
          name: current.displayName,
          displayName: current.displayName,
          firstName: current.firstName,
          lastName: current.lastName,
          phoneNumber: current.phoneNumber,
          officeLocation: current.officeLocation,
          timezone: current.timezone,
          position: current.position,
          department: current.department,
          status: current.status,
        }),
        updatePreferences({
          language: current.language,
          timezone: current.timezone,
          theme: current.theme,
          notifications: {
            email: current.emailNotifications,
            push: current.pushNotifications,
          },
        }),
      ])
      setProfile(profileData)
      setLoadState({ type: 'saved' })
    } catch (error) {
      setLoadState({
        type: 'error',
        message: error instanceof Error ? error.message : 'Could not save profile.',
      })
    }
  }

  const reset = () => {
    const currentProfile = profile()
    if (!currentProfile) {
      void loadAccount()
      return
    }
    void Promise.all([getPreferences()]).then(([preferences]) => {
      setForm(formFrom(currentProfile, preferences))
      setLoadState({ type: 'ready' })
    })
  }

  const signals = createMemo(() => {
    const currentProfile = profile()
    return [
      { label: 'Profile', value: currentProfile?.accountStatus || 'Loading' },
      { label: 'Email', value: currentProfile?.emailVerified ? 'Verified' : 'Unverified' },
      { label: 'Scope', value: currentProfile?.id ? 'Control Plane user' : 'Pending' },
    ]
  })

  const missingProfileFields = createMemo(() => {
    if (loadState().type === 'loading') return []

    const currentForm = form()
    const currentProfile = profile()
    return [
      !currentForm.displayName.trim() ? 'display name' : null,
      !currentForm.firstName.trim() || !currentForm.lastName.trim() ? 'first and last name' : null,
      !currentProfile?.avatarUrl ? 'profile image' : null,
      !currentForm.position.trim() ? 'job title' : null,
      !currentForm.department.trim() ? 'department' : null,
      !currentForm.phoneNumber.trim() ? 'phone number' : null,
    ].filter((item): item is string => Boolean(item))
  })

  const actionDescription = createMemo(() => {
    const state = loadState()
    if (state.type === 'saving') return 'Saving your profile and preferences.'
    if (state.type === 'saved') return 'Profile saved.'
    if (state.type === 'error') return state.message
    return 'Your personal profile and preferences are saved to your Velion account.'
  })

  return (
    <SettingsSurface
      contentTestId="settings-profile-content"
      contentVariant="account"
      scrollAttribute
    >
      <SettingsHero eyebrow="Account" title="Profile settings" signals={signals()} />
      <Show when={loadState().type === 'error'}>
        <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
          {(loadState() as Extract<LoadState, { type: 'error' }>).message}
        </p>
      </Show>
      <Show when={missingProfileFields().length > 0}>
        <p class="velion-settings-status-message" role="status">
          Provider data has been applied where available. Add {missingProfileFields().join(', ')} to complete the Velion profile.
        </p>
      </Show>
      <ProfileSection
        form={form}
        profile={profile}
        onField={updateField}
      />
      <ContactSection
        form={form}
        profile={profile}
        onField={updateField}
      />
      <PreferencesSection form={form} onField={updateField} />
      <AvailabilitySection form={form} onField={updateField} />
      <ConnectedAccountsSection profile={profile} />
      <TwoFactorEnrollmentSection />
      <PasskeySecuritySection />
      <PrivacySection />
      <SettingsSaveActions
        description={actionDescription()}
        saveLabel={loadState().type === 'saving' ? 'Saving...' : 'Save profile'}
        saveDisabled={loadState().type === 'loading' || loadState().type === 'saving'}
        onCancel={reset}
        onSave={() => void save()}
      />
    </SettingsSurface>
  )
}

function ProfileSection(props: {
  form: () => FormState
  profile: () => UserProfile | null
  onField: <K extends keyof FormState>(key: K, value: FormState[K]) => void
}) {
  const avatarInitials = () => initials(props.form().displayName, props.profile()?.email ?? '')

  return (
    <section id="profile" class="velion-settings-section">
      <SectionHeader
        title="Profile"
        description="Control how teammates and customers see you across Velion."
      />

      <div class="velion-settings-avatar-row">
        <button
          type="button"
          aria-label="Profile avatar"
          class="velion-settings-avatar"
          disabled
        >
          <Show when={props.profile()?.avatarUrl} fallback={avatarInitials()}>
            {(src) => <img src={src()} alt="" />}
          </Show>
        </button>
        <div>
          <p>{props.form().displayName || props.profile()?.email || 'Loading account details...'}</p>
          <div>
            <SettingsButton settingsSize="sm" disabled>
              {props.profile()?.avatarUrl ? 'Provider photo' : 'Upload photo'}
            </SettingsButton>
            <SettingsButton settingsSize="sm" disabled>Remove</SettingsButton>
          </div>
        </div>
      </div>

      <div class="velion-settings-field-grid">
        <SettingsField
          id="display-name"
          label="Display name"
          value={props.form().displayName}
          onInput={(event) => props.onField('displayName', event.currentTarget.value)}
        />
        <SettingsField
          id="first-name"
          label="First name"
          value={props.form().firstName}
          onInput={(event) => props.onField('firstName', event.currentTarget.value)}
        />
        <SettingsField
          id="last-name"
          label="Last name"
          value={props.form().lastName}
          onInput={(event) => props.onField('lastName', event.currentTarget.value)}
        />
        <SettingsField
          id="job-title"
          label="Job title"
          value={props.form().position}
          onInput={(event) => props.onField('position', event.currentTarget.value)}
        />
        <SettingsField
          id="department"
          label="Department"
          value={props.form().department}
          onInput={(event) => props.onField('department', event.currentTarget.value)}
        />
        <SettingsField
          id="username"
          label="Account ID"
          value={props.profile()?.id ?? ''}
          readOnly
          helpText="Read-only account identifier."
        />
      </div>
    </section>
  )
}

function ContactSection(props: {
  form: () => FormState
  profile: () => UserProfile | null
  onField: <K extends keyof FormState>(key: K, value: FormState[K]) => void
}) {
  const emailHelp = () => props.profile()?.emailVerified ? 'Verified.' : 'Not verified.'

  return (
    <section id="contact" class="velion-settings-section">
      <SectionHeader
        title="Contact"
        description="Keep sign-in and teammate contact details current."
      />
      <div class="velion-settings-field-grid">
        <SettingsField
          id="email"
          label="Primary email"
          type="email"
          value={props.profile()?.email ?? ''}
          readOnly
          helpText={emailHelp()}
        />
        <SettingsField
          id="phone"
          label="Phone number"
          type="tel"
          value={props.form().phoneNumber}
          onInput={(event) => props.onField('phoneNumber', event.currentTarget.value)}
        />
        <SettingsField
          id="office-location"
          label="Office location"
          value={props.form().officeLocation}
          onInput={(event) => props.onField('officeLocation', event.currentTarget.value)}
        />
      </div>
    </section>
  )
}

function PreferencesSection(props: {
  form: () => FormState
  onField: <K extends keyof FormState>(key: K, value: FormState[K]) => void
}) {
  return (
    <section id="preferences" class="velion-settings-section">
      <SectionHeader
        title="Preferences"
        description="Personalize how Velion formats language, appearance, and teammate names."
      />
      <div class="velion-settings-field-grid">
        <SettingsSelect
          id="language"
          label="Language"
          value={props.form().language}
          onChange={(event) => props.onField('language', event.currentTarget.value)}
          options={[
            { value: 'en-US', label: 'English' },
            { value: 'nb-NO', label: 'Norwegian Bokmal' },
            { value: 'fr-FR', label: 'French' },
            { value: 'de-DE', label: 'German' },
          ]}
        />
        <SettingsSelect
          id="timezone"
          label="Time zone"
          value={props.form().timezone}
          onChange={(event) => props.onField('timezone', event.currentTarget.value)}
          options={[
            { value: 'Europe/Oslo', label: 'Europe/Oslo' },
            { value: 'UTC', label: 'UTC' },
            { value: 'America/New_York', label: 'America/New York' },
            { value: 'Europe/London', label: 'Europe/London' },
            { value: 'US', label: 'US region default' },
          ]}
        />
        <SettingsSelect
          id="theme"
          label="Theme"
          value={props.form().theme}
          onChange={(event) => props.onField('theme', event.currentTarget.value)}
          options={[
            { value: 'system', label: 'System' },
            { value: 'light', label: 'Light' },
            { value: 'dark', label: 'Dark' },
          ]}
        />
      </div>
    </section>
  )
}

function AvailabilitySection(props: {
  form: () => FormState
  onField: <K extends keyof FormState>(key: K, value: FormState[K]) => void
}) {
  const supportPreferences = () => [
    {
      title: 'Email notifications',
      description: 'Send account and support updates to your primary email.',
      enabled: props.form().emailNotifications,
      onChange: (checked: boolean) => props.onField('emailNotifications', checked),
    },
    {
      title: 'Push notifications',
      description: 'Show browser notifications for assigned conversations.',
      enabled: props.form().pushNotifications,
      onChange: (checked: boolean) => props.onField('pushNotifications', checked),
    },
  ]

  return (
    <section id="availability" class="velion-settings-section">
      <SectionHeader
        title="Availability"
        description="Tune personal helpdesk behavior for conversations assigned to you."
      />
      <div class="velion-settings-field-grid">
        <SettingsSelect
          id="availability-status"
          label="Availability status"
          value={props.form().status}
          onChange={(event) => props.onField('status', event.currentTarget.value)}
          options={[
            { value: 'online', label: 'Online' },
            { value: 'busy', label: 'Busy' },
            { value: 'away', label: 'Away' },
            { value: 'offline', label: 'Offline' },
          ]}
        />
      </div>
      <div class="velion-settings-divided-list">
        <For each={supportPreferences()}>
          {(preference) => <ToggleRow {...preference} />}
        </For>
      </div>
    </section>
  )
}

function ConnectedAccountsSection(props: {
  profile: () => UserProfile | null
}) {
  const connectedProviderRows = createMemo(() => {
    const current = props.profile()
    return [
      {
        provider: 'Signed-in provider',
        detail: current?.avatarUrl
          ? 'Name, email, and profile image retained from the auth provider.'
          : 'Name and email retained; add a profile image if the provider did not send one.',
      },
      ...providerRows,
    ]
  })

  return (
    <section id="connected-accounts" class="velion-settings-section">
      <SectionHeader
        title="Connected accounts"
        description="Sign-in providers configured for this Velion account."
      />
      <div class="velion-settings-list-card">
        <For each={connectedProviderRows()}>
          {(account) => (
            <div class="velion-settings-list-row">
              <div>
                <p>{account.provider}</p>
                <span>{account.detail}</span>
              </div>
              <SettingsButton settingsSize="sm" disabled>Managed</SettingsButton>
            </div>
          )}
        </For>
      </div>
    </section>
  )
}

function PrivacySection() {
  return (
    <section id="privacy" class="velion-settings-section">
      <SectionHeader
        title="Privacy"
        description="Choose how discoverable your account is to other workspaces."
      />
      <div class="velion-settings-divided-list">
        <ToggleRow
          title="Profile visibility"
          description="Allow people with your email address to see your name and avatar when inviting you."
          enabled
          disabled
        />
        <ToggleRow
          title="Record profile activity"
          description="Include profile views and contribution history in account activity."
          enabled={false}
          disabled
        />
      </div>
    </section>
  )
}
