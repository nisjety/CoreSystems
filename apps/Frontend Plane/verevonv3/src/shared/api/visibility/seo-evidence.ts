import { isPublicSitePage } from '../marketing/discovery-client'
export type SEOEvidence = {
  version: 'seo_page_v1'; headings: { level: number; text: string }[]; headingsPartial: boolean
  noindex: boolean; nofollow: boolean; robotsStatus: number; wordCount: number
  alternates: { lang: string; url: string }[]; invalidAlternates: number; alternatesPartial: boolean
  structuredData: { documents: number; invalid: number; types: string[]; partial: boolean }
  redirects: { url: string; status: number; target: string }[]; canonicalOutsideSite: boolean
}
const object = (v: unknown): Record<string, unknown> => { if (!v || typeof v !== 'object' || Array.isArray(v)) throw Error('Invalid SEO evidence'); return v as Record<string, unknown> }
const text = (v: unknown, max: number) => { if (typeof v !== 'string' || v.length > max || /[\x00-\x1f\x7f]/.test(v)) throw Error('Invalid SEO text'); return v }
const count = (v: unknown, max: number) => { if (!Number.isSafeInteger(v) || Number(v) < 0 || Number(v) > max) throw Error('Invalid SEO count'); return Number(v) }
const flag = (v: unknown) => { if (typeof v !== 'boolean') throw Error('Invalid SEO flag'); return v }
const list = (v: unknown, max: number) => { if (!Array.isArray(v) || v.length > max) throw Error('Invalid SEO list'); return v as unknown[] }
const page = (site: string, v: unknown) => { if (!isPublicSitePage(site, v)) throw Error('Unexpected SEO page'); return v }
/** Rebuild structural evidence; never forward HTML, JSON-LD values or provider extras. */
export function sanitizeSEO(value: unknown, site: string): SEOEvidence {
  const v = object(value), schema = object(v.structuredData)
  if (v.version !== 'seo_page_v1') throw Error('Unknown SEO evidence version')
  const documents = count(schema.documents, 32), invalid = count(schema.invalid, documents)
  const headings = list(v.headings, 200).map(raw => { const h = object(raw), level = count(h.level, 6); if (level < 1) throw Error('Invalid heading'); return { level, text: text(h.text, 2048) } })
  const redirects = list(v.redirects, 3).map(raw => { const r = object(raw), status = count(r.status, 599); if (![301, 302, 303, 307, 308].includes(status)) throw Error('Invalid redirect'); return { url: page(site, r.url), status, target: page(site, r.target) } })
  const robotsStatus = count(v.robotsStatus, 599); if (robotsStatus !== 0 && robotsStatus < 100) throw Error('Invalid robots status')
  return { version: 'seo_page_v1', headings, headingsPartial: flag(v.headingsPartial), noindex: flag(v.noindex), nofollow: flag(v.nofollow), robotsStatus, wordCount: count(v.wordCount, 1048576), alternates: list(v.alternates, 50).map(raw => { const a = object(raw); return { lang: text(a.lang, 40), url: page(site, a.url) } }), invalidAlternates: count(v.invalidAlternates, 50), alternatesPartial: flag(v.alternatesPartial), structuredData: { documents, invalid, types: list(schema.types, 32).map(v => text(v, 128)), partial: flag(schema.partial) }, redirects, canonicalOutsideSite: flag(v.canonicalOutsideSite) }
}
