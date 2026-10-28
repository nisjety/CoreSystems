/**
 * Case 1 · Tilbud til Solhaug Hotell: fixtures for the interactive demo (src/demo/live).
 *
 * A sales case, unlike the two films (a late delivery, an invoice deviation). A hotel
 * writes to Fjordform AS and asks for a quote on 40 chairs and 10 tables before its
 * opening. Verevon checks stock, the price list and the assembly calendar in parallel
 * (a Do turn with three sub-tasks), builds the quote and a reply, and waits for a
 * person to approve before anything is reserved or sent.
 *
 * The numbers agree and are checked in scenarios/tilbud-solhaug.test.ts: 40 × 1 490 + 10 × 4 200 =
 * 101 600; 8 % volume discount = 8 128; plus assembly 2 × 3 600 = 7 200 gives
 * 100 672 excl. VAT; VAT 25 % = 25 168; total 125 840. Dates: today is Friday
 * 9 October 2026; the last chairs are ready Friday 6 November; assembly is
 * Wednesday 18 and Thursday 19 November; the opening is Monday 30 November,
 * eleven days after the assembly. Hostnames end in `.fjordform.example`.
 * All visible strings are Norwegian bokmål.
 */

export const tilbudQuestion =
  'Solhaug Hotell ber om tilbud på 40 stoler og 10 bord til åpningen 30. november. Skal jeg lage et tilbud?'

export const tilbudSuggestion = {
  label: 'Dette har skjedd',
  text: tilbudQuestion,
  source: 'Innboks · Solhaug Hotell AS',
  time: 'i dag 08:47',
}

export const tilbudTitle = 'Tilbud · Solhaug Hotell'

/** The grounding sources, in the order they are shown. */
export const tilbudSources = [
  {
    id: 'src_epost_tilbud',
    title: 'E-post · Solhaug Hotell · 9. okt 08:47',
    url: 'https://post.fjordform.example/innboks/solhaug-hotell/2026-10-09-tilbud',
    snippet: 'Vi åpner 30. november og trenger 40 stoler og 10 bord i eik. Kan dere levere og montere før det, og hva blir prisen?',
  },
  {
    id: 'src_pris_tilbud',
    title: 'Prisliste 2026.pdf · pkt. 2.3',
    url: 'https://dokumenter.fjordform.example/priser/prisliste-2026.pdf',
    snippet: 'Fjord Stol Eik (FS-210) 1 490 kr. Fjord Bord Eik 180 (FB-180) 4 200 kr. Volumrabatt 8 % på hele varebeløpet når det overstiger 75 000 kr eks. mva.',
  },
  {
    id: 'src_lager_tilbud',
    title: 'Lagerstatus · FS-210 og FB-180',
    url: 'https://lager.fjordform.example/status/fs-210-fb-180',
    snippet: 'FS-210: 28 stk på lager i hall B, 12 stk ferdig fra produksjon 6. november. FB-180: 10 stk på lager.',
  },
  {
    id: 'src_montering_tilbud',
    title: 'Monteringskalender · november',
    url: 'https://kalender.fjordform.example/montering/2026-11',
    snippet: 'Montørlag 2 er ledig 18.–19. november. Montering koster 3 600 kr per dag.',
  },
]

/** The three sub-tasks of the Do turn: what each one checks and what it finds. */
export const tilbudSubtasks = [
  { id: 'sub-lager', title: 'Sjekk lager og leveringstid · FS-210 og FB-180', result: '28 stoler og 10 bord på lager, 12 stoler ferdig 6. nov.' },
  { id: 'sub-pris', title: 'Hent pris og volumrabatt · prisliste pkt. 2.3', result: '101 600 kr på varene, 8 % volumrabatt gir 93 472 kr.' },
  { id: 'sub-montering', title: 'Finn ledig monteringsdag · november', result: 'Montørlag 2 er ledig 18.–19. nov, 11 dager før åpningen.' },
]

/** Assistant line, streamed in chunks. */
export const tilbudAnswerChunks = [
  'Ja, vi rekker det. 28 stoler og 10 bord er på lager, og de siste 12 stolene er ferdige 6. november. ',
  'Montering kan skje 18.–19. november, mer enn en uke før åpningen.',
]

/** The second paragraph, after the quote is in place. */
export const tilbudFollowChunks = [
  '\n\nTilbudet ligger i Resultat: 100 672 kr eks. mva med montering og 8 % volumrabatt på varene. ',
  'Svaret til Solhaug Hotell er klart som utkast.',
]

/** The quote's internal reference. Not a carrier's tracking number. */
export const tilbudReference = 'TB-7312'

export const tilbudArtifactTitle = 'Tilbud · Solhaug Hotell'

/**
 * The quote, revealed in three revisions: the lines and the discount, then assembly
 * and totals, then the reply. A list, not a table: the Resultat surface is narrow.
 */
export function tilbudArtifactMarkdown(stage: 1 | 2 | 3): string {
  const parts = [
    '- **40 stoler** Fjord Stol Eik (FS-210): 40 × 1 490 kr = 59 600 kr *· prisliste*',
    '- **10 bord** Fjord Bord Eik 180 (FB-180): 10 × 4 200 kr = 42 000 kr *· prisliste*',
    '- **Volumrabatt 8 %:** −8 128 kr *· prisliste pkt. 2.3*',
  ]
  if (stage >= 2) {
    parts.push(
      '- **Montering 18.–19. nov:** 2 dager × 3 600 kr = 7 200 kr *· monteringskalender*',
      '- **Sum eks. mva:** 100 672 kr',
      '- **Mva 25 %:** 25 168 kr',
      `- **Totalt:** 125 840 kr *· ${tilbudReference}*`,
      '- **Levering:** 28 stoler og 10 bord fra lager, 12 stoler ferdig 6. nov *· lagerstatus*',
    )
  }
  if (stage >= 3) {
    parts.push(
      '',
      '## Svar til kunden',
      '',
      'Til: Solhaug Hotell AS',
      '',
      'Emne: Tilbud · 40 stoler og 10 bord',
      '',
      'Hei! Vedlagt tilbud på 40 stoler og 10 bord i eik, levert og montert 18.–19. november, før åpningen 30. november. Totalt 125 840 kr inkl. mva, med 8 % volumrabatt på varene. Tilbudet gjelder til 23. oktober.',
      '',
      'Med vennlig hilsen',
      'Ida Berg, Fjordform AS',
    )
  }
  return parts.join('\n')
}

/** The gate under the quote: nothing is reserved or put in the inbox before a person says yes. */
export const tilbudApproval = {
  kind: 'Legg tilbud som utkast og reserver varer',
  detail: `Tilbud ${tilbudReference} legges som utkast til Solhaug Hotell, og 28 stoler og 10 bord reserveres. Ingenting sendes før du godkjenner.`,
}

/** The step the run's gate sits on. Its id is the run id, set by the scenario. */
export const tilbudQuoteStep = { title: 'Lager tilbud', detail: 'Tilbud og svar er klare for godkjenning.' }
