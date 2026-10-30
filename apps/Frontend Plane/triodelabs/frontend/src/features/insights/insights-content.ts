import type { RoutePath } from "@/lib/i18n/routes";

export type InsightSection = {
  body: readonly string[];
  title: string;
};

export type InsightLink = {
  description: string;
  href: string;
  label: string;
};

export type InsightFaq = {
  answer: string;
  question: string;
};

export type InsightTrend = {
  additionalSources?: readonly InsightLink[];
  body: string;
  sourceHref: string;
  sourceKind: "Kilde" | "Videre lesing";
  sourceLabel: string;
  title: string;
};

export type Insight = {
  canonical: RoutePath;
  checklist: readonly string[];
  contentType: "Guide" | "Fagartikkel";
  description: string;
  faqs: readonly InsightFaq[];
  heading: string;
  hero: {
    alt: string;
    image: string;
  };
  intro: readonly string[];
  label: string;
  lastUpdated: string;
  lastUpdatedLabel: string;
  publishedAt: string;
  readingTime: string;
  relatedLinks: readonly InsightLink[];
  relatedService: string;
  sections: readonly InsightSection[];
  slug: string;
  summary: string;
  title: string;
  trend: InsightTrend;
};

export const insights: readonly Insight[] = [
  {
    slug: "systemintegrasjon-for-nettbutikk",
    canonical: "/innsikt/systemintegrasjon-for-nettbutikk",
    contentType: "Guide",
    label: "Nettbutikk og integrasjoner",
    title:
      "Systemintegrasjon for nettbutikk: slik unngår dere manuelle feil og skjulte driftsbrudd",
    heading: "Systemintegrasjon for nettbutikk.",
    description:
      "En praktisk guide til å planlegge integrasjon mellom nettbutikk, lager, økonomi og andre systemer – med tydelige eiere, kontrollpunkter og drift i sentrum.",
    summary:
      "En integrasjon skal ikke bare flytte data. Den skal gjøre et viktig arbeidsløp mer pålitelig, også når noe går galt.",
    publishedAt: "2026-10-08",
    lastUpdated: "2026-10-08",
    lastUpdatedLabel: "8. oktober 2026",
    readingTime: "8 min lesing",
    relatedService: "/tjenester/integrasjoner",
    hero: {
      // Photo: Kampus Production / Pexels (https://www.pexels.com/photo/couple-packing-products-together-7289732/).
      image: "/images/insights/ecommerce-order-workflow.jpg",
      alt: "To personer pakker varer ved et arbeidsbord med bærbar datamaskin og esker",
    },
    intro: [
      "Når en nettbutikk vokser, er det fort gjort at ordre, lagerstatus, priser og kundeinformasjon begynner å leve i flere systemer. Det kan fungere en stund med eksportfiler og manuelle kontroller. Etter hvert blir arbeidet sårbart: en ordre blir registrert to ganger, et produkt blir solgt uten lager, eller en retur når aldri frem til økonomisystemet.",
      "En god systemintegrasjon gjør ikke virksomheten avhengig av magi i bakgrunnen. Den gjør en konkret dataflyt tydeligere, mer kontrollert og enklere å følge opp. Derfor bør arbeidet starte med forretningsprosessen og ansvaret rundt den – ikke med hvilket API som er mest tilgjengelig.",
    ],
    trend: {
      title: "Aktuelt nå: når KI-agenter skal bruke integrasjonen",
      body: "KI-agenter som bruker verktøy på tvers av systemer er et aktuelt tema, men bør ikke få vide skrive-rettigheter som standard. Start med lesetilgang, ett tydelig formål og menneskelig godkjenning før en løsning endrer ordre, pris, kundedata eller økonomi. Det er et kontrollspørsmål – ikke et argument for å bruke agenter overalt.",
      sourceHref:
        "https://cloud.google.com/resources/content/ai-agent-trends-2026",
      sourceKind: "Videre lesing",
      sourceLabel: "Google Cloud: AI agent trends 2026",
    },
    sections: [
      {
        title: "Avklar når en integrasjon faktisk er verdt det",
        body: [
          "En integrasjon er mest verdifull når den fjerner gjentatt manuelt arbeid, reduserer feil i et kritisk løp eller gir ansatte og kunder riktig informasjon raskere. Ikke alle kopier av data trenger sanntid. For noen oppgaver er en kontrollert synkronisering én gang i døgnet helt riktig. For andre, som lagerstatus ved salg, kan forsinkelse få direkte konsekvenser.",
          "Start med ett problem som er lett å beskrive. For eksempel: «En betalt ordre må opprette korrekt ordregrunnlag i økonomisystemet uten at teamet taster den inn på nytt.» Et slikt utgangspunkt gir dere noe konkret å prioritere, teste og måle – i stedet for et prosjekt som bare heter «vi må integrere systemene våre».",
        ],
      },
      {
        title: "Tegn dataflyten før dere bygger",
        body: [
          "Kartlegg hvilke systemer som er involvert, hvilken informasjon de eier, og når data skal flyttes. Nettbutikken kan være stedet kunden legger inn en bestilling, mens lager- eller ERP-systemet eier tilgjengelig beholdning. Et CRM kan eie samtykker og kundehistorikk. Når to systemer begge antas å være fasiten for samme felt, oppstår ofte vanskelige konflikter senere.",
          "Beskriv også hendelsen som starter flyten: betaling godkjent, ordre kansellert, retur mottatt eller produkt oppdatert. Da kan dere skille tydelig mellom en ny ordre og en endring i en eksisterende ordre. Det er et viktig grunnlag for både teknisk løsning og drift.",
        ],
      },
      {
        title: "Begynn med én flyt med tydelig nytte",
        body: [
          "Det er fristende å koble sammen alle systemer på én gang. I praksis er det tryggere å velge den mest verdifulle flyten først, og bruke den til å etablere felles prinsipper. For en nettbutikk kan det være ordre fra nettbutikk til ordrebehandling, eller lagerstatus fra lager til nettbutikk.",
          "En avgrenset første flyt gjør det mulig å ta gode valg om format, tilgangsstyring, logging og feilretting uten at prosjektet blir låst av alle særtilfellene fra dag én. Når dere ser at løsningen fungerer i hverdagen, er det lettere å bygge videre med mindre risiko.",
        ],
      },
      {
        title: "Planlegg for det som skjer når noe feiler",
        body: [
          "En integrasjon må tåle at et system er utilgjengelig, at samme hendelse sendes to ganger, eller at informasjon mangler. En ordre bør ikke bli dobbeltført fordi en forespørsel må forsøkes på nytt. Samtidig skal en feil ikke forsvinne uten at noen ser den. Det krever at dere bestemmer hvilke data som identifiserer en hendelse, hva som kan prøves igjen, og når et menneske skal varsles.",
          "Lag en enkel arbeidsregel for avvik: Hva skjer med en ordre som ikke kommer frem? Hvem får beskjed? Hvor ser de hva som feilet? Og hvordan vet de at den er håndtert? Slike spørsmål er like viktige som selve koblingen mellom to API-er.",
        ],
      },
      {
        title: "Test med realistiske variasjoner – og avstem resultatet",
        body: [
          "En demonstrasjon med én perfekt ordre sier lite om hvordan løsningen fungerer i drift. Test derfor endringer, kanselleringer, rabatter, retur, manglende produktdata og forsinkede svar. Målet er ikke å fjerne alle unntak, men å forstå hvordan dere oppdager og håndterer dem på en forutsigbar måte.",
          "Avstemming er et nyttig kontrollpunkt. Med jevne mellomrom bør en ansvarlig kunne sammenligne antall og verdier i kildesystemet med målsystemet. Når tallene ikke stemmer, skal teamet ha nok sporbarhet til å finne ut om årsaken er en feil, en forsinkelse eller en normal forretningshendelse.",
        ],
      },
      {
        title: "Gjør eierskap og videre drift til en del av leveransen",
        body: [
          "En integrasjon er ikke ferdig når den første dataflyten virker. Systemer oppdateres, nye kampanjer og betalingsmetoder innføres, og produktdata endrer seg. Dokumentasjon bør derfor forklare hva løsningen gjør, hvilke systemer den avhenger av, hvilke tilganger den bruker og hvordan avvik håndteres.",
          "Avklar også hvem som kan prioritere endringer, hvem som følger med på feil og hvordan kunnskap overleveres ved bytte av leverandør eller interne roller. Det gjør investeringen mer robust og gir virksomheten bedre kontroll over både data og videreutvikling.",
        ],
      },
    ],
    checklist: [
      "Vi har valgt én konkret arbeidsflyt som skal bli bedre.",
      "Vi vet hvilket system som eier hver viktig dataverdi.",
      "Vi har beskrevet hendelsene som oppretter, endrer og avslutter flyten.",
      "Vi har bestemt hvordan duplikater, forsinkelser og feil skal håndteres.",
      "Vi tester med kanselleringer, endringer, retur og ufullstendige data.",
      "Vi har en plan for varsling, avstemming og manuell oppfølging.",
      "Vi har dokumentert ansvar, tilganger og hvordan løsningen kan utvikles videre.",
    ],
    faqs: [
      {
        question:
          "Hvilket system skal være fasit for pris, lager, kunde og ordre?",
        answer:
          "Det avhenger av arbeidsflyten, men hvert felt bør ha ett tydelig kildesystem. Nettbutikken er ofte stedet kunden handler, mens ERP, lager- eller CRM-systemet kan eie andre verdier. Dokumenter valget før integrasjonen bygges.",
      },
      {
        question:
          "Trenger vi sanntid, eller er planlagt synkronisering tryggere?",
        answer:
          "Velg ut fra konsekvensen av forsinkelse. Lagerstatus ved salg kan kreve hyppige oppdateringer, mens andre verdier kan synkroniseres kontrollert på faste tidspunkt. Sanntid er ikke et mål i seg selv.",
      },
      {
        question:
          "Hva skjer når samme ordre sendes to ganger, eller et system er nede?",
        answer:
          "Løsningen bør gjenkjenne duplikater, forsøke på nytt på en kontrollert måte og vise avvik til en ansvarlig person. Det må være tydelig hvor feilen kan undersøkes og når den er håndtert.",
      },
      {
        question:
          "Når trenger en integrasjon menneskelig godkjenning før den oppdaterer data?",
        answer:
          "Bruk godkjenning når en endring kan få stor økonomisk, juridisk eller kundeopplevd konsekvens. Det gjelder særlig avvikende ordre, prisendringer, kundedata og transaksjoner der regelen ikke er helt entydig.",
      },
      {
        question:
          "Hvem følger opp feil, avstemming og endringer etter lansering?",
        answer:
          "Dette bør være avtalt før lansering. Utpek en forretningseier for arbeidsflyten og en tydelig ansvarlig for teknisk oppfølging, med fast rutine for varsling, avstemming og endringer.",
      },
    ],
    relatedLinks: [
      {
        href: "/tjenester/nettbutikk",
        label: "Nettbutikk",
        description:
          "Se hvordan vi bygger kjøpsopplevelser med drift og videreutvikling i sentrum.",
      },
      {
        href: "/tjenester/integrasjoner",
        label: "Systemintegrasjoner",
        description:
          "Når systemer skal henge sammen, med tydelige dataflyter og ansvar.",
      },
      {
        href: "/tjenester/radgivning-og-konsulentbistand",
        label: "Rådgivning og konsulentbistand",
        description:
          "For dere som vil avklare retning, risiko og neste steg før bygging.",
      },
    ],
  },
  {
    slug: "ki-pilot-for-bedrifter",
    canonical: "/innsikt/ki-pilot-for-bedrifter",
    contentType: "Guide",
    label: "KI og arbeidsflyt",
    title:
      "KI-pilot for bedrifter: 7 avklaringer før dere automatiserer en arbeidsflyt",
    heading: "KI-pilot for bedrifter.",
    description:
      "En praktisk guide til å avgrense en KI-pilot med riktig oppgave, tydelige kilder, menneskelig kontroll og en plan for videre drift.",
    summary:
      "En KI-pilot skal redusere usikkerhet rundt én viktig oppgave – ikke bli en demonstrasjon uten eierskap eller mål.",
    publishedAt: "2026-10-08",
    lastUpdated: "2026-10-08",
    lastUpdatedLabel: "8. oktober 2026",
    readingTime: "7 min lesing",
    relatedService: "/tjenester/ki-og-automatisering",
    hero: {
      // Photo: Vitaly Gariev / Unsplash (https://unsplash.com/photos/team-collaborating-around-a-whiteboard-during-a-meeting-rgKX4o2xSqI).
      image: "/images/insights/ki-pilot-workshop.jpg",
      alt: "Et team diskuterer strategi rundt en tavle under en arbeidsøkt",
    },
    intro: [
      "Mange virksomheter ser at generativ KI kan hjelpe ansatte med å finne informasjon, sortere henvendelser eller lage et første utkast. Det gir sjelden mening å starte med en bred løsning for hele organisasjonen. En pilot er mer nyttig når den prøver ut én avgrenset oppgave med tydelig nytte og kjente rammer.",
      "Målet er ikke å bevise at KI er imponerende. Målet er å finne ut om en konkret arbeidsflyt blir tryggere, raskere eller enklere å håndtere – og om virksomheten har det som trengs av kilder, ansvar og kontroll for å ta løsningen videre.",
    ],
    trend: {
      title: "Aktuelt nå: fra KI-assistent til avgrenset arbeidsflyt",
      body: "Bruken av KI i norsk næringsliv har økt raskt. SSB oppgir at nesten fem av ti foretak med minst ti ansatte brukte én eller flere KI-teknologier i 2026, opp fra tre av ti året før. Når flere går fra utforsking til bruk, blir det viktigere å velge én rolle, én datagrense og én godkjenningsflyt før løsningen får større ansvar.",
      sourceHref:
        "https://www.ssb.no/teknologi-og-innovasjon/informasjons-og-kommunikasjonsteknologi-ikt/statistikk/bruk-av-ikt-i-naeringslivet/artikler/bruken-av-ki-i-naeringslivet-akselererer",
      sourceKind: "Kilde",
      sourceLabel: "SSB: Bruken av KI i næringslivet akselererer",
    },
    sections: [
      {
        title: "Velg en oppgave som kan vurderes",
        body: [
          "En god KI-pilot begynner med et gjentakende arbeid som allerede tar tid eller skaper friksjon. Det kan være å finne frem i intern kunnskap, sortere innkommende spørsmål eller lage et forslag som en fagperson kan redigere. Oppgaven bør være konkret nok til at dere kan beskrive dagens arbeidsmåte og se om piloten faktisk forbedrer den.",
          "Unngå mål som «vi skal ta i bruk KI». Beskriv heller hva som skal bli enklere for hvem. For eksempel kan en pilot undersøke om et kundeteam finner relevant, godkjent informasjon raskere når de får forslag med tydelige kilder. Da blir både nytten og risikoen mulig å diskutere på en saklig måte.",
        ],
      },
      {
        title: "Skill mellom KI, automatisering og integrasjon",
        body: [
          "KI er nyttig når oppgaven handler om å tolke, oppsummere, foreslå eller finne mønster i språk og kunnskap. Vanlig automatisering passer ofte bedre når en oppgave følger faste regler: Når en ordre er betalt, opprett en sak. Når et skjema mangler et felt, be om mer informasjon.",
          "Integrasjoner handler om å flytte eller synkronisere data mellom systemer. I mange gode løsninger spiller disse tre sammen, men de løser ulike problemer. Å skille dem tidlig gjør det lettere å velge riktig teknologi, forklare løsningen til brukerne og unngå at KI får ansvar for noe en enkel regel burde håndtert.",
        ],
      },
      {
        title: "Avklar kilder, tilganger og datagrenser",
        body: [
          "Svarene en KI-assistent gir blir ikke mer pålitelige enn kildene og instruksjonene den får. Før dere kobler på dokumenter, kundedata eller interne systemer, bør dere vite hva som faktisk er oppdatert, hvem som eier informasjonen og hvem som skal ha tilgang til den.",
          "Definer en bevisst grense for hva piloten kan se og gjøre. Noen oppgaver krever bare et avgrenset kunnskapsbibliotek. Andre kan ha behov for å hente opplysninger fra et system, men ikke skrive tilbake. Slike valg bør være synlige i pilotens avgrensning, ikke gjemt i tekniske detaljer.",
        ],
      },
      {
        title: "Bestem når mennesker skal ta over",
        body: [
          "En KI-løsning bør ikke gjøre en viktig vurdering uten at virksomheten har bestemt hva som er akseptabelt. Se derfor på hvor et forslag er nok, hvor en fagperson må godkjenne, og hvilke svar eller handlinger som alltid skal stoppes og vurderes manuelt.",
          "Det er også verdt å gjøre ansvaret tydelig. Hvem følger opp at kunnskapsgrunnlaget er riktig? Hvem kan endre instruksjoner og tilgang? Hvem tar beslutningen hvis piloten gir et usikkert eller feilaktig svar? Klare roller er en forutsetning for at en pilot kan bli et nyttig arbeidsverktøy, ikke en kortvarig demonstrasjon.",
        ],
      },
      {
        title: "Test med ekte spørsmål, ikke bare en god demo",
        body: [
          "Før en pilot settes i vanlig bruk, trenger dere et utvalg av realistiske spørsmål, oppgaver og vanskelige tilfeller. Testen bør inneholde både spørsmål løsningen forventes å håndtere, og spørsmål den skal avvise eller sende videre. Da ser dere ikke bare om svaret høres godt ut, men om det er forankret i rett informasjon og blir håndtert riktig når den er usikker.",
          "Avtal på forhånd hva dere vil vurdere: kvalitet på svar, sporbarhet til kilder, tid brukt av medarbeidere og hvor ofte menneskelig kontroll er nødvendig. Dette gir et bedre grunnlag for en go/no-go-beslutning enn en generell følelse av at teknologien virker lovende.",
        ],
      },
      {
        title: "Planlegg drift før dere skalerer",
        body: [
          "En pilot som virker i en avgrenset situasjon er et godt startpunkt, men ikke en ferdig virksomhetsløsning. Før dere utvider, bør dere vite hvordan kilder oppdateres, hvordan dere oppdager feil, hvordan tilganger endres og hvem som kan prioritere forbedringer. Det gjør det enklere å beholde kontroll når flere team, systemer eller oppgaver kommer til.",
          "Dokumenter det dere har lært: avgrensningen, kvaliteten på testene, åpne risikoer og anbefalt neste steg. Det gjør at virksomheten kan velge å bygge videre, endre kurs eller avslutte piloten på et opplyst grunnlag.",
        ],
      },
    ],
    checklist: [
      "Vi har valgt én konkret oppgave som piloten skal undersøke.",
      "Vi har beskrevet hva som er et nyttig resultat – og hva løsningen ikke skal gjøre.",
      "Vi kjenner kildene, eierne og tilgangsgrensene for informasjonen som brukes.",
      "Vi har skilt KI-oppgaven fra regler og integrasjoner som bør håndteres på andre måter.",
      "Vi har bestemt hvilke svar eller handlinger som krever menneskelig godkjenning.",
      "Vi tester med reelle spørsmål, feiltilfeller og oppgaver piloten skal sende videre.",
      "Vi har en plan for ansvar, endringer og videre drift før vi skalerer.",
    ],
    faqs: [
      {
        question: "Hvilken oppgave er egnet som første KI-pilot?",
        answer:
          "Velg et gjentakende arbeid med tydelig friksjon og et avgrenset resultat – for eksempel å finne frem i godkjent intern kunnskap eller lage et forslag som en fagperson vurderer. Dere skal kunne sammenligne dagens arbeidsmåte med piloten.",
      },
      {
        question: "Kan en KI-pilot bruke kunde- eller medarbeiderdata?",
        answer:
          "Bare når dere kjenner datagrunnlaget, tilgangene og formålet. Start med minst mulig data og tydelige grenser for hva piloten kan lese, foreslå og eventuelt skrive tilbake. Involver riktig fagansvarlig når oppgaven berører personopplysninger eller andre følsomme forhold.",
      },
      {
        question:
          "Når bør en KI-agent bare foreslå noe, og når kan den utføre en handling?",
        answer:
          "La den foreslå når utfallet krever skjønn eller kan få vesentlig konsekvens for kunde, økonomi eller omdømme. En avgrenset handling kan automatiseres først når regelen, datakilden og kontrollpunktet er tydelige og testet.",
      },
      {
        question:
          "Hvordan tester vi at svarene er korrekte og forankret i godkjente kilder?",
        answer:
          "Test med reelle spørsmål, forventede svar og vanskelige tilfeller. Logg testspørsmål, hvilken kilde som burde brukes, faktisk resultat, feiltype og hvem som vurderte det. Da kan dere rette kilder eller instruksjoner på et konkret grunnlag.",
      },
      {
        question:
          "Hva må være på plass før en pilot kan skaleres til flere team eller systemer?",
        answer:
          "Dere bør kunne vise hvilken nytte piloten ga, hvordan kvaliteten følges opp, hvilke tilganger den trenger og hvem som kan endre den. Først da er det et godt tidspunkt å vurdere flere brukere, systemer eller oppgaver.",
      },
    ],
    relatedLinks: [
      {
        href: "/tjenester/ki-og-automatisering",
        label: "KI og automatisering",
        description:
          "Slik kan KI-assistenter og automatiserte arbeidsflyter brukes rundt konkrete oppgaver.",
      },
      {
        href: "/tjenester/integrasjoner",
        label: "Systemintegrasjoner",
        description:
          "Når arbeidsflyten trenger tydelige dataflyter mellom eksisterende systemer.",
      },
      {
        href: "/tjenester/radgivning-og-konsulentbistand",
        label: "Rådgivning og konsulentbistand",
        description:
          "For dere som vil avklare behov, risiko og retning før dere går videre.",
      },
    ],
  },
  {
    slug: "google-meta-tiktok-snapchat-annonsering",
    canonical: "/innsikt/google-meta-tiktok-snapchat-annonsering",
    contentType: "Fagartikkel",
    label: "Google Ads, sosiale medier og KI",
    title:
      "Google Ads, Meta, TikTok og Snapchat: trendene som former annonsering i 2026",
    heading: "Annonsering med mennesker, KI og målbar verdi.",
    description:
      "En fagartikkel om Google Ads, Meta og Instagram, TikTok og Snapchat: KI-kreativitet, skapere, måling og kanalvalg for norske virksomheter.",
    summary:
      "KI kan gjøre kreativ produksjon raskere. Den erstatter ikke et tydelig tilbud, troverdig avsender eller et mål som betyr noe i virksomheten.",
    publishedAt: "2026-10-08",
    lastUpdated: "2026-10-08",
    lastUpdatedLabel: "8. oktober 2026",
    readingTime: "9 min lesing",
    relatedService: "/tjenester/google-og-meta-ads",
    hero: {
      // Photo: RDNE Stock project / Pexels (https://www.pexels.com/photo/content-creator-in-office-8370432/).
      image: "/images/insights/paid-social-creative.jpg",
      alt: "En innholdsskaper filmer med mobil, ringlys og bærbar datamaskin ved et arbeidsbord",
    },
    intro: [
      "Betalt synlighet er ikke én kanal. Google Ads møter ofte mennesker med et tydelig behov her og nå. Meta og Instagram, TikTok og Snapchat kan gjøre et tilbud kjent, vise produktet i bruk eller skape en grunn til å undersøke videre. Verdien ligger ikke i å være overalt, men i å velge riktig jobb for hver kanal.",
      "I 2026 går den samme utviklingen igjen hos annonseplattformene: KI hjelper med varianter, optimalisering og produksjon, mens skaperinnhold og egne kundesignaler blir viktigere for å være relevant. Det gjør det enklere å lage mer materiale, men stiller høyere krav til avsender, rettigheter, dokumentasjon og måling.",
    ],
    trend: {
      title: "Aktuelt nå: KI-innhold må også tåle å bli forklart",
      body: "Generativ KI brukes stadig mer for å lage og tilpasse annonsekreativer. I juli 2026 annonserte Google utvidet AI-merking, inkludert «How this ad was made», for innhold laget eller redigert med KI. Det er et tydelig signal: bruk gjerne verktøy som Higgsfield til å utforske vinkler, språk og videoformater, men behold menneskelig kvalitetssikring, riktige produktpåstander og en enkel rutine for transparens før materiale publiseres.",
      sourceHref:
        "https://blog.google/products/ads-commerce/google-ads-ai-transparency-labels/",
      sourceKind: "Kilde",
      sourceLabel: "Google: AI transparency labels in Ads",
      additionalSources: [
        {
          href: "https://about.fb.com/news/2026/01/2026-ai-drives-performance/",
          label: "Meta: AI tools and original content in 2026",
          description:
            "Metas plattformnyhet om KI-verktøy og distribusjon av originalt innhold.",
        },
        {
          href: "https://ads.tiktok.com/business/en-US/blog/tiktok-symphony-ai-creative-suite",
          label: "TikTok: Symphony Creative Studio",
          description:
            "TikToks plattformnyhet om KI-støttet kreativ produksjon.",
        },
        {
          href: "https://forbusiness.snapchat.com/blog/human-first-ai-enabled-snaps-latest-ads-innovations",
          label: "Snap: Human-first, AI-enabled ads innovations",
          description:
            "Snaps plattformnyhet om KI i kreativ produksjon, kampanjeoppsett og skaperpartnere.",
        },
      ],
    },
    sections: [
      {
        title: "Start med kundens situasjon, ikke med plattformen",
        body: [
          "Google Ads er ofte et naturlig sted å teste når en kunde allerede leter etter en løsning, et produkt eller en leverandør. Sosiale flater kan være bedre når dere må vise hvorfor tilbudet er interessant før behovet er formulert som et søk. Begge deler kan være riktig, men de løser ulike oppgaver og bør ikke vurderes på nøyaktig samme tall.",
          "Definer derfor én verdifull handling før dere kjøper rekkevidde: en kvalifisert henvendelse, en booket samtale, et salg eller et konkret steg i kjøpsprosessen. Da blir kanalvalget en hypotese som kan prøves, i stedet for en vane bygget på hvilke flater andre bruker.",
        ],
      },
      {
        title: "Bruk KI til flere gode forsøk – ikke til å late som",
        body: [
          "KI-verktøy kan gjøre det raskere å lage utkast, åpninger, formater og språkvarianter. En løsning som Higgsfield kan være nyttig for å utforske korte, vertikale videokonsepter før dere bruker mye tid på produksjon. Det er mest verdifullt når verktøyet inngår i en tydelig prosess: hva skal testes, hvilken påstand må være sann, og hvem godkjenner resultatet?",
          "Ikke bruk syntetiske stemmer, ansikter eller kundehistorier på en måte som kan oppfattes som en virkelig person eller ekte erfaring uten samtykke. For produkter med detaljer, pris, levering eller faglige påstander, må et menneske kontrollere at annonsematerialet stemmer med det kunden faktisk møter. Transparens er ikke bare et merke i en plattform; det er et tillitsspørsmål.",
        ],
      },
      {
        title: "La ekte skapere bidra der tillit ikke kan simuleres",
        body: [
          "Influencere og faglige skapere bør ikke behandles som et løst tillegg til medieplanen. Når noen faktisk kan vise et produkt, forklare en vanskelig vurdering eller snakke til en relevant målgruppe, kan innholdet brukes som en troverdig inngang til både organisk og betalt kommunikasjon.",
          "Avtal dette før innholdet lages: hvilken periode det kan brukes betalt, hvilke land og flater avtalen dekker, om materialet kan klippes om, hvem som godkjenner endringer og hvordan reklame merkes. Sammenlign så skaperinnhold med eget materiale i en avgrenset test. Et kjent ansikt er ikke i seg selv bevis på at budskapet eller tilbudet virker.",
        ],
      },
      {
        title: "Tilpass idéen til flaten den skal leve i",
        body: [
          "Ett innsiktspunkt kan godt bli til flere annonser, men en identisk video overalt er sjelden den beste testen. På Instagram og Facebook kan tydelig produktbevis, sosial troverdighet og et visuelt konsistent uttrykk være viktig. På TikTok bør starten raskt forklare hvorfor seeren skal bry seg, med språk og tempo som føles hjemmehørende i formatet.",
          "Snapchat kan være relevant når målgruppe, situasjon og format passer, men bør vurderes ut fra faktisk dekning og tilgjengelige annonseprodukter i markedet deres. Sjekk alltid hva som er tilgjengelig i Ads Manager før dere planlegger produksjon rundt en spesifikk funksjon. Poenget er ikke å kopiere en trend, men å lage noe som føles nyttig i konteksten kunden møter det.",
        ],
      },
      {
        title: "Gi algoritmene et bedre signal enn et tilfeldig klikk",
        body: [
          "Automatisert budgivning og optimalisering kan bare jobbe med signalene dere gir den. Hvis alle skjemaer behandles likt, lærer kampanjen at et svakt kontaktpunkt er like verdifullt som en reell salgsmulighet. For B2B bør dere derfor kunne skille mellom et nytt lead, et kvalifisert lead, en mulighet og vunnet omsetning i CRM-et.",
          "Koble bare samtykkede og kvalitetssikrede hendelser til annonseplattformene. Bruk konsekvente UTM-parametere, definer eierskap til dataene og følg opp om salgsresultatet stemmer med det kampanjene rapporterer. Hashing eller teknisk sporing er ikke i seg selv et grunnlag for behandling av personopplysninger; avklar samtykke, dataminimering og juridiske krav med ansvarlige fagpersoner.",
        ],
      },
      {
        title: "Bygg en liten læringssløyfe før dere skalerer",
        body: [
          "En god første test trenger ikke mange kampanjer. Velg én målgruppe, ett tilbud, én viktig konvertering og noen tydelig forskjellige kreative vinkler. Det kan være en ekspert som forklarer problemet, et konkret produktbevis og en kort kundesituasjon. Test én ting om gangen nok til at dere kan forstå hva som påvirket resultatet.",
          "Se både på respons i kanalen og på kvaliteten som kommer videre. Et kreativt konsept kan gi mange klikk uten å skape gode samtaler, og en søkekampanje kan ha lavere volum men høyere andel kvalifiserte henvendelser. Del læringen med salg, kundeteam og de som lager innhold, slik at neste variant bygger på en faktisk innsikt.",
        ],
      },
    ],
    checklist: [
      "Vi har bestemt hvilken forretningshandling annonseringen skal bidra til.",
      "Vi har valgt kanal etter kundens situasjon, ikke bare etter rekkevidde.",
      "Vi har flere kreative vinkler som tester en tydelig hypotese.",
      "Vi har kvalitetssikret KI-generert materiale, produktpåstander og transparens.",
      "Vi har rettigheter, godkjenning og eventuell reklamemerking på plass for skaperinnhold.",
      "Vi måler kvalifiserte leads eller salg – ikke bare klikk og skjemaer.",
      "Vi bruker bare samtykkede, relevante data i måling og optimalisering.",
      "Vi har avtalt hvem som vurderer læringen og bestemmer neste test.",
    ],
    faqs: [
      {
        question: "Må vi være på Google, Meta, TikTok og Snapchat samtidig?",
        answer:
          "Nei. Start der dere kan forklare hvorfor kanalen passer kundens situasjon og tilbudet. Google kan fange eksisterende etterspørsel, mens sosiale flater kan være gode for å vise verdi før noen søker. Test én eller to klare hypoteser først.",
      },
      {
        question:
          "Er KI-video og AI-influencere en god erstatning for ekte skapere?",
        answer:
          "De kan gjøre det raskere å teste formater, åpninger og lokalisering, men erstatter ikke reell fagkunnskap, produkttesting eller tillit. Bruk KI til variasjon og produksjon, og la ekte mennesker bære budskap som krever erfaring eller troverdighet. Ha alltid kontroll på samtykke, rettigheter og transparens.",
      },
      {
        question: "Hvordan bruker vi influencerinnhold i betalte kampanjer?",
        answer:
          "Avtal betalt bruksrett, periode, markeder, flater, redigering og merking før innholdet produseres. Test så materialet mot egne annonser med samme mål og måling. Resultatet skal avgjøre om dere fortsetter, ikke følgerens størrelse alene.",
      },
      {
        question:
          "Hvordan sammenligner vi Google Ads med annonser i sosiale medier?",
        answer:
          "Sammenlign dem mot den samme forretningsverdien, men respekter at de kan ha ulike roller i reisen. Se etter kvalifiserte leads, samtaler, salg eller andre avtalte steg i CRM-et – ikke bare klikkpris eller visninger isolert.",
      },
      {
        question: "Er TikTok og Snapchat relevant for B2B?",
        answer:
          "Det kan de være når relevante mennesker faktisk bruker flaten, budskapet kan formidles godt i formatet og dere har noe konkret å teste. B2B er ikke én målgruppe. Begynn med målgruppen og kjøpssituasjonen, og vurder tilgjengelighet og måling i det aktuelle markedet før dere investerer stort.",
      },
    ],
    relatedLinks: [
      {
        href: "/tjenester/google-og-meta-ads",
        label: "Google + Meta Ads",
        description:
          "Se hvordan vi jobber med strategi, sporing og forbedring av betalt annonsering.",
      },
      {
        href: "/tjenester/ki-og-automatisering",
        label: "KI og automatisering",
        description:
          "Når KI skal inn i en konkret arbeidsflyt med tydelige rammer og kontrollpunkter.",
      },
      {
        href: "/tjenester/radgivning-og-konsulentbistand",
        label: "Rådgivning og konsulentbistand",
        description:
          "For dere som vil velge retning, risiko og neste test før dere skalerer.",
      },
    ],
  },
];

export function findInsight(slug: string) {
  return insights.find((insight) => insight.slug === slug);
}
