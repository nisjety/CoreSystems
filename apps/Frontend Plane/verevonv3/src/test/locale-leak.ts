/**
 * Helpers for the English-locale leak smoke (chat-locale-smoke.test.tsx).
 *
 * `renderInLocale` mounts a tree under the app's I18nProvider with the reader's
 * language stored first, as the browser finds it on load. `expectNoNorwegian`
 * gathers the copy a reader or a screen reader gets from a rendered tree (its
 * text, and the names and hints in aria-label, aria-description, title,
 * placeholder and alt) and fails on any of it that reads as Norwegian UI copy.
 *
 * The check is a list of Norwegian letters and words, so it judges copy, not
 * language: a Norwegian word that is not on the list passes. Fixtures keep user
 * content free of the listed words, so a leak is never a fixture's own text.
 */
import type { JSX } from '@solidjs/web'
import { render } from '@solidjs/testing-library'
import { expect } from 'vitest'
import { I18nProvider, type Locale } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'

const NAMING_ATTRIBUTES = ['aria-label', 'aria-description', 'title', 'placeholder', 'alt'] as const
const NAMING_SELECTOR = NAMING_ATTRIBUTES.map((name) => `[${name}]`).join(',')

// The letters are Norwegian-only. The words are the labels, actions, states and
// file-type nouns the chat surfaces use; the nouns also match inside compounds
// such as `Excel-regneark`. Case is matched as the labels capitalise them. This
// is a regular expression because it detects Norwegian copy and never shows it:
// the UI copy gate reads Norwegian string literals as copy, and this file is not
// copy a reader sees.
const NORWEGIAN_COPY =
  /[æøåÆØÅ]|\b(?:Avbryt\w*|Avvis\w*|Effektklasse|Ekstern|Godkjen\w*|Handling|Henter|Ikke|Ingen|Kun|Kvittering|Last ned|Lagre\w*|Lukk\w*|Noen|Oppdater\w*|Skjul\w*|Steg|Ukjent|Utfall\w*|Verifis\w*|Vis|registrert)\b|(?:dokument|regneark|presentasjon|tekst|arkiv|bilde|lyd|filtype|fil)\b/

/** Renders `ui` under the app's I18nProvider, with the reader's language stored first. */
export function renderInLocale(locale: Locale, ui: () => JSX.Element) {
  window.localStorage.setItem(localeStorageKey, locale)
  return render(() => I18nProvider({
    get children() {
      return ui()
    },
  }))
}

/** The copy a reader or a screen reader gets from `root`: its text and its names and hints. */
export function readableCopy(root: Element): string[] {
  const copy: string[] = []
  const texts = root.ownerDocument.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  for (let node = texts.nextNode(); node; node = texts.nextNode()) {
    if (node.parentElement?.closest('script, style')) continue
    copy.push(node.textContent ?? '')
  }
  for (const element of root.querySelectorAll(NAMING_SELECTOR)) {
    for (const name of NAMING_ATTRIBUTES) {
      const value = element.getAttribute(name)
      if (value) copy.push(value)
    }
  }
  return copy.map((text) => text.replace(/\s+/g, ' ').trim()).filter((text) => text !== '')
}

/**
 * Fails on Norwegian copy anywhere in `root`. `except` lists Norwegian copy that
 * is known and not yet translated. Each entry must still be on the page, so a
 * string that is translated has to leave the list in the same change.
 */
export function expectNoNorwegian(root: Element, except: readonly string[] = []): void {
  const copy = readableCopy(root)
  const leaks = new Set(copy.filter((text) => NORWEGIAN_COPY.test(text) && !except.includes(text)))
  expect([...leaks]).toEqual([])
  for (const known of except) expect(copy).toContain(known)
}
