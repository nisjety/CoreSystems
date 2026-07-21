import { createMemo, createSignal, For, onMount, Show } from 'solid-js'
import { PasskeySecuritySection } from '@/features/settings/components/PasskeySecuritySection'
import { PrivacyDataSection } from '@/features/settings/components/PrivacyDataSection'
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
import { useI18n } from '@/shared/i18n'

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

function getProviderRows(i18n: ReturnType<typeof useI18n>) {
  return [
    { provider: i18n.tr('E-post og passord', 'Email and password'), detail: i18n.tr('Primær innloggingsmetode', 'Primary sign-in method') },
    { provider: 'Google', detail: i18n.tr('Tilgjengelig innloggingsleverandør', 'Available sign-in provider') },
    { provider: 'Microsoft', detail: i18n.tr('Tilgjengelig innloggingsleverandør', 'Available sign-in provider') },
    { provider: 'Apple', detail: i18n.tr('Planlagt for produksjons-HTTPS-domener', 'Planned for production HTTPS origins') },
  ]
}

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
  const i18n = useI18n()
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
        message: error instanceof Error ? error.message : i18n.tr('Kunne ikke laste profilen.', 'Could not load profile.'),
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
        message: error instanceof Error ? error.message : i18n.tr('Kunne ikke lagre profilen.', 'Could not save profile.'),
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
      { label: i18n.tr('Profil', 'Profile'), value: currentProfile?.accountStatus || i18n.tr('Laster', 'Loading') },
      { label: i18n.tr('E-post', 'Email'), value: currentProfile?.emailVerified ? i18n.tr('Verifisert', 'Verified') : i18n.tr('Ikke verifisert', 'Unverified') },
      { label: i18n.tr('Omfang', 'Scope'), value: currentProfile?.id ? i18n.tr('Control Plane-bruker', 'Control Plane user') : i18n.tr('Venter', 'Pending') },
    ]
  })

  const missingProfileFields = createMemo(() => {
    if (loadState().type === 'loading') return []

    const currentForm = form()
    const currentProfile = profile()
    return [
      !currentForm.displayName.trim() ? i18n.tr('visningsnavn', 'display name') : null,
      !currentForm.firstName.trim() || !currentForm.lastName.trim() ? i18n.tr('for- og etternavn', 'first and last name') : null,
      !currentProfile?.avatarUrl ? i18n.tr('profilbilde', 'profile image') : null,
      !currentForm.position.trim() ? i18n.tr('stillingstittel', 'job title') : null,
      !currentForm.department.trim() ? i18n.tr('avdeling', 'department') : null,
      !currentForm.phoneNumber.trim() ? i18n.tr('telefonnummer', 'phone number') : null,
    ].filter((item): item is string => Boolean(item))
  })

  const actionDescription = createMemo(() => {
    const state = loadState()
    if (state.type === 'saving') return i18n.tr('Lagrer profilen og innstillingene dine.', 'Saving your profile and preferences.')
    if (state.type === 'saved') return i18n.tr('Profil lagret.', 'Profile saved.')
    if (state.type === 'error') return state.message
    return i18n.tr('Din personlige profil og dine innstillinger lagres på Velion-kontoen din.', 'Your personal profile and preferences are saved to your Velion account.')
  })

  return (
    <SettingsSurface
      contentTestId="settings-profile-content"
      contentVariant="account"
      scrollAttribute
    >
      <SettingsHero eyebrow={i18n.tr('Konto', 'Account')} title={i18n.tr('Profilinnstillinger', 'Profile settings')} signals={signals()} />
      <Show when={loadState().type === 'error'}>
        <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
          {(loadState() as Extract<LoadState, { type: 'error' }>).message}
        </p>
      </Show>
      <Show when={missingProfileFields().length > 0}>
        <p class="velion-settings-status-message" role="status">
          {i18n.tr('Leverandørdata er brukt der tilgjengelig. Legg til', 'Provider data has been applied where available. Add')} {missingProfileFields().join(', ')} {i18n.tr('for å fullføre Velion-profilen.', 'to complete the Velion profile.')}
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
      <PrivacyDataSection />
      <SettingsSaveActions
        description={actionDescription()}
        saveLabel={loadState().type === 'saving' ? i18n.tr('Lagrer …', 'Saving...') : i18n.tr('Lagre profil', 'Save profile')}
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
  const i18n = useI18n()
  const avatarInitials = () => initials(props.form().displayName, props.profile()?.email ?? '')

  return (
    <section id="profile" class="velion-settings-section">
      <SectionHeader
        title={i18n.tr('Profil', 'Profile')}
        description={i18n.tr('Bestem hvordan kollegaer og kunder ser deg på tvers av Velion.', 'Control how teammates and customers see you across Velion.')}
      />

      <div class="velion-settings-avatar-row">
        <button
          type="button"
          aria-label={i18n.tr('Profilbilde', 'Profile avatar')}
          class="velion-settings-avatar"
          disabled
        >
          <Show when={props.profile()?.avatarUrl} fallback={avatarInitials()}>
            {(src) => <img src={src()} alt="" />}
          </Show>
        </button>
        <div>
          <p>{props.form().displayName || props.profile()?.email || i18n.tr('Laster kontodetaljer …', 'Loading account details...')}</p>
          <div>
            <SettingsButton settingsSize="sm" disabled>
              {props.profile()?.avatarUrl ? i18n.tr('Bilde fra leverandør', 'Provider photo') : i18n.tr('Last opp bilde', 'Upload photo')}
            </SettingsButton>
            <SettingsButton settingsSize="sm" disabled>{i18n.tr('Fjern', 'Remove')}</SettingsButton>
          </div>
        </div>
      </div>

      <div class="velion-settings-field-grid">
        <SettingsField
          id="display-name"
          label={i18n.tr('Visningsnavn', 'Display name')}
          value={props.form().displayName}
          onInput={(event) => props.onField('displayName', event.currentTarget.value)}
        />
        <SettingsField
          id="first-name"
          label={i18n.tr('Fornavn', 'First name')}
          value={props.form().firstName}
          onInput={(event) => props.onField('firstName', event.currentTarget.value)}
        />
        <SettingsField
          id="last-name"
          label={i18n.tr('Etternavn', 'Last name')}
          value={props.form().lastName}
          onInput={(event) => props.onField('lastName', event.currentTarget.value)}
        />
        <SettingsField
          id="job-title"
          label={i18n.tr('Stillingstittel', 'Job title')}
          value={props.form().position}
          onInput={(event) => props.onField('position', event.currentTarget.value)}
        />
        <SettingsField
          id="department"
          label={i18n.tr('Avdeling', 'Department')}
          value={props.form().department}
          onInput={(event) => props.onField('department', event.currentTarget.value)}
        />
        <SettingsField
          id="username"
          label={i18n.tr('Konto-ID', 'Account ID')}
          value={props.profile()?.id ?? ''}
          readOnly
          helpText={i18n.tr('Skrivebeskyttet kontoidentifikator.', 'Read-only account identifier.')}
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
  const i18n = useI18n()
  const emailHelp = () => props.profile()?.emailVerified ? i18n.tr('Verifisert.', 'Verified.') : i18n.tr('Ikke verifisert.', 'Not verified.')

  return (
    <section id="contact" class="velion-settings-section">
      <SectionHeader
        title={i18n.tr('Kontakt', 'Contact')}
        description={i18n.tr('Hold innlogging og kontaktinformasjon oppdatert.', 'Keep sign-in and teammate contact details current.')}
      />
      <div class="velion-settings-field-grid">
        <SettingsField
          id="email"
          label={i18n.tr('Primær e-post', 'Primary email')}
          type="email"
          value={props.profile()?.email ?? ''}
          readOnly
          helpText={emailHelp()}
        />
        <SettingsField
          id="phone"
          label={i18n.tr('Telefonnummer', 'Phone number')}
          type="tel"
          value={props.form().phoneNumber}
          onInput={(event) => props.onField('phoneNumber', event.currentTarget.value)}
        />
        <SettingsField
          id="office-location"
          label={i18n.tr('Kontorsted', 'Office location')}
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
  const i18n = useI18n()
  return (
    <section id="preferences" class="velion-settings-section">
      <SectionHeader
        title={i18n.tr('Innstillinger', 'Preferences')}
        description={i18n.tr('Tilpass hvordan Velion formaterer språk, utseende og kollegers navn.', 'Personalize how Velion formats language, appearance, and teammate names.')}
      />
      <div class="velion-settings-field-grid">
        <SettingsSelect
          id="language"
          label={i18n.tr('Språk', 'Language')}
          value={props.form().language}
          onChange={(event) => props.onField('language', event.currentTarget.value)}
          options={[
            { value: 'en-US', label: i18n.tr('Engelsk', 'English') },
            { value: 'nb-NO', label: i18n.tr('Norsk bokmål', 'Norwegian Bokmal') },
            { value: 'fr-FR', label: i18n.tr('Fransk', 'French') },
            { value: 'de-DE', label: i18n.tr('Tysk', 'German') },
          ]}
        />
        <SettingsSelect
          id="timezone"
          label={i18n.tr('Tidssone', 'Time zone')}
          value={props.form().timezone}
          onChange={(event) => props.onField('timezone', event.currentTarget.value)}
          options={[
            { value: 'Europe/Oslo', label: 'Europe/Oslo' },
            { value: 'UTC', label: 'UTC' },
            { value: 'America/New_York', label: 'America/New York' },
            { value: 'Europe/London', label: 'Europe/London' },
          ]}
        />
        <SettingsSelect
          id="theme"
          label={i18n.tr('Tema', 'Theme')}
          value={props.form().theme}
          onChange={(event) => props.onField('theme', event.currentTarget.value)}
          options={[
            { value: 'system', label: i18n.tr('System', 'System') },
            { value: 'light', label: i18n.tr('Lys', 'Light') },
            { value: 'dark', label: i18n.tr('Mørk', 'Dark') },
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
  const i18n = useI18n()
  const supportPreferences = () => [
    {
      title: i18n.tr('E-postvarsler', 'Email notifications'),
      description: i18n.tr('Send konto- og supportoppdateringer til den primære e-posten din.', 'Send account and support updates to your primary email.'),
      enabled: props.form().emailNotifications,
      onChange: (checked: boolean) => props.onField('emailNotifications', checked),
    },
    {
      title: i18n.tr('Push-varsler', 'Push notifications'),
      description: i18n.tr('Vis nettleservarsler for samtaler tildelt deg.', 'Show browser notifications for assigned conversations.'),
      enabled: props.form().pushNotifications,
      onChange: (checked: boolean) => props.onField('pushNotifications', checked),
    },
  ]

  return (
    <section id="availability" class="velion-settings-section">
      <SectionHeader
        title={i18n.tr('Tilgjengelighet', 'Availability')}
        description={i18n.tr('Juster personlig atferd for support på samtaler tildelt deg.', 'Tune personal helpdesk behavior for conversations assigned to you.')}
      />
      <div class="velion-settings-field-grid">
        <SettingsSelect
          id="availability-status"
          label={i18n.tr('Tilgjengelighetsstatus', 'Availability status')}
          value={props.form().status}
          onChange={(event) => props.onField('status', event.currentTarget.value)}
          options={[
            { value: 'online', label: i18n.tr('Pålogget', 'Online') },
            { value: 'busy', label: i18n.tr('Opptatt', 'Busy') },
            { value: 'away', label: i18n.tr('Borte', 'Away') },
            { value: 'offline', label: i18n.tr('Avlogget', 'Offline') },
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
  const i18n = useI18n()
  const connectedProviderRows = createMemo(() => {
    const current = props.profile()
    return [
      {
        provider: i18n.tr('Innloggingsleverandør', 'Signed-in provider'),
        detail: current?.avatarUrl
          ? i18n.tr('Navn, e-post og profilbilde er hentet fra autentiseringsleverandøren.', 'Name, email, and profile image retained from the auth provider.')
          : i18n.tr('Navn og e-post er hentet; legg til et profilbilde hvis leverandøren ikke sendte ett.', 'Name and email retained; add a profile image if the provider did not send one.'),
      },
      ...getProviderRows(i18n),
    ]
  })

  return (
    <section id="connected-accounts" class="velion-settings-section">
      <SectionHeader
        title={i18n.tr('Tilkoblede kontoer', 'Connected accounts')}
        description={i18n.tr('Innloggingsleverandører konfigurert for denne Velion-kontoen.', 'Sign-in providers configured for this Velion account.')}
      />
      <div class="velion-settings-list-card">
        <For each={connectedProviderRows()}>
          {(account) => (
            <div class="velion-settings-list-row">
              <div>
                <p>{account.provider}</p>
                <span>{account.detail}</span>
              </div>
              <SettingsButton settingsSize="sm" disabled>{i18n.tr('Administrert', 'Managed')}</SettingsButton>
            </div>
          )}
        </For>
      </div>
    </section>
  )
}

function PrivacySection() {
  const i18n = useI18n()
  return (
    <section id="privacy" class="velion-settings-section">
      <SectionHeader
        title={i18n.tr('Personvern', 'Privacy')}
        description={i18n.tr('Velg hvor synlig kontoen din er for andre arbeidsområder.', 'Choose how discoverable your account is to other workspaces.')}
      />
      <div class="velion-settings-divided-list">
        <ToggleRow
          title={i18n.tr('Profilsynlighet', 'Profile visibility')}
          description={i18n.tr('Tillat personer med e-postadressen din å se navnet og profilbildet ditt når de inviterer deg.', 'Allow people with your email address to see your name and avatar when inviting you.')}
          enabled
          disabled
        />
        <ToggleRow
          title={i18n.tr('Registrer profilaktivitet', 'Record profile activity')}
          description={i18n.tr('Inkluder profilvisninger og bidragshistorikk i kontoaktivitet.', 'Include profile views and contribution history in account activity.')}
          enabled={false}
          disabled
        />
      </div>
    </section>
  )
}
