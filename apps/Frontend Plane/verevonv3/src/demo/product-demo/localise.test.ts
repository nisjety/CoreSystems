// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest'
import { localiseDemoText, startDemoLocalisation } from './localise'

describe('demo localisation of the Arbeid rows', () => {
  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('translates exactly the known English row texts', () => {
    expect(localiseDemoText('Compose response')).toBe('Skriver svar')
    expect(localiseDemoText('  Completed.  ')).toBe('Fullført.')
    expect(localiseDemoText('Output')).toBe('Resultat')
    expect(localiseDemoText('Kilder')).toBeUndefined()
    expect(localiseDemoText('Compose response and more')).toBeUndefined()
  })

  it('translates the connection, search, stop and verification rows the product writes', () => {
    expect(localiseDemoText('Connect stream')).toBe('Kobler til agentstrømmen')
    expect(localiseDemoText('Source found')).toBe('Kilde funnet')
    expect(localiseDemoText('Stopped by the user.')).toBe('Stoppet av brukeren.')
    expect(localiseDemoText('Stream interrupted')).toBe('Strømmen ble avbrutt.')
    expect(localiseDemoText('Search')).toBe('Søk')
    expect(localiseDemoText('Web search is available; it runs only if the answer needs fresh data.')).toBe(
      'Nettsøk er tilgjengelig; det kjøres bare hvis svaret trenger ferske data.',
    )
    expect(localiseDemoText('Answer verification')).toBe('Kontroll av svaret')
    expect(localiseDemoText('No web search needed — answered from existing knowledge or other tools.')).toBe(
      'Ingen nettsøk nødvendig: svaret bygger på eksisterende kunnskap eller andre verktøy.',
    )
  })

  it('translates the technical rows of the run, named as the product names each event type', () => {
    expect(localiseDemoText('run · connected')).toBe('kjøring · tilkoblet')
    expect(localiseDemoText('thread · titled')).toBe('samtale · fikk tittel')
    expect(localiseDemoText('tool · call')).toBe('verktøy · kall')
    expect(localiseDemoText('tool · result')).toBe('verktøy · resultat')
    expect(localiseDemoText('citation · added')).toBe('kilde · lagt til')
    expect(localiseDemoText('message · delta')).toBe('melding · tekst')
    expect(localiseDemoText('message · done')).toBe('melding · ferdig')
    expect(localiseDemoText('step · updated')).toBe('steg · oppdatert')
    expect(localiseDemoText('artifact · updated')).toBe('dokument · oppdatert')
    expect(localiseDemoText('run · paused')).toBe('kjøring · satt på pause')
    expect(localiseDemoText('run · paused · approval')).toBe('kjøring · satt på pause · godkjenning')
  })

  it('leaves an event type or pause kind it does not know in English, so the scan still reports it', () => {
    expect(localiseDemoText('run · vanished')).toBeUndefined()
    expect(localiseDemoText('run · paused · sleeping')).toBeUndefined()
    expect(localiseDemoText('run · connected now')).toBeUndefined()
  })

  it('keeps the count of a search or verification row, in the singular and the plural', () => {
    expect(localiseDemoText('Web search captured 1 source.')).toBe('Nettsøk fant 1 kilde.')
    expect(localiseDemoText('Web search captured 3 sources.')).toBe('Nettsøk fant 3 kilder.')
    expect(localiseDemoText('Checked against 1 web source shown in Kilder.')).toBe('Kontrollert mot 1 nettkilde vist i Kilder.')
    expect(localiseDemoText('Checked against 4 web sources shown in Kilder.')).toBe('Kontrollert mot 4 nettkilder vist i Kilder.')
  })

  it('keeps the question of a preparing row and translates the regenerate row', () => {
    expect(localiseDemoText('Preparing "Hei der".')).toBe('Forbereder «Hei der».')
    expect(localiseDemoText('Regenerating latest response.')).toBe('Lager siste svar på nytt.')
  })

  it('leaves text alone that only resembles a known row, or that the map inherits from Object', () => {
    expect(localiseDemoText('Web search captured many sources.')).toBeUndefined()
    expect(localiseDemoText('Search for a supplier')).toBeUndefined()
    expect(localiseDemoText('constructor')).toBeUndefined()
    expect(localiseDemoText('toString')).toBeUndefined()
  })

  it('rewrites existing, added and updated text nodes', async () => {
    document.body.innerHTML = '<p id="a">Model selected</p>'
    const stop = startDemoLocalisation(document.body)
    expect(document.getElementById('a')?.textContent).toBe('Modell valgt')

    const added = document.createElement('p')
    added.textContent = 'Streaming answer text.'
    document.body.append(added)
    await Promise.resolve()
    expect(added.textContent).toBe('Skriver svaret.')

    const textNode = added.firstChild as Text
    textNode.data = 'Completed.'
    await Promise.resolve()
    expect(added.textContent).toBe('Fullført.')
    stop()
  })
})
