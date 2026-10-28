/**
 * Case 2 · Reklamasjon fra Havnebrygga Kafé: fixtures for the interactive demo (src/demo/live).
 *
 * A service case, unlike the two films and unlike the other two cases. A café reports
 * a crack in a table top. Verevon reads the warranty terms, the delivery receipt and
 * the production log, finds that the table belongs to a series with the same fault at
 * two other customers, and prepares a replacement and a reply. Nothing is ordered or
 * sent before a person says yes.
 *
 * Dates: today is Friday 9 October 2026; the table was delivered and signed for on
 * Tuesday 22 September, 17 days ago; the fitter replaces the top on Thursday
 * 15 October. In series 2609, 3 of 40 tables have the glue-joint fault: the café and
 * two other customers. Hostnames end in `.fjordform.example`. All visible strings are
 * Norwegian bokmål.
 */

export const reklamasjonQuestion =
  'Havnebrygga Kafé melder en sprekk i bordplaten på et bord levert 22. september. Skal jeg sjekke om garantien dekker det?'

export const reklamasjonSuggestion = {
  label: 'Dette har skjedd',
  text: reklamasjonQuestion,
  source: 'Innboks · Havnebrygga Kafé AS',
  time: 'i dag 09:35',
}

export const reklamasjonTitle = 'Reklamasjon · Havnebrygga Kafé'

/** The grounding sources, in the order they are shown. */
export const reklamasjonSources = [
  {
    id: 'src_epost_reklamasjon',
    title: 'E-post · Havnebrygga Kafé · 9. okt 09:35',
    url: 'https://post.fjordform.example/innboks/havnebrygga-kafe/2026-10-09-reklamasjon',
    snippet: 'Bordplaten på bord 6 har fått en sprekk langs limfugen. Bordet ble levert 22. september. Kan dere hjelpe?',
  },
  {
    id: 'src_levering_reklamasjon',
    title: 'Ordre 5120 · leveringskvittering',
    url: 'https://dokumenter.fjordform.example/ordre/5120',
    snippet: 'Fjord Bord Eik 180, serie 2609: levert og kvittert 22. september uten merknad.',
  },
  {
    id: 'src_garanti_reklamasjon',
    title: 'Garantivilkår 2026.pdf · pkt. 3',
    url: 'https://dokumenter.fjordform.example/vilkar/garanti-2026.pdf',
    snippet: 'Fem års garanti mot konstruksjons- og materialfeil. Fukt-, slitasje- og påkjørselsskader dekkes ikke.',
  },
  {
    id: 'src_produksjon_reklamasjon',
    title: 'Produksjonslogg · serie 2609',
    url: 'https://produksjon.fjordform.example/logg/serie-2609',
    snippet: 'Avvik på limfuge registrert for 3 av 40 bord i serien. To andre kunder har meldt sprekk.',
  },
]

/** Assistant line, streamed in chunks. */
export const reklamasjonAnswerChunks = [
  'Ja, dette dekkes av garantien. Sprekken sitter i limfugen, og bordet ble kvittert uten merknad for 17 dager siden. ',
  'Det hører til serie 2609, der to andre kunder har meldt det samme.',
]

/** The second paragraph, after the draft is in place. */
export const reklamasjonFollowChunks = [
  '\n\nUtkast til svar og bestilling av ny bordplate ligger i Resultat. ',
  'Jeg foreslår også at produksjon sjekker resten av serie 2609.',
]

export const reklamasjonArtifactTitle = 'Utkast · svar til Havnebrygga Kafé'

/**
 * The claim and the reply, revealed in three revisions: coverage, the delivery and the
 * series; then the remedy; then the reply itself. A list, not a table.
 */
export function reklamasjonArtifactMarkdown(stage: 1 | 2 | 3): string {
  const parts = [
    '- **Dekning:** konstruksjonsfeil, fem års garanti *· garantivilkår pkt. 3*',
    '- **Levering:** kvittert 22. sept uten merknad *· ordre 5120*',
    '- **Parti:** 3 av 40 bord i serie 2609 har limfugeavvik *· produksjonslogg*',
  ]
  if (stage >= 2) {
    parts.push('- **Tiltak:** ny bordplate til bord 6, byttes av montør torsdag 15. okt, 08–12')
  }
  if (stage >= 3) {
    parts.push(
      '',
      '## Svar til kunden',
      '',
      'Til: Havnebrygga Kafé AS',
      '',
      'Emne: Sprekk i bordplate · ordre 5120',
      '',
      'Hei, og beklager. Dette dekkes av garantien. Vi sender ny bordplate, og en montør bytter den torsdag 15. oktober mellom 08 og 12. Du trenger ikke gjøre noe før det.',
      '',
      'Med vennlig hilsen',
      'Ida Berg, Fjordform AS',
    )
  }
  return parts.join('\n')
}

/** The gate under the reply: nothing is ordered or put in the inbox before a person says yes. */
export const reklamasjonApproval = {
  kind: 'Bestill bordplate og lagre svar',
  detail: 'Ny bordplate bestilles, og svaret legges som utkast i innboksen. Ingenting sendes før du godkjenner.',
}

/** The steps shown under Arbeid, in order. The fourth is the run's gate: its id is the run id, set by the scenario. */
export const reklamasjonWarrantyStep = { id: 'steg-garanti', title: 'Sjekker garantivilkår', detail: 'Pkt. 3: konstruksjonsfeil dekkes i fem år.' }
export const reklamasjonDeliveryStep = { id: 'steg-levering', title: 'Sjekker leveringen', detail: 'Ordre 5120 ble kvittert 22. september uten merknad.' }
export const reklamasjonSeriesStep = { id: 'steg-serie', title: 'Søker i produksjonsloggen', detail: 'Serie 2609: 3 av 40 bord med limfugeavvik.' }
export const reklamasjonReplyStep = { title: 'Skriver svar og bestilling', detail: 'Svar og ny bordplate er klare for godkjenning.' }
