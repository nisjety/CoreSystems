import { A, useLocation } from '@solidjs/router'
import {
  Building2,
  CheckCircle2,
  CircleUserRound,
  CreditCard,
  Globe2,
  KeyRound,
  Plug,
  Route,
  Settings,
  ShieldCheck,
  TestTubeDiagonal,
  User,
  UsersRound,
} from 'lucide-solid'
import { createSignal, For, onCleanup, onMount, Show, type Component } from 'solid-js'
import type { LucideProps } from 'lucide-solid'
import { Dynamic } from 'solid-js/web'
import { SidebarPanelTitle } from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import { cn } from '@/shared/lib/cn'

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
  { id: 'billing', label: 'Billing', icon: CreditCard, href: '/settings/billing' },
  { id: 'sso', label: 'SSO', icon: KeyRound, href: '/settings/sso' },
  { id: 'org-security', label: 'Org security', icon: ShieldCheck, href: '/settings/org-security' },
  { id: 'integrations', label: 'Integrations', icon: Plug, href: '/settings/integrations' },
  { id: 'router-policy', label: 'Router policy', icon: Route, href: '/settings/router-policy' },
  { id: 'finetune', label: 'Fine-tune jobs', icon: TestTubeDiagonal, href: '/settings/finetune' },
]
const defaultAccountSectionId = accountSidebarSections[0]!.id
const defaultSettingsSectionId = settingsSidebarSections[0]!.id

export function AccountExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const activeSectionId = useActiveAccountSection()

  return (
    <div class="core-sidebar-dedicated-panel">
      <SidebarPanelTitle spacing="core-sidebar-title-spacious" onCollapse={props.onCollapse}>
        Account
      </SidebarPanelTitle>

      <SettingsSectionLinks
        ariaLabel="Account sections"
        sections={accountSidebarSections}
        activeSectionId={activeSectionId()}
      />
    </div>
  )
}

export function SettingsExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const location = useLocation()
  const activeSectionId = () => getSettingsActiveSectionId(location.pathname)

  return (
    <div class="core-sidebar-dedicated-panel">
      <SidebarPanelTitle spacing="core-sidebar-title-spacious" onCollapse={props.onCollapse}>
        Settings
      </SidebarPanelTitle>

      <SettingsSectionLinks
        ariaLabel="Settings sections"
        sections={settingsSidebarSections}
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
