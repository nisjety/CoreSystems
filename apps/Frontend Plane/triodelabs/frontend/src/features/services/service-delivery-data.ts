import type { ServiceSeo } from "./service-seo";

export type ServiceDelivery = {
  deliverables: readonly { title: string; description: string }[];
  collaboration: string;
  quality: string;
  continuation: string;
  related: readonly {
    before: string;
    label: string;
    href: ServiceSeo["canonical"];
    after: string;
  }[];
};

export const serviceDeliveries: Record<
  ServiceSeo["canonical"],
  ServiceDelivery
> = {
  "/tjenester/web-losninger": {
    deliverables: [
      {
        title: "Struktur og innhold",
        description:
          "Målgrupper, sidetyper, navigasjon og prioriterte brukerreiser avklares før design. Vi blir enige om hvem som skriver, godkjenner og vedlikeholder innholdet.",
      },
      {
        title: "Design og utvikling",
        description:
          "Responsive sidemaler, gjenbrukbare komponenter og et publiseringsoppsett som passer redaktørene. WordPress eller en skreddersydd løsning velges ut fra innhold, integrasjoner og forvaltning.",
      },
      {
        title: "Lansering og overlevering",
        description:
          "En plan for domene, eksisterende URL-er, videresendinger og publisering. Dokumentasjon og redaktøropplæring kan inngå, slik at teamet vet hvordan nettstedet oppdateres.",
      },
    ],
    collaboration:
      "Vi kan levere nettstedet som et avgrenset prosjekt eller arbeide sammen med deres designere, redaktører og utviklere. Tilganger, beslutningsansvar og godkjenning av sidene avtales ved oppstart.",
    quality:
      "Før publisering gjennomgår vi avtalte brukerreiser, skjemaer, tastaturnavigasjon, mobilvisning og ytelse. Kjente feil og eventuelle restpunkter samles, slik at lanseringsbeslutningen bygger på en synlig sjekkliste.",
    continuation:
      "Etter overlevering kan vi avtale vedlikehold og videreutvikling. Ansvar for hosting, oppdateringer, sikkerhetskopier og håndtering av feil beskrives i avtalen, slik at det er tydelig hvem som følger opp nettstedet.",
    related: [
      {
        before: "Skal nettstedet gjøre dere lettere å finne, planlegger vi ",
        label: "søkemotoroptimalisering og innhold",
        href: "/tjenester/organisk-trafikk",
        after: " sammen med struktur og publisering.",
      },
      {
        before:
          "Er teknologivalget eller omfanget uavklart, kan dere starte med ",
        label: "digital rådgivning",
        href: "/tjenester/radgivning-og-konsulentbistand",
        after: " før selve utviklingen.",
      },
    ],
  },
  "/tjenester/nettbutikk": {
    deliverables: [
      {
        title: "En tydelig kjøpsreise",
        description:
          "Kategorier, søk, produktinformasjon, handlekurv og betaling utformes rundt hvordan kundene handler. Vi avklarer varianter, språk og eventuelle ulike kundetyper tidlig.",
      },
      {
        title: "Butikk og drift",
        description:
          "Oppsett av plattform, produktdata, betalings- og fraktvalg ut fra avtalte leverandører. Behov for ordre-, lager- og regnskapsflyt kartlegges før vi setter integrasjoner i arbeid.",
      },
      {
        title: "Trygg overgang",
        description:
          "Ved bytte av plattform lager vi en plan for produkter, adresser og videresendinger. Prøveimport og avstemming gjør det mulig å oppdage mangler før den nye butikken overtar.",
      },
    ],
    collaboration:
      "Vi samarbeider med dem som eier produktdata, kundeservice, logistikk og økonomi. Der dere har et internt utviklingsteam eller en plattformpartner, fordeler vi ansvar for butikk, data og integrasjoner før arbeidet starter.",
    quality:
      "Vi tester avtalte kjøpsforløp, blant annet betaling, avbrutte kjøp, ordrebekreftelse og relevante fraktvalg. Testordre avstemmes mot systemene som skal motta dem, og kjøpsreisen kontrolleres på mobil.",
    continuation:
      "Vi kan følge opp tekniske oppdateringer og prioritere forbedringer basert på faktiske kjøpsforløp. Løpende produktarbeid, support, tredjepartskostnader og videreutvikling avklares som egne ansvarsområder.",
    related: [
      {
        before:
          "Når ordre og lager skal henge sammen med resten av virksomheten, er ",
        label: "systemintegrasjoner",
        href: "/tjenester/integrasjoner",
        after: " en del av planleggingen.",
      },
      {
        before: "Kategorier og produktsider gir også grunnlag for ",
        label: "organisk synlighet",
        href: "/tjenester/organisk-trafikk",
        after: ", mens annonsering kan støtte avgrensede kampanjer.",
      },
    ],
  },
  "/tjenester/organisk-trafikk": {
    deliverables: [
      {
        title: "Kartlegging og prioritering",
        description:
          "Vi gjennomgår søkeintensjon, eksisterende sider og tekniske hindringer. Dere får en prioritert liste over tiltak, med forklaring av hvilke kundeoppgaver hver side skal svare på.",
      },
      {
        title: "Teknikk og innhold",
        description:
          "Arbeidet kan omfatte indeksering, interne lenker, metadata, omdirigeringer og innholdsstruktur. Fagpersonene deres bidrar med erfaringene og dokumentasjonen som gjør innholdet særpreget.",
      },
      {
        title: "Måling av relevant trafikk",
        description:
          "Vi avtaler hvilke søk, sider og henvendelser som er viktige. Search Console og egnet analyse brukes til å følge utviklingen og skille synlighet fra besøk som faktisk er relevante.",
      },
    ],
    collaboration:
      "SEO-arbeidet fordeles mellom markedsansvarlig, fagpersoner og utviklingsteam. Vi kan levere analyser og innholdsbrief, implementere avtalte tiltak eller støtte teamet deres i gjennomføringen.",
    quality:
      "Vi kontrollerer at viktige sider kan besøkes og indekseres, at innholdet svarer på søket, og at lenker og strukturerte data stemmer med siden. Trafikk og henvendelser vurderes over tid; rangeringer eller bestemte resultater kan ikke garanteres.",
    continuation:
      "Etter første forbedringsrunde prioriterer vi ut fra data og endringer i tilbudet deres. Relevante spørsmål i Google Trends kan informere innholdsplanen, sammen med egne kundesamtaler og søkedata, uten å la kortvarige trender styre hele nettstedet.",
    related: [
      {
        before:
          "Når tekniske begrensninger hindrer forbedringene, kobler vi SEO-arbeidet til ",
        label: "webutvikling",
        href: "/tjenester/web-losninger",
        after: " og en konkret plan for endringene.",
      },
      {
        before: "Trenger dere synlighet mens organisk innhold utvikles, kan ",
        label: "Google Ads og Meta Ads",
        href: "/tjenester/google-og-meta-ads",
        after: " utfylle arbeidet med avtalte budsjetter og målepunkter.",
      },
    ],
  },
  "/tjenester/integrasjoner": {
    deliverables: [
      {
        title: "System- og datakart",
        description:
          "Vi beskriver hvilke systemer som er kilde til hvilke data, hva som skal flyttes, og når. Tilgang til API-er, testmiljøer og leverandørvilkår avklares før gjennomføring.",
      },
      {
        title: "En avgrenset dataflyt",
        description:
          "Integrasjonen bygges for avtalte hendelser og datafelter, med validering og håndtering av feil. Vi avklarer hvordan duplikater, endringer og midlertidige avbrudd skal behandles.",
      },
      {
        title: "Dokumentasjon og ansvar",
        description:
          "Dere får en beskrivelse av koblingen og hvordan feil kan oppdages og følges opp. Eier av tilganger, datakvalitet og hver del av driften navngis i leveranseavtalen.",
      },
    ],
    collaboration:
      "Vi arbeider med systemeierne, deres IT-team og eventuelle eksterne leverandører. Sammen avklarer vi hvilke opplysninger integrasjonen kan behandle og hvem som kan godkjenne endringer i dataflyten.",
    quality:
      "Testene omfatter riktig overføring, ugyldige data og avbrudd hos et tilkoblet system. Vi avstemmer utvalgte poster mellom kilde og mottaker og avtaler kriterier for å sette integrasjonen i produksjon.",
    continuation:
      "API-er og systemer endrer seg. Vedlikehold kan derfor omfatte oppfølging av leverandørendringer, feilvarsler og nye behov. Omfang, responstid og kostnader avtales separat fra den første leveransen.",
    related: [
      {
        before: "For en ",
        label: "nettbutikk",
        href: "/tjenester/nettbutikk",
        after:
          " kan første avgrensning være ordre til regnskap og lagerstatus tilbake til butikken.",
      },
      {
        before:
          "Skal løsningen også tolke informasjon eller foreslå et svar, vurderer vi ",
        label: "KI og automatisering",
        href: "/tjenester/ki-og-automatisering",
        after: " som et eget steg med tydelige kontrollpunkter.",
      },
    ],
  },
  "/tjenester/google-og-meta-ads": {
    deliverables: [
      {
        title: "Mål og kampanjestruktur",
        description:
          "Vi avklarer tilbudet, målgruppen, geografien og hva en relevant henvendelse eller handel er. Kanal og kampanjeformat velges ut fra dette, med budsjett som dere godkjenner.",
      },
      {
        title: "Budskap og landingssider",
        description:
          "Annonser, søkeord eller målgrupper og landingsside skal henge sammen. Vi avklarer hvem som leverer bilder og tekst, og hvem som godkjenner budskapet før kampanjen starter.",
      },
      {
        title: "Sporing og oppfølging",
        description:
          "Relevante konverteringer defineres, sporingen kontrolleres og en rapporteringsrytme avtales. Arbeidshonorar, annonsebudsjett og eventuell produksjon holdes tydelig adskilt.",
      },
    ],
    collaboration:
      "Vi samarbeider med markeds- og salgsansvarlige om kvaliteten på henvendelsene. Kontotilganger, godkjenningsrutiner og eierskap til annonser og måleoppsett avklares ved oppstart.",
    quality:
      "Før aktivering kontrollerer vi lenker, budskap, målretting, budsjett og registrering av avtalte konverteringer. Vi gjennomgår samtykkeoppsettet sammen med dere og vurderer resultatene opp mot faktiske salg eller henvendelser der data finnes.",
    continuation:
      "Løpende arbeid kan omfatte søketermer, budjusteringer og testing av nye budskap. Vi forklarer hva som er endret og hvilke observasjoner som ligger bak. Resultater påvirkes også av tilbud, konkurranse og landingssiden.",
    related: [
      {
        before:
          "En kampanje trenger en tydelig vei videre. Vi kan forbedre landingssiden som del av ",
        label: "webdesign og webutvikling",
        href: "/tjenester/web-losninger",
        after: ".",
      },
      {
        before: "Kunnskap fra kampanjene kan samtidig brukes i ",
        label: "SEO og innholdsarbeid",
        href: "/tjenester/organisk-trafikk",
        after: " når dere vil bygge synlighet over tid.",
      },
    ],
  },
  "/tjenester/ki-og-automatisering": {
    deliverables: [
      {
        title: "Én oppgave med tydelig nytte",
        description:
          "Vi beskriver hvem som skal bruke løsningen, hvilke data den trenger, og hva et godt resultat er. Sammen velger vi et avgrenset bruksområde og sammenligner KI med enklere automatisering.",
      },
      {
        title: "En pilot med kontrollpunkter",
        description:
          "Piloten prøves på avtalte spørsmål eller oppgaver. Kilder, tilganger og menneskelig godkjenning inngår i oppsettet. Handlinger i andre systemer avgrenses særskilt.",
      },
      {
        title: "Et grunnlag for videre beslutning",
        description:
          "Vi dokumenterer testresultater, kjente begrensninger og hvilke feil som må håndteres. Modell-, drifts- og leverandørkostnader synliggjøres før dere beslutter å utvide løsningen.",
      },
    ],
    collaboration:
      "Fagpersonene som kjenner arbeidsoppgaven, deltar sammen med IT- eller systemansvarlig. De hjelper med testeksempler og vurderer svarene, mens vi bygger og dokumenterer arbeidsflyten.",
    quality:
      "Vi tester korrekte svar, usikre spørsmål og situasjoner der kildene ikke gir et svar. Løsningen skal ha en avtalt vei til menneskelig vurdering. Godkjenning og datatilgang følger oppgaven, ikke modellens egen vurdering av hva den bør få gjøre.",
    continuation:
      "Etter en pilot kan vi avtale oppfølging av svar, kilder og kostnader. Endringer i kunnskapsgrunnlag eller modell testes før de tas i bruk, og videre utvidelse bygger på erfaringene fra den avgrensede arbeidsflyten.",
    related: [
      {
        before:
          "Når informasjon må hentes fra eller sendes til eksisterende verktøy, bygger vi på ",
        label: "systemintegrasjoner",
        href: "/tjenester/integrasjoner",
        after: " med avtalte tilganger og feilrutiner.",
      },
      {
        before: "Er det uklart hvor KI kan gi verdi, starter vi med ",
        label: "rådgivning og behovsavklaring",
        href: "/tjenester/radgivning-og-konsulentbistand",
        after: " før vi velger produkt eller teknologi.",
      },
    ],
  },
  "/tjenester/radgivning-og-konsulentbistand": {
    deliverables: [
      {
        title: "Avklaring og veikart",
        description:
          "Vi kartlegger behov, eksisterende løsninger og avhengigheter. Dere får anbefalinger og prioriterte neste steg, med forutsetninger og åpne spørsmål synlige i beslutningsgrunnlaget.",
      },
      {
        title: "Konsulent inn i teamet",
        description:
          "Vi kan bidra med avtalt kompetanse innen webutvikling, integrasjoner og praktisk KI. Oppgaver, kapasitet, samarbeidsverktøy og hvem som godkjenner leveransene avklares før oppstart.",
      },
      {
        title: "Et avgrenset prosjekt",
        description:
          "For en tydelig definert oppgave kan vi ta ansvar for gjennomføringen fra avklaring til overlevering. Milepæler og akseptansekriterier gjør det mulig å følge fremdrift og justere omfanget underveis.",
      },
    ],
    collaboration:
      "Vi tilpasser oss deres prosjektleder, produktansvarlige og tekniske team. Ansvar for prioritering, arkitektur, kodegjennomgang og beslutninger avklares sammen, slik at konsulentarbeidet inngår i den eksisterende organisasjonen.",
    quality:
      "Anbefalinger begrunnes med behov og alternativer. I utviklingsoppdrag avtaler vi kodegjennomgang, relevante tester, dokumentasjon og demonstrasjon av ferdig arbeid. Uavklarte forhold løftes før de blir skjulte avhengigheter.",
    continuation:
      "Oppdraget avsluttes med avtalt dokumentasjon og overføring av kunnskap. Dere kan overta forvaltningen selv eller avtale videre bistand; tilgang til kode, systemer og beslutningshistorikk inngår i planleggingen av overleveringen.",
    related: [
      {
        before: "Når retningen er avklart, kan vi gjennomføre ",
        label: "webutvikling",
        href: "/tjenester/web-losninger",
        after: " sammen med teamet deres eller som et eget prosjekt.",
      },
      {
        before: "Har dere mange systemavhengigheter, kan ",
        label: "kartlegging av integrasjoner",
        href: "/tjenester/integrasjoner",
        after: " være første arbeidsområde.",
      },
      {
        before: "For nye arbeidsmåter kan vi avgrense en pilot innen ",
        label: "KI og automatisering",
        href: "/tjenester/ki-og-automatisering",
        after: " med tydelige mål for evalueringen.",
      },
    ],
  },
};
