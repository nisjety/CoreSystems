/**
 * Matching a visitor's message to a case: exact suggestion text, whole-word
 * trigger groups, normalisation, and the fallback for everything else. Pure
 * functions, no browser.
 */
import { describe, expect, it } from 'vitest'
import { EXACT_SCORE, matchScenario, normalizeText, scoreTriggers, termMatches, titleFromQuestion } from './match'
import { scenarios } from './scenarios'
import type { Scenario } from './scenario-types'

function idOf(text: string): string | undefined {
  return matchScenario(text, scenarios)?.scenario.id
}

/** The start-screen card text of a case: the exact suggestion, which scores at the top. */
function cardOf(id: string): string {
  const found = scenarios.find((scenario) => scenario.id === id)
  if (!found) throw new Error(`no case ${id}`)
  return found.suggestion.text
}

/** A trigger group as a visitor would type it: the words, with a stem's `*` dropped. */
function wordsOf(group: readonly string[]): string {
  return group.map((term) => term.replace(/\*$/, '')).join(' ')
}

/**
 * The customer's name as the case question opens with it: its first word («Solhaug» in
 * «Solhaug Hotell ber om …», «Havnebrygga» in «Havnebrygga Kafé melder …», «Nordhavn» in
 * «Nordhavn kommune ber om …»), normalised. The rest of the name («Hotell», «Kafé») and other
 * words of the question («tilbud», «bordplate») are common words, not the customer's name.
 */
function customerWordsOf(question: string): string[] {
  const first = question.trim().split(/\s+/)[0] ?? ''
  if (!/^[A-ZÆØÅ]/.test(first)) return []
  return normalizeText(first).split(' ').filter((word) => word.length > 0).slice(0, 1)
}

/**
 * Whether a single-term group names its case: an order or series number, or the
 * customer's name (see customerWordsOf). A common word never does.
 */
function namesCase(scenario: Scenario, term: string): boolean {
  if (/^\d+$/.test(term)) return true
  const stem = term.endsWith('*')
  const word = normalizeText(stem ? term.slice(0, -1) : term)
  // A stem is the whole customer word plus any ending (Solhaugs, Solhaughotellet), never a prefix of it.
  return customerWordsOf(scenario.suggestion.text).some((item) => item === word)
}

/**
 * What is wrong with a trigger group, or null when its shape is right. A single
 * term must be a number or the customer's name. A longer group has at least two
 * terms, and a group of generic words (no number, no customer name) has three.
 */
function shapeProblem(scenario: Scenario, group: readonly string[]): string | null {
  if (group.length === 0) return 'an empty group'
  if (group.length === 1) return namesCase(scenario, group[0]!) ? null : 'one term that is neither a number nor the customer'
  if (group.some((term) => namesCase(scenario, term))) return null
  return group.length >= 3 ? null : 'a group of generic words with fewer than three terms'
}

describe('normalizeText', () => {
  it('lowercases, folds æ ø å to ae o a, and turns punctuation into single spaces', () => {
    expect(normalizeText('Oppfølging, ÅRET – Bærum!')).toBe('oppfolging aret baerum')
  })

  it('removes other diacritics', () => {
    expect(normalizeText('Café méd')).toBe('cafe med')
  })

  it('reduces empty or punctuation-only text to nothing', () => {
    expect(normalizeText('  ?! ')).toBe('')
  })
})

describe('termMatches', () => {
  it('matches a whole word of the message, never a part of one', () => {
    expect(termMatches('bord', ['bord'])).toBe(true)
    expect(termMatches('bord', ['bordplate'])).toBe(false)
    expect(termMatches('bord', ['bordene'])).toBe(false)
  })

  it('matches every word that starts with a stem marked with *', () => {
    expect(termMatches('stol*', ['stoler'])).toBe(true)
    expect(termMatches('stol*', ['stolene'])).toBe(true)
    expect(termMatches('stol*', ['bord'])).toBe(false)
  })

  it('reads a term the way it reads the message: oppfølging is oppfolging', () => {
    expect(termMatches('oppfølging', ['oppfolging'])).toBe(true)
  })

  it('an empty term matches nothing', () => {
    expect(termMatches('  ', ['bord'])).toBe(false)
    expect(termMatches('*', ['bord'])).toBe(false)
  })
})

describe('matchScenario', () => {
  it.each(scenarios.map((scenario) => [scenario.id, scenario.suggestion.text] as const))(
    'scores the exact suggestion text of %s at the top score',
    (id, text) => {
      const match = matchScenario(text, scenarios)
      expect(match?.scenario.id).toBe(id)
      expect(match?.score).toBe(EXACT_SCORE)
    },
  )

  it('answers the quote case when the customer is named, and falls back on quote words without the customer', () => {
    expect(idOf('Kan dere sende et tilbud på 40 stoler til Solhaug?')).toBe('tilbud-solhaug')
    expect(idOf('Hva med tilbudet til Solhaug?')).toBe('tilbud-solhaug')
    for (const question of [
      'Vi trenger tilbudet til hotellet før åpningen.',
      'Kan dere levere tilbudet med bord og stoler?',
      'Kan dere sende et tilbud på 40 stoler til et hotell?',
      'TILBUD til Hotell: 40 STOLER!',
      'Hei, kan vi få et tilbud på bord for hotellet?',
    ]) {
      expect(matchScenario(question, scenarios), question).toBeNull()
    }
  })

  it('tolerates capitals, Norwegian letters and punctuation in the message', () => {
    expect(idOf('SOLHAUG HOTELL: 40 STOLER!')).toBe('tilbud-solhaug')
    expect(idOf('Hei, kan vi få et tilbud på 10 bord til Solhaug før åpningen?')).toBe('tilbud-solhaug')
  })

  it('needs every term of a group, so a single common word is not enough', () => {
    expect(idOf('Tilbud')).toBeUndefined()
    expect(idOf('Hvordan lager jeg et bord?')).toBeUndefined()
  })

  it('a common word of a case never answers on its own, nor does a series number without the word serie', () => {
    for (const word of [
      'tilbud', 'stoler', 'bord', 'hotell', 'reklamasjon', 'dokumentasjon', 'garanti', 'bordplate', 'sprekk',
      'serie', '2609', 'kommune', 'miljødokumentasjon', 'FSC', 'EPD', 'anbud',
    ]) {
      expect(idOf(word), word).toBeUndefined()
    }
  })

  it('answers with the case that matches the most groups', () => {
    expect(matchScenario('Havnebrygga serie 2609 og Nordhavn', scenarios)?.score).toBe(2)
    expect(idOf('Havnebrygga serie 2609 og Nordhavn')).toBe('reklamasjon-bordplate')
    expect(matchScenario('Solhaug tilbud stoler bord', scenarios)?.score).toBe(1)
    expect(idOf('Solhaug tilbud stoler bord')).toBe('tilbud-solhaug')
  })

  it('falls back for questions that only share a fragment of a case word, instead of answering with a case', () => {
    for (const question of [
      'Hva koster det å bringe varer til Bergen?',
      'Hva er innboksen min?',
      'Hva er et tilbud?',
      'Hva er tilbudet?',
      'Hvor mange stoler har dere på lager?',
      'Hvordan lager jeg et bord?',
    ]) {
      expect(matchScenario(question, scenarios), question).toBeNull()
    }
  })

  it('returns null for unrelated, empty or blank text, so the fallback answers', () => {
    expect(matchScenario('Hva er været i Bergen i morgen?', scenarios)).toBeNull()
    expect(matchScenario('', scenarios)).toBeNull()
    expect(matchScenario('   ', scenarios)).toBeNull()
  })

  it('keeps the cases apart: no case’s triggers fire on another case’s question', () => {
    for (const question of scenarios) {
      for (const other of scenarios) {
        if (other.id === question.id) continue
        expect(scoreTriggers(normalizeText(question.suggestion.text), other.triggers)).toBe(0)
      }
    }
  })
})

/**
 * Free text a visitor types that must not answer any case, against the real
 * registry: questions on a case's topic that do not name it, and questions that
 * belong to no case. Each one falls back to the honest demo reply.
 */
const NEAR_MISSES: readonly string[] = [
  'FSC EPD anbud',
  'Har dere gyldig FSC og EPD til anbudet?',
  'Hva må vi levere av FSC og EPD til anbud hos Bergen kommune?',
  'fsc epd anbudsfrist',
  // The quote case's topic, without its customer.
  'Hva koster et tilbud på bord?',
  'Lag et tilbud til et hotell i Bergen',
  'Kan dere gi et tilbud på 40 stoler?',
  'Hva er et tilbud?',
  'Hvordan lager jeg et tilbud i systemet?',
  'Hvem godkjenner tilbud over 100 000 kroner?',
  'Hvor lang tid tar det å levere 40 stoler til et hotell?',
  'Hva er rabatten på større bestillinger av stoler?',
  'Hvor mange bord trenger et hotell i gjennomsnitt?',
  'Hva er leveringstiden på bord til et hotell?',
  'Montering av stoler: hvor lang tid tar det?',
  'bestill stoler',
  // The warranty case's topic, without its customer or its series pair.
  'Hva er garantien på bordplater?',
  'Hvordan fjerner jeg en sprekk i bordplaten?',
  'Hva gjør vi med en reklamasjon på et bord?',
  'Hvor lenge er garantien på en bordplate?',
  'Kunden klager på en sprekk i bordplaten, hva nå?',
  'En kafé vil ha erstatning for en skadet bordplate.',
  'Limskjøtene på bordplatene slipper, hva skjer da?',
  'Hvilke serier av bord har feil i limskjøtene?',
  'hva betyr serie 2610',
  'Hva betyr serienummer 2609?',
  'Kan dere se på en sprekk i en bordplate fra 2609?',
  'Hva er 2609?',
  'Hvordan fungerer garantien på møbler?',
  'kafe bestilling',
  // The tender case's topic, without its customer or all three of its words.
  'Dokumentasjon til anbud for Bergen kommune',
  'kan du hjelpe meg med anbud',
  'Har dere FSC og EPD for kjøkkenbenken?',
  'hva er en EPD',
  'Trenger vi sertifikat',
  'Hva er et anbud?',
  'Hva er FSC-sertifisering?',
  'Har dere EPD på stolene?',
  'Hva betyr FSC-merket?',
  'Er EPD-en for stolene fortsatt gyldig?',
  'Hvilke miljøsertifikater har dere?',
  'Kan vi bruke FSC-merket i markedsføring?',
  'Hva er forskjellen mellom FSC og PEFC?',
  'Frist for anbud på kontorstoler, når er den?',
  'Vi skal levere anbud til Bergen kommune i november.',
  'Hvor finner jeg miljødokumentasjon for bord?',
  'Hva koster en EPD-verifisering?',
  'Hvordan søker vi om å delta i en offentlig konkurranse?',
  'Hvem i Bergen kommune kan vi ringe om innkjøp?',
  'Har vi gyldige FSC og EPD-bevis til kjøkkenbenken?',
  'Kan dere sende miljødokumentasjon til en anbudskonkurranse?',
  'Trenger vi miljødokumentasjon til anbudet?',
  // Questions that belong to no case.
  'Hva er været i Bergen i morgen?',
  'Hvem er Ida?',
  'Hva er innboksen min?',
  'Hvordan logger jeg meg inn?',
  'Hva er klokken i Oslo?',
  'Skriv en kort hilsen til en kunde.',
  'Hva koster det å sende en pall til Bergen?',
  'Kan du oppsummere dagens e-poster?',
]

/**
 * Free text that names a case, and must answer it: the card text of each case, the
 * customer's name alone or with other capitals and punctuation, with or without the
 * Norwegian letters, the series pair, and FSC, EPD and anbud together.
 */
const MUST_ANSWER: ReadonlyArray<readonly [string, string]> = [
  [cardOf('tilbud-solhaug'), 'tilbud-solhaug'],
  [cardOf('reklamasjon-bordplate'), 'reklamasjon-bordplate'],
  [cardOf('anbud-miljodokumentasjon'), 'anbud-miljodokumentasjon'],
  ['Solhaug', 'tilbud-solhaug'],
  ['solhaug', 'tilbud-solhaug'],
  ['SOLHAUG!', 'tilbud-solhaug'],
  ['solhaug hotell', 'tilbud-solhaug'],
  ['Hva med tilbudet til Solhaug?', 'tilbud-solhaug'],
  ['Kan dere sende et tilbud på 40 stoler til Solhaug?', 'tilbud-solhaug'],
  ['Hei, kan vi få et tilbud på 10 bord til Solhaug før åpningen?', 'tilbud-solhaug'],
  ['Hei, kan vi fa et tilbud pa 10 bord til Solhaug for apningen?', 'tilbud-solhaug'],
  ['Havnebrygga', 'reklamasjon-bordplate'],
  ['Havnebrygga Kafe', 'reklamasjon-bordplate'],
  ['Havnebrygga Kafé', 'reklamasjon-bordplate'],
  ['HAVNEBRYGGA KAFÉ!', 'reklamasjon-bordplate'],
  ['havnebrygga-kafé: sprekk i bordplaten?', 'reklamasjon-bordplate'],
  ['Havnebrygga Kafé: hva må gjøres med bordet?', 'reklamasjon-bordplate'],
  ['Havnebrygga Kafe: hva ma gjores med bordet?', 'reklamasjon-bordplate'],
  ['Serie 2609', 'reklamasjon-bordplate'],
  ['serie 2609', 'reklamasjon-bordplate'],
  ['SERIE-2609: sprekk i bordplaten?', 'reklamasjon-bordplate'],
  ['Hva betyr serie 2609 for garantien?', 'reklamasjon-bordplate'],
  ['Serie 2609 har feil i limskjøtene', 'reklamasjon-bordplate'],
  ['Nordhavn', 'anbud-miljodokumentasjon'],
  ['NORDHAVN kommune', 'anbud-miljodokumentasjon'],
  ['nordhavn-anbudet', 'anbud-miljodokumentasjon'],
  ['Nordhavn kommune: hva trenger vi til tenderen?', 'anbud-miljodokumentasjon'],
  ['Nordhavns kommune sitt anbud', 'anbud-miljodokumentasjon'],
  ['Kan du sjekke saken til Nordhavns kommune om anbudet?', 'anbud-miljodokumentasjon'],
  ['Solhaugs tilbud: er det sendt til kunden?', 'tilbud-solhaug'],
  ['Hva med Solhaughotellet sitt tilbud?', 'tilbud-solhaug'],
  ['Havnebryggas bordplate har fått en sprekk, hva må gjøres?', 'reklamasjon-bordplate'],
  ['Havnebrygga-saken', 'reklamasjon-bordplate'],
]

describe('the near misses, against the real registry', () => {
  it('holds at least forty realistic questions', () => {
    expect(NEAR_MISSES.length).toBeGreaterThanOrEqual(40)
  })

  it.each(NEAR_MISSES)('falls back on «%s»', (question) => {
    expect(matchScenario(question, scenarios), question).toBeNull()
  })
})

describe('the free text that names a case', () => {
  it.each(MUST_ANSWER)('answers «%s» with %s', (question, id) => {
    expect(idOf(question), question).toBe(id)
  })
})

describe.each(scenarios.map((scenario) => [scenario.id, scenario] as const))('the trigger groups of %s', (_id, scenario) => {
  it.each(scenario.triggers.map((group) => [group.join(' '), group] as const))(
    'the words «%s» answer this case',
    (_label, group) => {
      expect(idOf(wordsOf(group))).toBe(scenario.id)
    },
  )
})

describe('the trigger groups of the cases', () => {
  it('every term is one word in the normalised form, with at most a trailing * for a stem', () => {
    for (const scenario of scenarios) {
      for (const group of scenario.triggers) {
        for (const term of group) expect(term, scenario.id).toMatch(/^[a-z0-9]+\*?$/)
      }
    }
  })

  it('every group is a number or the customer’s name alone, or at least two words, and a group of generic words has at least three terms', () => {
    for (const scenario of scenarios) {
      for (const group of scenario.triggers) {
        expect(shapeProblem(scenario, group), `${scenario.id}: ${group.join(' ')}`).toBeNull()
      }
    }
  })

  it('the shape rule refuses a common word and a pair of common words, and accepts a customer, a number and three generic words', () => {
    const quote = scenarios.find((scenario) => scenario.id === 'tilbud-solhaug')!
    expect(shapeProblem(quote, ['tilbud'])).not.toBeNull()
    expect(shapeProblem(quote, ['tilbud*', 'stoler'])).not.toBeNull()
    expect(shapeProblem(quote, ['solhaug'])).toBeNull()
    expect(shapeProblem(quote, ['solhaug*'])).toBeNull()
    expect(shapeProblem(quote, ['sol*'])).not.toBeNull()
    expect(shapeProblem(quote, ['hotell'])).not.toBeNull()
    expect(shapeProblem(quote, ['2609'])).toBeNull()
    expect(shapeProblem(quote, ['tilbud*', 'stoler', 'bord'])).toBeNull()
  })

  it('every case has at least one trigger group, and a case may have a single group', () => {
    for (const scenario of scenarios) expect(scenario.triggers.length, scenario.id).toBeGreaterThanOrEqual(1)
  })
})

describe('titleFromQuestion', () => {
  it('keeps a short question as it is, on one line', () => {
    expect(titleFromQuestion('  Hei\n  der  ')).toBe('Hei der')
  })

  it('cuts a long question to 60 characters with an ellipsis', () => {
    const title = titleFromQuestion('a'.repeat(80))
    expect(title).toHaveLength(60)
    expect(title.endsWith('…')).toBe(true)
  })

  it('names an empty question «Ny samtale»', () => {
    expect(titleFromQuestion('   ')).toBe('Ny samtale')
  })
})
