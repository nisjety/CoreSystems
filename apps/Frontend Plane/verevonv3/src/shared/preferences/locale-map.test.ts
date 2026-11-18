import { describe, expect, it } from 'vitest'
import { languageFromLocale, localeFromLanguage } from '@/shared/preferences/locale-map'

// WS16-S3: the server's stored language tag maps onto the two SPA locales. Norwegian tags map to
// 'no', English tags to 'en'. Any other tag maps to nothing, so the SPA leaves that server value alone.

describe('localeFromLanguage', () => {
  it.each(['nb', 'nb-NO', 'nn', 'nn-NO', 'no', 'no_NO', 'NB-no'])('maps the Norwegian tag %s to no', (tag) => {
    expect(localeFromLanguage(tag)).toBe('no')
  })

  it.each(['en', 'en-US', 'en-GB', 'EN'])('maps the English tag %s to en', (tag) => {
    expect(localeFromLanguage(tag)).toBe('en')
  })

  it.each(['fr-FR', 'de-DE', '', '   ', null, undefined])('leaves the unmapped or empty value %s untouched', (tag) => {
    expect(localeFromLanguage(tag)).toBeUndefined()
  })
})

describe('languageFromLocale', () => {
  it('writes the server tag for each SPA locale', () => {
    expect(languageFromLocale('no')).toBe('nb-NO')
    expect(languageFromLocale('en')).toBe('en-US')
  })
})
