import { A, useLocation } from '@solidjs/router'
import {
  Building2,
  CheckCircle2,
  CircleUserRound,
  CreditCard,
  Globe2,
  KeyRound,
  Package,
  Plug,
  Route,
  ServerCog,
  Settings,
  ShieldCheck,
  Sparkles,
  TestTubeDiagonal,
  User,
  UsersRound,
} from 'lucide-solid'
import { createSignal, For, onCleanup, onMount, Show, type Component } from 'solid-js'
import type { LucideProps } from 'lucide-solid'
import { Dynamic } from 'solid-js/web'
import { SidebarPanelTitle } from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { getSession } from '@/shared/session/session-store'
import { hasPlatformAdminAccess } from '@/shared/session/access'

type SettingsIcon = Component<LucideProps>
type SettingsLinkSection = { id: string; label: string; icon: SettingsIcon; href: string }

const accountSidebarSections: SettingsLinkSection[] = [
  { id: 'profile', label: 'Profile', icon: User, href: '#profile' },
  { id: 'contact', label: 'Contact', icon: CircleUserRound, href: '#contact' },
  { id: 'preferences', label: 'Preferences', icon: Settings, href: '#preferences' },
  { id: 'availability', label: 'Availability', icon: CheckCircle2, href: '#availability' },
  { id: 'connected-accounts', label: 'Connected accounts', icon: Plug, href: '#connected-accounts' },
  { id: 'privacy', label: 'Privacy', icon: Globe2, href: '#privacy' },
]

const settingsSidebarSections: SettingsLinkSection[] = [
  { id: 'workspace', label: 'Workspace', icon: Building2, href: '/settings/workspace' },
  { id: 'members', label: 'Members & roles', icon: UsersRound, href: '/settings/members' },
  { id: 'platform-users', label: 'All users (platform)', icon: ShieldCheck, href: '/settings/platform-users' },
  { id: 'billing', label: 'Billing', icon: CreditCard, href: '/settings/billing' },
  { id: 'sso', label: 'SSO', icon: KeyRound, href: '/settings/sso' },
  { id: 'org-security', label: 'Org security', icon: ShieldCheck, href: '/settings/org-security' },
  { id: 'integrations', label: 'Integrations', icon: Plug, href: '/settings/integrations' },
  { id: 'router-policy', label: 'Router policy', icon: Route, href: '/settings/router-policy' },
  { id: 'finetune', label: 'Fine-tune jobs', icon: TestTubeDiagonal, href: '/settings/finetune' },
  { id: 'mcp', label: 'MCP-servere', icon: ServerCog, href: '/settings/mcp' },
  { id: 'skills', label: 'Ferdigheter', icon: Sparkles, href: '/settings/skills' },
  { id: 'plugins', label: 'Plugin-pakker', icon: Package, href: '/settings/plugins' },
]
const defaultAccountSectionId = accountSidebarSections[0]!.id
const defaultSettingsSectionId = settingsSidebarSections[0]!.id

export function AccountExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const i18n = useI18n()
  const activeSectionId = useActiveAccountSection()

  return (
    <div class="core-sidebar-dedicated-panel">
      <SidebarPanelTitle spacing="core-sidebar-title-spacious" onCollapse={props.onCollapse}>
        {i18n.tr('Konto', 'Account')}
      </SidebarPanelTitle>

      <SettingsSectionLinks
        ariaLabel={i18n.tr('Kontoseksjoner', 'Account sections')}
        sections={localizedAccountSections(i18n)}
        activeSectionId={activeSectionId()}
      />
    </div>
  )
}

export function SettingsExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const i18n = useI18n()
  const location = useLocation()
  const activeSectionId = () => getSettingsActiveSectionId(location.pathname)

  return (
    <div class="core-sidebar-dedicated-panel">
      <SidebarPanelTitle spacing="core-sidebar-title-spacious" onCollapse={props.onCollapse}>
        {i18n.tr('Innstillinger', 'Settings')}
      </SidebarPanelTitle>

      <SettingsSectionLinks
        ariaLabel={i18n.tr('Innstillingsseksjoner', 'Settings sections')}
        sections={localizedSettingsSections(i18n).filter(
          (section) => section.id !== 'platform-users' || hasPlatformAdminAccess(getSession()),
        )}
        activeSectionId={activeSectionId()}
      />
    </div>
  )
}

function SettingsSectionLinks(props: {
  activeSectionId: string
  ariaLabel: string
  sections: SettingsLinkSection[]
}) {
  return (
    <nav aria-label={props.ariaLabel} class="core-sidebar-link-list">
      <For each={props.sections}>
        {(section) => (
          <SettingsSectionLink active={props.activeSectionId === section.id} section={section} />
        )}
      </For>
    </nav>
  )
}

function SettingsSectionLink(props: {
  active: boolean
  section: SettingsLinkSection
}) {
  const content = (
    <>
      <Dynamic component={props.section.icon} class="core-sidebar-dedicated-icon" strokeWidth={1.7} />
      <span>{props.section.label}</span>
    </>
  )
  const className = () => cn('core-sidebar-section-link', props.active && 'core-sidebar-section-link--active')

  return (
    <Show
      when={props.section.href.startsWith('#')}
      fallback={
        <A href={props.section.href} class={className()} aria-current={props.active ? 'page' : undefined}>
          {content}
        </A>
      }
    >
      <a href={props.section.href} class={className()} aria-current={props.active ? 'location' : undefined}>
        {content}
      </a>
    </Show>
  )
}

function getSettingsActiveSectionId(pathname: string) {
  const section = pathname.split('/')[2]
  return section && settingsSidebarSections.some((item) => item.id === section)
    ? section
    : defaultSettingsSectionId
}

function useActiveAccountSection() {
  const [activeSectionId, setActiveSectionId] = createSignal(getActiveAccountSectionId())

  onMount(() => {
    const ids = accountSidebarSections.map((section) => section.id)
    let frame = 0

    const updateActiveSection = () => {
      window.cancelAnimationFrame(frame)
      frame = window.requestAnimationFrame(() => {
        const scrollRoot = document.querySelector<HTMLElement>('[data-account-settings-scroll]')
        if (scrollRoot && scrollRoot.scrollTop + scrollRoot.clientHeight >= scrollRoot.scrollHeight - 24) {
          setActiveSectionId(ids[ids.length - 1] ?? defaultAccountSectionId)
          return
        }

        const rootTop = scrollRoot?.getBoundingClientRect().top ?? 0
        const activationLine = rootTop + 180
        let nextActiveSectionId = ids[0] ?? defaultAccountSectionId

        for (const id of ids) {
          const section = document.getElementById(id)
          if (section && section.getBoundingClientRect().top <= activationLine) {
            nextActiveSectionId = id
          }
        }

        setActiveSectionId(nextActiveSectionId)
      })
    }

    const scrollRoot = document.querySelector<HTMLElement>('[data-account-settings-scroll]')
    scrollRoot?.addEventListener('scroll', updateActiveSection, { passive: true })
    window.addEventListener('resize', updateActiveSection)

    onCleanup(() => {
      window.cancelAnimationFrame(frame)
      scrollRoot?.removeEventListener('scroll', updateActiveSection)
      window.removeEventListener('resize', updateActiveSection)
    })
  })

  return activeSectionId
}

function getActiveAccountSectionId() {
  if (typeof document === 'undefined') return defaultAccountSectionId

  const ids = accountSidebarSections.map((section) => section.id)
  const scrollRoot = document.querySelector<HTMLElement>('[data-account-settings-scroll]')
  if (scrollRoot && scrollRoot.scrollTop + scrollRoot.clientHeight >= scrollRoot.scrollHeight - 24) {
    return ids[ids.length - 1] ?? defaultAccountSectionId
  }

  const rootTop = scrollRoot?.getBoundingClientRect().top ?? 0
  const activationLine = rootTop + 180
  return ids.reduce((activeId, id) => {
    const section = document.getElementById(id)
    return section && section.getBoundingClientRect().top <= activationLine ? id : activeId
  }, ids[0] ?? defaultAccountSectionId)
}

function localizedAccountSections(i18n: ReturnType<typeof useI18n>): SettingsLinkSection[] {
  return accountSidebarSections.map((section) => ({
    ...section,
    label: accountSectionLabel(section.id, section.label, i18n),
  }))
}

function localizedSettingsSections(i18n: ReturnType<typeof useI18n>): SettingsLinkSection[] {
  return settingsSidebarSections.map((section) => ({
    ...section,
    label: settingsSectionLabel(section.id, section.label, i18n),
  }))
}

function accountSectionLabel(id: string, fallback: string, i18n: ReturnType<typeof useI18n>): string {
  switch (id) {
    case 'profile':
      return i18n.tr('Profil', 'Profile')
    case 'contact':
      return i18n.tr('Kontakt', 'Contact')
    case 'preferences':
      return i18n.tr('Preferanser', 'Preferences')
    case 'availability':
      return i18n.tr('Tilgjengelighet', 'Availability')
    case 'connected-accounts':
      return i18n.tr('Tilkoblede kontoer', 'Connected accounts')
    case 'privacy':
      return i18n.tr('Personvern', 'Privacy')
    default:
      return fallback
  }
}

function settingsSectionLabel(id: string, fallback: string, i18n: ReturnType<typeof useI18n>): string {
  switch (id) {
    case 'workspace':
      return i18n.tr('Arbeidsområde', 'Workspace')
    case 'members':
      return i18n.tr('Medlemmer og roller', 'Members & roles')
    case 'platform-users':
      return i18n.tr('Alle brukere (plattform)', 'All users (platform)')
    case 'billing':
      return i18n.tr('Fakturering', 'Billing')
    case 'sso':
      return i18n.tr('SSO', 'SSO')
    case 'org-security':
      return i18n.tr('Organisasjonssikkerhet', 'Org security')
    case 'integrations':
      return i18n.tr('Integrasjoner', 'Integrations')
    case 'router-policy':
      return i18n.tr('Router-policy', 'Router policy')
    case 'finetune':
      return i18n.tr('Finjusteringsjobber', 'Fine-tune jobs')
    case 'mcp':
      return i18n.tr('MCP-servere', 'MCP servers')
    case 'skills':
      return i18n.tr('Ferdigheter', 'Skills')
    case 'plugins':
      return i18n.tr('Plugin-pakker', 'Plugins')
    default:
      return fallback
  }
}
