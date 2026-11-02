import { useNavigate, useParams } from '@solidjs/router'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { VerevonWorkspaceSettingsPage } from '@/features/settings/components/WorkspaceSettingsPage'
import { isWorkspaceSettingsSection, type SectionDetail } from '@/features/settings/lib/settings-sections'
import { searchSettingsSections } from '@/features/settings/lib/settings-search'
import { useI18n } from '@/shared/i18n'
import { hasPlatformAdminAccess } from '@/shared/session/access'
import { getSession } from '@/shared/session/session-store'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'

const resultsId = 'settings-search-results'
const optionId = (sectionId: string) => `settings-search-option-${sectionId}`

export default function SettingsPage() {
  const params = useParams<{ section?: string }>()
  const section = () => {
    const value = params.section ?? ''
    return isWorkspaceSettingsSection(value)
      ? value
      : 'workspace'
  }

  // A column, so the search bar keeps its own height and the settings surface,
  // which scrolls, takes the rest of the panel.
  return (
    <div style={{ display: 'flex', 'flex-direction': 'column', height: '100%' }}>
      <SettingsSearch />
      {/* Keyed, so a jump to another section starts that page at the top. */}
      <Show when={section()} keyed>
        {(current) => <VerevonWorkspaceSettingsPage section={current} />}
      </Show>
    </div>
  )
}

function SettingsSearch() {
  const i18n = useI18n()
  const navigate = useNavigate()
  const [query, setQuery] = createSignal('')
  const [activeIndex, setActiveIndex] = createSignal(0)
  const [open, setOpen] = createSignal(false)
  // The sidebar lists the platform users page only to platform admins; search
  // must not offer a page the navigation withholds.
  const results = createMemo(() =>
    searchSettingsSections(query()).filter(
      (hit) => hit.id !== 'platform-users' || hasPlatformAdminAccess(getSession()),
    ),
  )
  const showResults = () => open() && query().trim() !== ''
  const activeOptionId = () => {
    const hit = results()[activeIndex()]
    return showResults() && hit ? optionId(hit.id) : undefined
  }

  function jump(hit: SectionDetail) {
    setQuery('')
    setOpen(false)
    navigate(`/settings/${hit.id}`)
  }

  function handleKeyDown(event: KeyboardEvent) {
    const count = results().length
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      if (count === 0) return
      event.preventDefault()
      const step = event.key === 'ArrowDown' ? 1 : count - 1
      setActiveIndex((activeIndex() + step) % count)
      setOpen(true)
    } else if (event.key === 'Enter') {
      const hit = results()[activeIndex()]
      if (!hit || !showResults()) return
      event.preventDefault()
      jump(hit)
    } else if (event.key === 'Escape') {
      event.preventDefault()
      setQuery('')
      setOpen(false)
    }
  }

  return (
    <div
      style={{
        'flex-shrink': 0,
        'box-sizing': 'border-box',
        'max-width': '960px',
        width: '100%',
        margin: '0 auto',
        padding: '16px 20px 0',
      }}
    >
      <div style={{ position: 'relative' }}>
        <VerevonInput
          type="text"
          role="combobox"
          class="verevon-settings-input"
          aria-label={i18n.tr('Søk i innstillinger', 'Search settings')}
          aria-autocomplete="list"
          aria-expanded={showResults() ? 'true' : 'false'}
          aria-controls={showResults() && results().length > 0 ? resultsId : undefined}
          aria-activedescendant={activeOptionId()}
          autocomplete="off"
          spellcheck={false}
          placeholder={i18n.tr('Søk i innstillinger', 'Search settings')}
          value={query()}
          onInput={(event) => {
            setQuery(event.currentTarget.value)
            setActiveIndex(0)
            setOpen(true)
          }}
          onFocus={() => setOpen(true)}
          onBlur={() => setOpen(false)}
          onKeyDown={handleKeyDown}
        />
        <Show when={showResults()}>
          <div
            class="verevon-floating-panel"
            style={{
              position: 'absolute',
              top: 'calc(100% + 6px)',
              left: 0,
              right: 0,
              'z-index': 40,
              'max-height': 'min(60vh, 360px)',
              'overflow-y': 'auto',
              padding: '6px',
            }}
          >
            <Show
              when={results().length > 0}
              fallback={
                <p class="verevon-settings-panel-note" role="status" style={{ margin: '10px 12px' }}>
                  {i18n.tr('Ingen innstillinger samsvarer med søket.', 'No settings match your search.')}
                </p>
              }
            >
              <ul
                id={resultsId}
                role="listbox"
                aria-label={i18n.tr('Treff i innstillinger', 'Matching settings')}
                style={{ margin: 0, padding: 0, 'list-style': 'none' }}
              >
                <For each={results()}>
                  {(hit, index) => (
                    <li
                      id={optionId(hit.id)}
                      role="option"
                      aria-selected={index() === activeIndex() ? 'true' : 'false'}
                      class="verevon-menu-item"
                      style={{ background: index() === activeIndex() ? 'var(--verevon-surface-soft)' : 'transparent' }}
                      onMouseDown={(event) => event.preventDefault()}
                      onMouseEnter={() => setActiveIndex(index())}
                      onClick={() => jump(hit)}
                    >
                      {hit.label}
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </div>
        </Show>
      </div>
    </div>
  )
}
