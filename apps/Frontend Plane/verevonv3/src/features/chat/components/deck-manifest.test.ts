import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { DECK_MAX_SLIDES, type DeckManifest, parseDeckManifest, slideLines } from './deck-manifest'

/**
 * The vendored copy of mp-contracts' manifest fixture (`deck_manifest_v1.json`), pinned by sha256 beside it.
 * The checks below read only this tree, so a missing or edited copy fails here rather than passing by not running.
 */
const here = dirname(fileURLToPath(import.meta.url))
const contracts = join(here, '..', '..', '..', 'shared', 'api', 'contracts')
const fixtureBytes = readFileSync(join(contracts, 'deck_manifest_v1.json'))
const fixture = fixtureBytes.toString('utf8')

type Json = Record<string, unknown>

/** The fixture with one edit applied, as text. */
function edited(change: (manifest: Json) => void): string {
  const manifest = JSON.parse(fixture) as Json
  change(manifest)
  return JSON.stringify(manifest)
}

function slideOf(manifest: Json, index: number): Json {
  return (manifest.slides as Json[])[index] as Json
}

function parsed(): DeckManifest {
  const deck = parseDeckManifest(fixture)
  if (!deck) throw new Error('the vendored fixture must parse')
  return deck
}

describe('the vendored manifest contract', () => {
  it('is the pinned copy, byte for byte', () => {
    const pin = readFileSync(join(contracts, 'deck_manifest_v1.json.sha256'), 'utf8').split(/\s+/)[0]
    expect(createHash('sha256').update(fixtureBytes).digest('hex')).toBe(pin)
  })
})

describe('parseDeckManifest', () => {
  it('reads the v1 fixture into a deck the panel can draw', () => {
    expect(parsed()).toMatchObject({
      version: 1,
      deckId: 'deck-01jbx3q9k2m4n5p6r7s8t9v0wx',
      title: 'Kvartalstall Q3',
      locale: 'nb-NO',
      revision: 5,
      nextSlide: 6,
    })
    expect(parsed().slides.map((slide) => [slide.id, slide.layout, slide.state])).toEqual([
      ['s1', 'title', 'ok'],
      ['s3', 'title_bullets', 'warn'],
      ['s4', 'two_column', 'draft'],
      ['s5', 'image', 'error'],
    ])
    expect(parsed().slides[1]?.findings).toEqual([
      { id: 'text_may_overflow', message: 'text may not fit its box; shorten it or split the slide' },
    ])
    expect(parsed().slides[1]?.notes).toBe('Kilde: regnskapsrapport Q3, side 4.')
  })

  it('refuses another version, an unknown state or layout, and a value outside its type', () => {
    const refused = [
      edited((manifest) => {
        manifest.version = 2
      }),
      edited((manifest) => {
        slideOf(manifest, 0).state = 'done'
      }),
      edited((manifest) => {
        slideOf(manifest, 0).layout = 'cover'
      }),
      edited((manifest) => {
        manifest.revision = 1.5
      }),
      edited((manifest) => {
        manifest.next_slide = 0
      }),
      edited((manifest) => {
        slideOf(manifest, 0).bullets = 'not a list'
      }),
      edited((manifest) => {
        delete manifest.title
      }),
    ]
    for (const raw of refused) {
      expect(parseDeckManifest(raw)).toBeNull()
    }
  })

  it('refuses a deck over the slide cap, which the server would never have stored', () => {
    const over = edited((manifest) => {
      const slide = slideOf(manifest, 0)
      manifest.slides = Array.from({ length: DECK_MAX_SLIDES + 1 }, (_, index) => ({ ...slide, id: `s${index + 1}` }))
    })
    expect(parseDeckManifest(over)).toBeNull()
  })

  it('refuses text that is not JSON or not an object, without throwing', () => {
    for (const raw of ['', 'not json', '[]', 'null', '"deck"']) {
      expect(() => parseDeckManifest(raw), raw).not.toThrow()
      expect(parseDeckManifest(raw), raw).toBeNull()
    }
  })

  it('ignores the optional keys later slices add, so an older viewer still reads the slides', () => {
    const deck = parseDeckManifest(
      edited((manifest) => {
        manifest.template = { skill_id: 'brand' }
        manifest.source = { artifact_id: 'report', version: 2 }
      }),
    )
    expect(deck?.slides).toHaveLength(4)
    expect(deck).not.toHaveProperty('template')
    expect(deck).not.toHaveProperty('source')
  })
})

describe('slideLines', () => {
  it('lists the bullets of a slide, or a two-column slide left items then right items', () => {
    const [, bulleted, twoColumn] = parsed().slides
    expect(slideLines(bulleted!)).toEqual(['Omsetning 12,4 MNOK, opp 8 %', 'Nord vokste mest', 'Marginen holdt seg'])
    expect(slideLines(twoColumn!)).toEqual(['Mål: 11 MNOK', 'Resultat: 12,4 MNOK'])
  })
})
