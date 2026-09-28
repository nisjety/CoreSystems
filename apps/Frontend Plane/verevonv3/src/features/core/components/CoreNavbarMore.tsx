import { createEffect, Show } from 'solid-js'
import { Languages, MoreHorizontal, MoonStar, Sparkles, Sun } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { ChatAppearancePreview } from '@/features/chat/components/ChatAppearancePreview'
import type { ThemePayload } from '@/shared/api/navbar-client'

export function CoreNavbarMore(props: {
  theme: ThemePayload['theme']
  onThemeChange: (theme: ThemePayload['theme']) => void
  onAssistant: () => void
}) {
  const i18n = useI18n()
  let root: HTMLDetailsElement | undefined
  let trigger: HTMLElement | undefined
  createEffect(() => undefined, () => {
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && root?.open && !root.contains(event.target)) root.open = false
    }
    document.addEventListener('pointerdown', outside)
    return () => document.removeEventListener('pointerdown', outside)
  })
  return (
    <details ref={root} class="core-navbar-more" onKeyDown={(event) => {
      if (event.key === 'Escape' && root?.open) {
        event.stopPropagation()
        root.open = false
        trigger?.focus()
      }
      if (!root?.open || !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
      const controls = [...root.querySelectorAll<HTMLElement>('button:not(:disabled), summary, input')].filter((item) => item !== trigger && item.getClientRects().length)
      if (!controls.length) return
      event.preventDefault()
      const index = controls.indexOf(document.activeElement as HTMLElement)
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? controls.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + controls.length) % controls.length
      controls[next]?.focus()
    }}>
      <summary ref={trigger} title={i18n.tr('Flere valg', 'More options')} aria-label={i18n.tr('Flere valg', 'More options')}>
        <MoreHorizontal size={21} />
      </summary>
      <section class="core-navbar-more__panel" aria-label={i18n.tr('Flere valg', 'More options')}>
        <Show when={import.meta.env.DEV}><ChatAppearancePreview embedded /></Show>
        <button type="button" onClick={i18n.toggleLocale}><Languages size={18} /><span>{i18n.tr('Språk', 'Language')}</span><small>{i18n.localeCode()}</small></button>
        <fieldset class="core-navbar-more__theme">
          <legend>{i18n.tr('Fargetema', 'Theme')}</legend>
          <button type="button" aria-pressed={props.theme === 'light' ? 'true' : 'false'} onClick={() => props.onThemeChange('light')}><Sun size={16} />{i18n.tr('Lyst', 'Light')}</button>
          <button type="button" aria-pressed={props.theme === 'dark' ? 'true' : 'false'} onClick={() => props.onThemeChange('dark')}><MoonStar size={16} />{i18n.tr('Mørkt', 'Dark')}</button>
          <button type="button" aria-pressed={props.theme === 'system' ? 'true' : 'false'} onClick={() => props.onThemeChange('system')}>{i18n.tr('System', 'System')}</button>
        </fieldset>
        <button type="button" onClick={() => { if (root) root.open = false; props.onAssistant() }}><Sparkles size={18} /><span>{i18n.tr('AI-assistent', 'AI assistant')}</span></button>
      </section>
    </details>
  )
}
