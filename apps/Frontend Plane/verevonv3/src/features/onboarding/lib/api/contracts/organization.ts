export type BrregEnhet = {
  organisasjonsnummer: string
  navn: string
  antallAnsatte?: number
  /** Registered website (e.g. `www.aquatiq.com`) — used to pre-fill the website
   * step so the crawl preview can run against the selected org. */
  hjemmeside?: string
  forretningsadresse?: {
    adresse?: string[]
    postnummer?: string
    poststed?: string
    kommune?: string
    land?: string
  }
  organisasjonsform?: { kode?: string; beskrivelse?: string }
  naeringskode1?: { kode?: string; beskrivelse?: string }
  konkurs?: boolean
  underAvvikling?: boolean
}
