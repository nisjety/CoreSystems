/**
 * The deck manifest the artifact panel reads (EP-5 S4): a pure parser, with no DOM and no i18n.
 *
 * It mirrors `DeckManifest` in mp-contracts (`deck_manifest.rs`, manifest v1). The model gateway keeps the
 * manifest as the content of a `deck` artifact, and the panel checks it before anything is drawn: a manifest
 * that does not parse, breaks the shape, or is a version this viewer does not read is refused whole, so the
 * panel shows its raw text rather than a half-read deck. Unknown keys are dropped, not refused, so the
 * optional keys later slices add to v1 (the template, the source) do not stop an older viewer from reading
 * the slides.
 */
import { z } from 'zod'

/** The artifact kind a deck is kept under in the thread. */
export const DECK_ARTIFACT_KIND = 'deck'

/** The one manifest version this viewer reads. */
export const DECK_MANIFEST_VERSION = 1

/** Most slides in one deck (EP-5 decision D11, `MAX_SLIDES` in mp-contracts). */
export const DECK_MAX_SLIDES = 40

const SLIDE_LAYOUTS = ['title', 'title_bullets', 'title_only', 'section', 'two_column', 'image'] as const
const SLIDE_STATES = ['draft', 'ok', 'warn', 'error'] as const

const findingSchema = z.object({ id: z.string(), message: z.string() })

const slideSchema = z.object({
  id: z.string().min(1).max(64),
  layout: z.enum(SLIDE_LAYOUTS),
  title: z.string(),
  bullets: z.array(z.string()),
  left: z.array(z.string()),
  right: z.array(z.string()),
  image: z.string().nullable(),
  notes: z.string().nullable(),
  state: z.enum(SLIDE_STATES),
  findings: z.array(findingSchema),
})

const manifestSchema = z.object({
  version: z.literal(DECK_MANIFEST_VERSION),
  deck_id: z.string().min(1),
  title: z.string(),
  locale: z.string(),
  slides: z.array(slideSchema).max(DECK_MAX_SLIDES),
  revision: z.number().int().nonnegative(),
  next_slide: z.number().int().positive(),
})

export type DeckSlide = z.infer<typeof slideSchema>
export type DeckSlideLayout = DeckSlide['layout']
export type DeckSlideState = DeckSlide['state']

/** A deck as the panel holds it. The wire's snake_case names are mapped to camelCase; slide fields are one word. */
export type DeckManifest = {
  version: typeof DECK_MANIFEST_VERSION
  deckId: string
  title: string
  locale: string
  slides: DeckSlide[]
  revision: number
  nextSlide: number
}

/**
 * The manifest in `raw`, or `null` when it is not JSON, is another version, or breaks the shape or a cap.
 * Never throws: the caller draws the raw text for a `null`.
 */
export function parseDeckManifest(raw: string): DeckManifest | null {
  let json: unknown
  try {
    json = JSON.parse(raw)
  } catch {
    return null
  }
  const parsed = manifestSchema.safeParse(json)
  if (!parsed.success) return null
  const manifest = parsed.data
  return {
    version: manifest.version,
    deckId: manifest.deck_id,
    title: manifest.title,
    locale: manifest.locale,
    slides: manifest.slides,
    revision: manifest.revision,
    nextSlide: manifest.next_slide,
  }
}

/** The text lines of a slide in reading order: its bullets, or a two-column slide's left items then its right items. */
export function slideLines(slide: DeckSlide): string[] {
  return [...slide.bullets, ...slide.left, ...slide.right]
}
