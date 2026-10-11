/**
 * Contextual prompt suggestions on the start screen (CONTRACT.md amendment F).
 *
 * The context-aware assistant opens on what happened, not on a generic
 * welcome. When Verevon has a relevant message, alert or observed change, the
 * start screen shows one to three concrete suggestions in place of the
 * general welcome text. Each one says what happened, where it came from and
 * when, and proposes the next action. A click FILLS the composer so the person
 * can read and edit the question; it never sends and never acts.
 *
 * The product's own start screen (`EmptyChatState`) already has the pieces:
 * the starter row (`verevon-chat-starter`, icon + title + small line) and a
 * click that only sets the composer input (`onSelectPrompt={setInput}`). It has
 * no source for contextual suggestions yet, so for the films the same markup
 * is built here and inserted into the real start screen's heading block, with
 * the product's own classes and tokens. Nothing in the chat components is
 * edited; the welcome paragraph is hidden by `scene.css`. The block lives
 * inside `.verevon-chat-empty__heading`, so the welcome cross-fade that copies
 * that heading on send carries the suggestion out with it.
 *
 * Scene mode only (this module is imported by the scene definitions).
 */
import { render } from '@solidjs/web'
import { ChevronRight, Package, Receipt } from '@/shared/icons'

export type SuggestionIcon = 'package' | 'receipt'

export type SceneSuggestion = {
  /** The headline: what happened and the proposed next action, as the question the composer receives. */
  text: string
  /** Where it came from, e.g. «Bring · sporingsvarsel». */
  source: string
  /** When, in the film's fictional clock, e.g. «i dag 08:12». */
  time: string
  icon: SuggestionIcon
}

export type SceneSuggestionSet = {
  /** Small label above the suggestions. */
  label: string
  items: SceneSuggestion[]
}

export const COMPOSER_TEXTAREA = 'textarea#verevon-dashboard-input'

/** Set the real textarea value and let the composer hear it as typed input. */
export function fillComposer(text: string): boolean {
  const textarea = document.querySelector<HTMLTextAreaElement>(COMPOSER_TEXTAREA)
  if (!textarea) return false
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set
  if (setter) setter.call(textarea, text)
  else textarea.value = text
  textarea.dispatchEvent(new Event('input', { bubbles: true }))
  return true
}

const ICONS: Record<SuggestionIcon, typeof Package> = { package: Package, receipt: Receipt }

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag)
  node.className = className
  if (text !== undefined) node.textContent = text
  return node
}

/** One suggestion as a real starter row (same classes as `EmptyChatState`'s starters). */
function buildSuggestion(item: SceneSuggestion, index: number): HTMLButtonElement {
  const button = element('button', 'verevon-chat-starter v-suggestion')
  button.type = 'button'
  button.dataset.suggestionIndex = String(index)

  const icon = element('span', 'verevon-chat-starter__icon')
  render(() => ICONS[item.icon]({ size: 15, strokeWidth: 1.9 }), icon)

  const copy = element('span', 'verevon-chat-starter__copy')
  copy.appendChild(element('strong', 'v-suggestion__text', item.text))
  const meta = element('small', 'v-suggestion__meta')
  meta.appendChild(element('span', 'v-suggestion__source', item.source))
  meta.appendChild(element('span', 'v-suggestion__dot', '·'))
  meta.appendChild(element('time', 'v-suggestion__time', item.time))
  copy.appendChild(meta)

  const chevron = element('span', 'v-suggestion__chevron')
  chevron.setAttribute('aria-hidden', 'true')
  render(() => ChevronRight({ size: 15 }), chevron)

  button.append(icon, copy, chevron)
  // The product's starter behaviour: fill the composer, nothing else.
  button.addEventListener('click', () => {
    if (!fillComposer(item.text)) return
    for (const other of document.querySelectorAll('.v-suggestion')) other.classList.remove('is-chosen')
    button.classList.add('is-chosen')
  })
  return button
}

/**
 * Keeps one suggestion block inside the start screen's heading for as long as
 * the start screen is mounted. Safe to call once, before the chat page renders.
 */
export function installPromptSuggestions(set: SceneSuggestionSet): void {
  const place = () => {
    const heading = document.querySelector<HTMLElement>('.verevon-chat-empty__heading')
    if (!heading || heading.querySelector('.v-suggestions')) return
    const block = element('div', 'v-suggestions')
    block.dataset.vSuggestions = ''
    block.setAttribute('role', 'group')
    block.setAttribute('aria-label', set.label)
    block.appendChild(element('span', 'v-suggestions__label', set.label))
    set.items.forEach((item, index) => block.appendChild(buildSuggestion(item, index)))
    heading.appendChild(block)
  }
  const observer = new MutationObserver(place)
  const start = () => {
    observer.observe(document.body, { childList: true, subtree: true })
    place()
  }
  if (document.body) start()
  else document.addEventListener('DOMContentLoaded', start, { once: true })
}
