import type { RoutePath } from "../../lib/i18n/routes";

export type LegalDocument = {
  readonly path: RoutePath;
  /** Shown above the text when the page is a translation. */
  readonly notice?: string;
  readonly title: string;
  readonly description: string;
  readonly introduction: string;
  readonly updatedAt: string;
  readonly updatedLabel: string;
  readonly sections: readonly {
    readonly title: string;
    readonly paragraphs: readonly string[];
    readonly items?: readonly string[];
    readonly links?: readonly { readonly label: string; readonly href: string }[];
  }[];
};

// Source: the four published legal pages on www.triodelab.no, read 8 October 2026.
// Sales/general terms retain their original text and date. Privacy/cookies
// describe conditional form delivery, user-initiated chat and the consent-gated
// GA4/Clarity rollout. Owner review is required before publishing the revised notices.
const publication = {
  updatedAt: "2026-09-24",
  updatedLabel: "24. september 2026",
} as const;

export const privacyDocument: LegalDocument = {
  ...publication,
  updatedAt: "2026-10-23",
  updatedLabel: "23. oktober 2026",
  path: "/personvern",
  title: "Personvernerklæring",
  description:
    "Les hvordan TriodeLab DA behandler personopplysninger, hvilke rettigheter dere har, og hvordan dere kan kontakte oss om personvern.",
  introduction:
    "Denne personvernerklæringen forklarer hvilke personopplysninger TriodeLab DA samler inn når dere bruker triodelab.no eller er i kontakt med oss, hva vi bruker dem til, og hvilke rettigheter dere har.",
  sections: [
    {
      title: "Behandlingsansvarlig",
      paragraphs: [
        "TriodeLab DA, Sigurd Hoels vei 114, 0655 Oslo, er behandlingsansvarlig for personopplysningene som beskrives i denne erklæringen. Dere når oss på post@triodelab.no eller +47 988 46 460.",
      ],
    },
    {
      title: "Hvilke opplysninger vi behandler",
      paragraphs: [
        "Vi behandler personopplysninger dere selv gir oss, blant annet gjennom kontaktskjemaet på nettsiden, e-post eller telefon:",
      ],
      items: [
        "Navn og hvilken bedrift dere representerer",
        "E-postadresse og telefonnummer",
        "Innhold i henvendelsen, herunder ønsket prosjekttype og budsjett dere oppgir",
      ],
    },
    {
      title: "Hva vi bruker opplysningene til",
      paragraphs: [
        "Opplysningene brukes til å svare på henvendelsen deres, vurdere og følge opp mulige prosjekter, og til å administrere eventuelle kundeforhold og fakturering. Det rettslige grunnlaget er personvernforordningen (GDPR) artikkel 6 nr. 1 bokstav b (oppfylle eller inngå avtale) og bokstav f (berettiget interesse i å besvare henvendelser).",
        "Vi bruker ikke opplysningene til automatiserte avgjørelser eller profilering.",
      ],
    },
    {
      title: "Kontaktskjemaet på nettsiden",
      paragraphs: [
        "Når innsending på nettsiden er aktivert, sendes navn, e-postadresse, eventuell bedrift, valgt tjeneste og melding til post@triodelab.no via e-postleverandøren Resend. Hvis denne funksjonen ikke er aktivert, viser nettsiden en e-postlenke, slik at dere selv kan sende henvendelsen.",
        "For å begrense misbruk av skjemaet bruker vi en pseudonymisert nettverksidentifikator som slettes etter 10 minutter. Vi lagrer også en teknisk leveringsidentifikator i inntil 48 timer for å unngå dobbelttelling av samme innsending. Denne lagringen håndteres av Upstash og inneholder ikke meldingstekst, navn, e-postadresse eller rå IP-adresse.",
        "Vi teller mottatte henvendelser samlet per dag, valgt tjeneste og hvor på nettstedet skjemaet ble åpnet. Tellingene inneholder ikke kontaktopplysninger eller meldingstekst og bruker ikke informasjonskapsler i nettleseren.",
      ],
    },
    {
      title: "Hvor lenge vi lagrer opplysningene",
      paragraphs: [
        "Henvendelser som ikke fører til et kundeforhold, slettes normalt innen 12 måneder. Opplysninger knyttet til et kundeforhold lagres så lenge det er nødvendig for å oppfylle avtalen, og deretter i tråd med lovpålagte lagringsplikter, blant annet bokføringsloven for regnskapsbilag (5 år).",
      ],
    },
    {
      title: "Hvem vi deler opplysninger med",
      paragraphs: [
        "Nettsiden driftes hos Vercel, som kan behandle tekniske data (som IP-adresse) på våre vegne for å levere siden. Vi selger ikke personopplysninger videre, og deler dem kun med andre der det er nødvendig for å levere en tjeneste til dere, eller der vi er pålagt det ved lov.",
        "Når innsending via kontaktskjemaet er aktivert, bruker vi Resend til å levere henvendelsen på e-post og Upstash til misbruksvern og samlet telling av mottatte henvendelser, som beskrevet over.",
        "Når dere har valgt Statistikk, bruker vi Microsoft Clarity på utvalgte offentlige sider for å forstå hvordan nettstedet brukes og forbedre det. Microsoft mottar tekniske data og hendelser om sidevisning, klikk, rulling, musebevegelser, layout og ytelse for å lage varmekart og øktopptak. Vi lenker til Microsofts personvernerklæring for mer informasjon om deres behandling.",
        "Chatten fra Agenci lastes først når dere velger å åpne den. Opplysninger dere skriver i chatten, sendes til Agenci for å behandle samtalen. Unngå å sende sensitive personopplysninger i chatten.",
      ],
      links: [
        { label: "Microsofts personvernerklæring", href: "https://privacy.microsoft.com/privacystatement" },
      ],
    },
    {
      title: "Informasjonskapsler (cookies)",
      paragraphs: [
        "Cookiebot viser og lagrer samtykkevalgene deres. Når besøksmåling er aktivert, lastes Google Analytics 4 og Microsoft Clarity først etter at dere velger Statistikk. Clarity får samtykkesignalet analytics_Storage=granted og ad_Storage=denied. Avvisning og manglende samtykke laster ikke måleskriptene. Dere kan endre valget eller trekke samtykket tilbake via samtykkeinnstillingene på Cookie-policy-siden.",
      ],
    },
    {
      title: "Valgfri besøksmåling med Google Analytics 4 og Microsoft Clarity",
      paragraphs: [
        "Formålet er å forstå bruk av våre offentlige sider og forbedre nettstedet. Etter samtykke kan Google motta opplysninger om sidevisninger, økter, engasjement og rulling, nettleser, enhet og tekniske forbindelsesopplysninger. Forbedret måling er på og kan også registrere interaksjoner med offentlig innhold, som innebygde videoer. Informasjonskapsler med pseudonyme identifikatorer brukes for å skille brukere og økter.",
        "Implementasjonen begrenser målingen til utvalgte offentlige sider. Den starter ikke på kontakt-, konto- eller juridiske sider, nettadresser med søkeparametere eller fragmenter, og stopper videre måling i dokumentet ved skjema-/chatinteraksjon og lenker som kan eksponere personlige opplysninger. Ikke skriv personlige eller sensitive opplysninger i nettadresser. Vi sender ikke kontaktskjemaets eller chattens meldingstekst som egendefinerte Analytics-hendelser.",
        "Microsoft Clarity bruker statistikkdata til varmekart og øktopptak. Det kan registrere musebevegelser, klikk, rulling, vindusstørrelse, sidevisning og tekniske hendelser for gjengivelse og ytelse. Nettstedets innhold er eksplisitt maskert med Claritys data-clarity-mask-markering; maskert innhold sendes ikke til Clarity. Inndatafelt er også maskert av Clarity. Kontakt-, juridiske og ukjente sider er utelatt, og målingen stopper ved skjema-/chatinteraksjon og følsomme navigasjoner. Microsofts lagring er oppgitt til 30 dager for avspillingsdata og 9 måneder for klikkdata og varmekart.",
        "Denne integrasjonen gir ikke samtykke til reklamelagring, reklamepersonalisering eller Google signals. Clarity får reklamelagring avslått. Nettleserens DNT- eller GPC-signal stopper sporingen.",
        "De kontrollerte GA4-innstillingene 22. oktober 2026 har 2 måneder oppbevaring for hendelsesdata og 14 måneder for brukerdata. Brukerperioden tilbakestilles ved ny aktivitet. Disse innstillingene begrenser ikke nødvendigvis oppbevaring av aggregerte standardrapporter. Informasjonskapslenes varighet er beskrevet separat i Cookie-policy.",
      ],
      links: [
        { label: "Microsofts personvernerklæring", href: "https://privacy.microsoft.com/privacystatement" },
      ],
    },
    {
      title: "Valgfri førsteparts ytelsesmåling",
      paragraphs: [
        "Når denne funksjonen er aktivert og dere velger Statistikk, måler vi LCP (lasting), INP (respons på interaksjon) og CLS (layoutendringer) på utvalgte offentlige sider. Nettleseren sender måleverdien, en grov vindusbreddegruppe, en godkjent sidesti, en tilfeldig dokumentidentifikator og en oppdateringsteller til vårt eget nettsted. Identifikatoren gjelder bare dokumentet, lagres ikke i en informasjonskapsel og brukes ikke til å følge personer mellom besøk.",
        "Råmålinger lagres i en serverbasert Redis-kompatibel datalagring og i vår markedsføringsplattform i maksimalt 7 UTC-kalenderdager, inkludert mottaksdagen. Vi bruker dem til samlede ytelsesrapporter, ikke til profilering. Måledataene inneholder ikke søkeparametere, fragmenter, henvisningsadresse, DOM-innhold, bruker-ID eller rå IP-adresse. Hosting og lagringsleverandører kan behandle tekniske forbindelsesopplysninger for å levere tjenesten. Målingen starter ikke uten samtykke, respekterer DNT/GPC og stopper ved følsomme interaksjoner eller navigasjon. Tilbaketrekking stopper nye målinger; allerede mottatte målinger slettes ved utløp.",
      ],
    },
    {
      title: "Deres rettigheter",
      paragraphs: [
        "Dere har etter personvernforordningen rett til innsyn i, retting eller sletting av egne personopplysninger, samt rett til å begrense eller protestere mot behandlingen og til dataportabilitet der det er relevant. Ønsker dere å bruke noen av disse rettighetene, kontakt oss på post@triodelab.no.",
        "Dere kan også klage til Datatilsynet dersom dere mener vi behandler personopplysninger i strid med regelverket.",
      ],
    },
  ],
};

export const cookiesDocument: LegalDocument = {
  ...publication,
  updatedAt: "2026-10-23",
  updatedLabel: "23. oktober 2026",
  path: "/cookies",
  title: "Cookie-policy",
  description:
    "Informasjon om informasjonskapsler på triodelab.no, hva de brukes til, og hvordan dere kan styre eller slette dem i nettleseren.",
  introduction:
    "Denne siden forklarer bruken av informasjonskapsler (cookies) og andre lagringsmekanismer på triodelab.no. Vi oppdaterer siden dersom bruken endres.",
  sections: [
    {
      title: "Hva er en informasjonskapsel?",
      paragraphs: [
        "En informasjonskapsel er en liten tekstfil som lagres i nettleseren når dere besøker en nettside. Den kan blant annet brukes til å huske innstillinger, måle trafikk eller vise relevante annonser.",
      ],
    },
    {
      title: "Kapsler vi bruker i dag",
      paragraphs: [
        "Cookiebot viser samtykkebanneret og lagrer samtykkevalget deres i en nødvendig informasjonskapsel. Next.js og hostingplattformen Vercel leverer nettsiden. Misbruksvern og samlet telling av mottatte kontakthenvendelser skjer på serveren og bruker ikke informasjonskapsler i nettleseren.",
      ],
    },
    {
      title: "Valgfrie statistikkapsler",
      paragraphs: [
        "Når besøksmåling er aktivert og dere velger Statistikk, lastes Google Analytics 4 og Microsoft Clarity for utvalgte offentlige sider. _ga brukes til å skille brukere, og _ga_<container-id> brukes til å opprettholde øktinformasjon. Googles nominelle standardvarighet er 2 år; nettleseren kan begrense den, og nye besøk kan fornye utløpstiden. Clarity kan bruke _clck for å huske en pseudonym bruker-ID og _clsk for å knytte sidevisninger sammen i et øktopptak. Clarity kan også sette Microsofts egne informasjonskapsler for teknisk drift. Dette er ikke det samme som leverandørenes oppbevaring av rapport- og opptaksdata.",
        "Analysefunksjonene starter ikke uten Statistikk-samtykke. Dere kan trekke samtykket tilbake ved å åpne samtykkeinnstillingene nedenfor og slå av Statistikk. Integrasjonen stopper målingen, forsøker å slette sine førsteparts Google- og Clarity-kapsler og laster dokumentet på nytt. Dette sletter ikke automatisk data som allerede er mottatt av Google eller Microsoft. Dere kan også slette kapsler i nettleseren.",
        "Vi aktiverer ikke reklamefunksjoner gjennom denne integrasjonen. Clarity bruker maskerte nettstedsdata til varmekart og øktopptak; avspillingsdata oppbevares i 30 dager, og klikkdata og varmekart i 9 måneder, ifølge Microsoft. Se personvernerklæringen vår og Microsofts personvernerklæring for mer informasjon.",
      ],
      links: [
        { label: "Microsofts personvernerklæring", href: "https://privacy.microsoft.com/privacystatement" },
      ],
    },
    {
      title: "Valgfri førsteparts ytelsesmåling uten kapsler",
      paragraphs: [
        "Etter Statistikk-samtykke kan vår egen ytelsesmåling sende LCP, INP og CLS til nettstedets egen server. Denne funksjonen bruker ikke informasjonskapsler eller vedvarende nettleserlagring. En tilfeldig dokumentidentifikator brukes bare til å unngå dobbelttelling av oppdaterte målinger. Se personvernerklæringen for datainnhold, samtykkevalg og maksimal lagringstid på 7 UTC-kalenderdager.",
      ],
    },
    {
      title: "Valgfri chat",
      paragraphs: [
        "Chatten leveres av Agenci og lastes først når dere velger å åpne den. Da kobles nettleseren til Agenci, og det dere skriver i chatten, sendes dit for å behandle samtalen. Dere kan bruke resten av nettsiden og kontakte oss på e-post uten å åpne chatten.",
        "Chatleverandørens bruk av informasjonskapsler og lokal lagring er ikke omfattet av beskrivelsen av besøksmåling over. Vi bekrefter derfor ikke at chatten er uten informasjonskapsler eller lokal lagring.",
      ],
    },
    {
      title: "Endringer i bruk av kapsler",
      paragraphs: [
        "Endrer vi formål, leverandører eller hvilke valgfrie kapsler som brukes, oppdaterer vi denne siden og samtykkeinformasjonen før den nye bruken aktiveres.",
      ],
    },
    {
      title: "Styre eller slette kapsler",
      paragraphs: [
        "Dere kan når som helst slette eller blokkere informasjonskapsler i nettleserens innstillinger. Dette kan påvirke hvordan enkelte nettsider fungerer, men vil ikke hindre dere i å bruke triodelab.no.",
      ],
    },
    {
      title: "Spørsmål",
      paragraphs: [
        "Har dere spørsmål om informasjonskapsler eller personvern generelt, se personvernerklæringen vår eller kontakt oss på post@triodelab.no.",
      ],
    },
  ],
};

export const termsDocument: LegalDocument = {
  ...publication,
  path: "/vilkar",
  title: "Vilkår og betingelser",
  description:
    "Generelle vilkår for bruk av triodelab.no og avtaler med TriodeLab DA. Les om tjenester, ansvar, rettigheter og kontaktinformasjon.",
  introduction:
    "Disse vilkårene gjelder for bruk av triodelab.no og for avtaler dere inngår med TriodeLab DA. Ved å bruke nettsiden eller bestille en tjeneste hos oss, godtar dere vilkårene under.",
  sections: [
    {
      title: "Om selskapet",
      paragraphs: [
        "TriodeLab DA («TriodeLab», «vi», «oss») er et norsk konsulentselskap med kontoradresse Sigurd Hoels vei 114, 0655 Oslo. Vi leverer webløsninger, nettbutikker, integrasjoner og digital markedsføring til bedriftskunder.",
        "Har dere spørsmål om disse vilkårene, kan dere nå oss på post@triodelab.no eller +47 988 46 460.",
      ],
    },
    {
      title: "Bruk av nettsiden",
      paragraphs: [
        "Innholdet på triodelab.no eies av TriodeLab eller våre lisensgivere, og er beskyttet av åndsverkloven. Dere kan lese og dele innhold fra siden, men ikke kopiere, gjenbruke eller distribuere det kommersielt uten skriftlig samtykke fra oss.",
        "Nettsiden skal ikke brukes til ulovlige formål, til å spre skadelig kode, eller på en måte som forstyrrer driften av siden for andre besøkende.",
      ],
    },
    {
      title: "Avtaler om tjenester",
      paragraphs: [
        "Konkrete leveranser, priser og tidsfrister avtales særskilt med hver kunde, normalt i et eget tilbud eller en signert avtale. Der en slik avtale finnes, går den foran disse generelle vilkårene ved motstrid.",
        "For kjøp av tjenester som ikke er regulert i en egen avtale, gjelder salgsvilkårene våre, se «Salgsvilkår».",
      ],
    },
    {
      title: "Ansvar",
      paragraphs: [
        "Vi gjør vårt beste for at informasjonen på nettsiden er riktig og oppdatert, men kan ikke garantere at den til enhver tid er fullstendig eller feilfri.",
        "TriodeLab er ikke ansvarlig for indirekte tap som følge av bruk av nettsiden, med mindre annet følger av ufravikelig lovgivning eller en egen avtale mellom partene.",
      ],
    },
    {
      title: "Lenker til tredjeparter",
      paragraphs: [
        "Nettsiden kan inneholde lenker til nettsteder vi ikke kontrollerer, blant annet prosjekter og plattformer vi har bygget for kunder. Vi er ikke ansvarlige for innholdet på disse sidene.",
      ],
    },
    {
      title: "Endringer",
      paragraphs: [
        "Vi kan oppdatere disse vilkårene ved behov, for eksempel når tjenestene eller lovkravene endrer seg. Datoen øverst på siden viser når vilkårene sist ble oppdatert. Ved vesentlige endringer i en løpende kundeavtale, varsler vi dere direkte.",
      ],
    },
    {
      title: "Lovvalg og tvister",
      paragraphs: [
        "Disse vilkårene reguleres av norsk rett. Eventuelle tvister søkes først løst i minnelighet. Fører ikke det frem, hører tvisten hjemme ved TriodeLabs alminnelige verneting i Norge.",
      ],
    },
  ],
};

export const salesTermsDocument: LegalDocument = {
  ...publication,
  path: "/salgsvilkar",
  title: "Salgsvilkår",
  description:
    "Salgsvilkår for konsulent- og utviklingstjenester fra TriodeLab DA: bestilling, betaling, leveranser, endringer, reklamasjon og rettigheter.",
  introduction:
    "Disse salgsvilkårene gjelder når dere kjøper konsulent- og utviklingstjenester fra TriodeLab DA. Vilkårene gjelder sammen med et eventuelt tilbud, prosjektavtale eller ordrebekreftelse dere har mottatt fra oss.",
  sections: [
    {
      title: "Hvem vilkårene gjelder for",
      paragraphs: [
        "Vi leverer tjenester til næringsdrivende (B2B). Forbrukerkjøpsloven og angrerettloven, som gjelder kjøp av varer og tjenester til privatpersoner, kommer derfor normalt ikke til anvendelse på våre leveranser.",
      ],
    },
    {
      title: "Tilbud og bestilling",
      paragraphs: [
        "En avtale kommer i stand når dere skriftlig aksepterer et tilbud fra oss, for eksempel ved signering, e-post eller bekreftelse i et prosjektverktøy. Tilbud er normalt gyldige i 30 dager, med mindre annet er oppgitt.",
        "Omfang, leveranser, tidsplan og pris for det enkelte prosjekt fremgår av tilbudet eller avtalen, og kan avvike fra det som er beskrevet generelt på nettsiden.",
      ],
    },
    {
      title: "Pris og betaling",
      paragraphs: [
        "Priser oppgis i norske kroner (NOK) og er, med mindre annet er avtalt, eksklusive merverdiavgift. Betalingsbetingelser fremgår av faktura, normalt 14 dagers forfall.",
        "For større prosjekter kan vi avtale delbetaling, for eksempel ved oppstart, underveis og ved levering. Ved forsinket betaling påløper forsinkelsesrente etter forsinkelsesrenteloven.",
      ],
    },
    {
      title: "Leveranse og medvirkning",
      paragraphs: [
        "Vi leverer tjenestene med normal faglig aktsomhet og innenfor de tidsrammer som er avtalt. Fremdriften forutsetter at dere bidrar med nødvendig informasjon, tilbakemeldinger og beslutninger underveis.",
        "Forsinkelser som skyldes manglende medvirkning fra kunden, eller forhold utenfor vår kontroll, gir ikke grunnlag for erstatning eller prisavslag.",
      ],
    },
    {
      title: "Endringer i prosjektet",
      paragraphs: [
        "Ønsker dere endringer i omfang underveis, avtaler vi konsekvensene for pris og tidsplan før arbeidet igangsettes. Mindre justeringer innenfor opprinnelig omfang håndteres fortløpende.",
      ],
    },
    {
      title: "Reklamasjon",
      paragraphs: [
        "Oppdager dere feil eller mangler ved leveransen, ber vi dere melde fra så snart som mulig, og senest innen rimelig tid etter at feilen ble eller burde vært oppdaget. Vi retter berettigede feil uten ugrunnet opphold.",
      ],
    },
    {
      title: "Ansvarsbegrensning",
      paragraphs: [
        "Vårt samlede erstatningsansvar for et prosjekt er begrenset til det beløpet kunden har betalt for den aktuelle leveransen, med mindre annet er avtalt skriftlig eller tapet skyldes grov uaktsomhet eller forsett.",
        "Vi er ikke ansvarlige for indirekte tap, som tapt fortjeneste eller tap av data, utover det som følger av ufravikelig lovgivning.",
      ],
    },
    {
      title: "Opphavsrett",
      paragraphs: [
        "Med mindre annet er avtalt, får kunden full bruksrett til det som er utviklet spesifikt for prosjektet ved fullført betaling. TriodeLab beholder rettighetene til egne verktøy, rammeverk og generisk kode som gjenbrukes på tvers av prosjekter.",
      ],
    },
    {
      title: "Lovvalg og tvister",
      paragraphs: [
        "Disse salgsvilkårene reguleres av norsk rett. Tvister søkes løst i minnelighet, og hører ellers hjemme ved TriodeLabs alminnelige verneting i Norge.",
      ],
    },
  ],
};

export const legalDocuments = [
  privacyDocument,
  cookiesDocument,
  termsDocument,
  salesTermsDocument,
] as const;
