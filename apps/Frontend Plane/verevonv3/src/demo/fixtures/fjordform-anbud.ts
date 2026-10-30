/**
 * Case 3 · Dokumentasjon til Nordhavn kommune: fixtures for the interactive demo (src/demo/live).
 *
 * A tender case, unlike the two films and the other two cases. A municipality asks for
 * environmental documents with its tender. Verevon reads the requirement, checks each
 * document's validity and catches the one that has run out, then prepares a reply and a
 * task for purchasing. Nothing is saved or created before a person says yes.
 *
 * Dates: today is Friday 9 October 2026; the deadline is Friday 16 October at 12:00;
 * the FSC certificate is valid to 31 October (22 days, three weeks, from today); the
 * table EPD is valid to 14 March 2029; the chair EPD ran out on 1 September 2026.
 * Hostnames end in `.fjordform.example`. All visible strings are Norwegian bokmål.
 */

export const anbudQuestion =
  'Nordhavn kommune ber om FSC- og EPD-dokumentasjon til anbudet innen fredag 16. oktober. Har vi gyldige bevis?'

export const anbudSuggestion = {
  label: 'Dette har skjedd',
  text: anbudQuestion,
  source: 'Anbud · Nordhavn kommune',
  time: 'i dag 11:05',
}

export const anbudTitle = 'Anbud · dokumentasjon til Nordhavn kommune'

/** The grounding sources, in the order they are shown. */
export const anbudSources = [
  {
    id: 'src_anbud_krav',
    title: 'Anbudsbrev · Nordhavn kommune · krav K4.2',
    url: 'https://post.fjordform.example/anbud/nordhavn-kommune/anbudsbrev',
    snippet: 'Tilbyder skal legge ved gyldig FSC-sertifikat og EPD for hver produktgruppe. Frist fredag 16. oktober kl. 12.',
  },
  {
    id: 'src_anbud_fsc',
    title: 'FSC-sertifikat · Fjordform AS',
    url: 'https://dokumenter.fjordform.example/miljo/fsc-sertifikat.pdf',
    snippet: 'Gyldig til 31. oktober 2026. Dekker eik og ask.',
  },
  {
    id: 'src_anbud_epd_bord',
    title: 'EPD · Fjord Bord Eik 180 (FB-180)',
    url: 'https://dokumenter.fjordform.example/miljo/epd-fb-180.pdf',
    snippet: 'Gyldig til 14. mars 2029.',
  },
  {
    id: 'src_anbud_epd_stol',
    title: 'EPD · Fjord Stol Ask (FS-310)',
    url: 'https://dokumenter.fjordform.example/miljo/epd-fs-310.pdf',
    snippet: 'Utløpt 1. september 2026. Fornyelse er ikke registrert.',
  },
]

/** Assistant line, streamed in chunks. */
export const anbudAnswerChunks = [
  'Nesten. FSC-sertifikatet er gyldig til 31. oktober, altså gjennom fristen, og EPD for eikebordene er gyldig til 2029. ',
  'EPD for ask-stolene utløp 1. september og kan ikke legges ved som den er.',
]

/** The second paragraph, after the draft is in place. */
export const anbudFollowChunks = [
  '\n\nSvar til kommunen og en oppgave til innkjøp ligger i Resultat. ',
  'Jeg foreslår å si fra om stolene i svaret og få fornyelsen på plass før fristen. FSC-sertifikatet utløper om tre uker.',
]

export const anbudArtifactTitle = 'Utkast · svar til Nordhavn kommune'

/**
 * The document check and the reply, revealed in three revisions: each document's
 * validity, then the actions, then the reply itself. A list, not a table.
 */
export function anbudArtifactMarkdown(stage: 1 | 2 | 3): string {
  const parts = [
    '- **FSC-sertifikat:** gyldig til 31. okt 2026, dekker eik og ask *· sertifikatet*',
    '- **EPD · bord (FB-180):** gyldig til 14. mars 2029 *· EPD*',
    '- **EPD · stol (FS-310):** utløpt 1. sept 2026, kan ikke legges ved *· EPD*',
  ]
  if (stage >= 2) {
    parts.push(
      '- **Oppgave til innkjøp:** forny EPD for FS-310 før fredag 16. okt',
      '- **Påminnelse:** FSC-sertifikatet utløper 31. okt, forny i god tid',
    )
  }
  if (stage >= 3) {
    parts.push(
      '',
      '## Svar til kommunen',
      '',
      'Til: Nordhavn kommune, innkjøp',
      '',
      'Emne: Miljødokumentasjon · anbud',
      '',
      'Hei! Vedlagt FSC-sertifikat og EPD for bordene. EPD for ask-stolen er under fornyelse, og vi bekrefter tidspunkt for ny EPD mandag.',
      '',
      'Med vennlig hilsen',
      'Ida Berg, Fjordform AS',
    )
  }
  return parts.join('\n')
}

/** The gate under the reply: nothing is saved or created before a person says yes. */
export const anbudApproval = {
  kind: 'Lagre svar og opprett oppgave',
  detail: 'Svaret legges som utkast i innboksen, og oppgaven går til innkjøp. Ingenting sendes før du godkjenner.',
}

/** The steps shown under Arbeid, in order. The third is the run's gate: its id is the run id, set by the scenario. */
export const anbudRequirementStep = { id: 'steg-krav', title: 'Leser kravene i anbudet', detail: 'Krav K4.2: FSC og EPD per produktgruppe, frist fredag 16. oktober.' }
export const anbudValidityStep = { id: 'steg-gyldighet', title: 'Sjekker gyldighet', detail: 'FSC og bord-EPD er gyldige. Stol-EPD utløp 1. september.' }
export const anbudReplyStep = { title: 'Skriver svar og oppgave', detail: 'Svar og oppgave er klare for godkjenning.' }
