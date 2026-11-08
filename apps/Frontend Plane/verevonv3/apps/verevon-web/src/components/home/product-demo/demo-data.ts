export const scenarios = [
  {
    label: "Finn grunnlaget",
    title: "Én levering. Hele bildet.",
    artifact: "Leveransegrunnlag · Orion",
    kind: "Beslutningsgrunnlag",
    prompt:
      "Hva har vi lovet kunden i prosjekt Orion, og hva setter leveringen i fare?",
    answer:
      "Vi har lovet en ferdig kundeportal 14. oktober. Eksporten venter på et godkjent skjema, og brukertesten kan ikke starte før eksporten er klar. Jeg har samlet kildene og markert konflikten i Resultat.",
    sources: [
      {
        name: "Kundeavtale · punkt 4.2",
        type: "Dokument",
        excerpt:
          "Kundeportalen leveres 14. oktober. Leveransen omfatter eksport av kunderegisteret og en godkjent brukertest.",
        detail: "Kundeavtale.pdf · side 4",
      },
      {
        name: "Prosjektmøte · 7. oktober",
        type: "Møtenotat",
        excerpt:
          "Skjemaet må godkjennes av produktansvarlig før teknisk ansvarlig starter eksporten. Brukertesten krever ferdig eksport.",
        detail: "Orion / møtereferater",
      },
      {
        name: "Teknisk status · i dag",
        type: "Statusrapport",
        excerpt:
          "Overordnet status: grønn. Eksport: blokkert, mangler godkjent skjema. Beregnet arbeid: to arbeidsdager, deretter brukertest.",
        detail: "Orion / teknisk status",
      },
    ],
  },
  {
    label: "Lag en plan",
    title: "Fra avklaring til neste steg.",
    artifact: "Utkast · leveranseplan",
    kind: "Prosjektplan",
    prompt:
      "Lag en realistisk plan for å sikre leveransen. Vis ansvar, frister og hva som må avklares først.",
    answer:
      "Planutkastet er klart for gjennomgang i Resultat. Første beslutning er å godkjenne skjemaet. Eksport og brukertest følger etter. Ingen oppgaver eller kalenderavtaler er opprettet.",
    sources: [
      {
        name: "Leveranserisiko · Orion",
        type: "Analyse",
        excerpt:
          "Avtalt levering er 14. oktober. Eksporten er blokkert av en uavklart skjemagodkjenning. Leveransedato må bekreftes etter brukertesten.",
        detail: "Fra eksemplet «Finn grunnlaget»",
      },
      {
        name: "Prosjektmøte · 7. oktober",
        type: "Møtenotat",
        excerpt:
          "Produktansvarlig eier skjemagodkjenningen. Teknisk ansvarlig eier eksporten. Kundeteamet gjennomfører brukertesten, og prosjektleder avklarer planen med kunden.",
        detail: "Orion / møtereferater",
      },
      {
        name: "Teamets tilgjengelighet",
        type: "Kapasitet",
        excerpt:
          "Teknisk ansvarlig trenger to arbeidsdager etter skjemagodkjenning. Kundeteamet kan teste når eksporten er verifisert.",
        detail: "Orion / ressursplan",
      },
    ],
  },
  {
    label: "Undersøk blokkeringen",
    title: "Se hva som stopper arbeidet.",
    artifact: "Eksport · årsak og test",
    kind: "Teknisk undersøkelse",
    prompt:
      "Undersøk hvorfor eksporten feiler. Vis hva du fant, og foreslå et trygt neste steg.",
    answer:
      "Eksporten stopper fordi customer_id mangler i mappingen. Loggen og skjemaet peker på samme årsak. Se den foreslåtte endringen i Resultat, og prøv den simulerte testen før du godkjenner.",
    sources: [
      {
        name: "Eksportlogg · kjøring 184",
        type: "Logg",
        excerpt:
          "09:41:03 · Validering feilet: required field customer_id is missing. Eksporten ble stoppet før noen data ble skrevet.",
        detail: "Testmiljø / export-184.log",
      },
      {
        name: "Godkjent skjema · v3",
        type: "Skjema",
        excerpt:
          "customer_id: string, påkrevd. name: string, påkrevd. email: string, valgfritt.",
        detail: "Testmiljø / customer-schema.json",
      },
      {
        name: "Mapping · kunderegister",
        type: "Kode",
        excerpt:
          "Mappingen inneholder name og email. Feltet id i kunderegisteret er ikke koblet til customer_id i eksportskjemaet.",
        detail: "Testmiljø / customer-mapping.ts",
      },
    ],
  },
  {
    label: "Følg opp",
    title: "En avklaring som varer.",
    artifact: "Utkast · kundeoppdatering",
    kind: "Oppfølging",
    prompt:
      "Lag en kundeoppdatering og et forslag til oppfølging til leveransen er avklart. Ikke send noe ennå.",
    answer:
      "Kundeoppdateringen er klar som utkast. Jeg foreslår å sjekke prosjektstatus hver morgen og varsle internt ved ny blokkering eller endret frist. Du kan redigere utkastet og tidspunktet i Resultat.",
    sources: [
      {
        name: "Planutkast · Orion",
        type: "Prosjektplan",
        excerpt:
          "Eksporten må verifiseres før brukertest. Prosjektleder bekrefter leveransedato med kunden etter godkjent brukertest.",
        detail: "Fra eksemplet «Lag en plan»",
      },
      {
        name: "Eksporttest · bestått",
        type: "Testresultat",
        excerpt:
          "Simulert test: feltmappingen er validert. Ingen produksjonsdata er endret. Brukertest gjenstår.",
        detail: "Forberedt eksempeldata, ikke en live test",
      },
      {
        name: "Kommunikasjonsrammer",
        type: "Retningslinje",
        excerpt:
          "Oppfølging går til prosjektleder internt. Kundeoppdateringer skal gjennomgås og godkjennes før ekstern utsending.",
        detail: "Velmora Demo / rutiner",
      },
    ],
  },
] as const;

export const planSteps = [
  {
    title: "Godkjenn eksportskjema",
    owner: "Produktansvarlig",
    deadline: "I dag",
    dependency: "Før eksport",
    criterion: "Alle påkrevde felt er avklart.",
  },
  {
    title: "Klargjør og test eksport",
    owner: "Teknisk ansvarlig",
    deadline: "+2 arbeidsdager",
    dependency: "Etter steg 1",
    criterion: "Eksporten passerer valideringen.",
  },
  {
    title: "Gjennomfør brukertest",
    owner: "Kundeteamet",
    deadline: "Etter eksport",
    dependency: "Etter steg 2",
    criterion: "Kundeteamet godkjenner resultatet.",
  },
  {
    title: "Bekreft planen med kunden",
    owner: "Prosjektleder",
    deadline: "Etter brukertest",
    dependency: "Etter steg 3",
    criterion: "Ny leveransedato er avklart med kunden.",
  },
] as const;

export const initialEmail =
  "Hei!\n\nVi har identifisert årsaken til eksportfeilen og verifisert løsningen i test. Neste steg er brukertesten. Vi bekrefter leveransedato så snart den er godkjent.\n\nVi holder dere oppdatert.\nProsjektteamet i Velmora";
