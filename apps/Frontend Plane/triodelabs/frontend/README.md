## First-party inquiry receipts (local development)

`CONTACT_DELIVERY_MODE=leads` routes validated contact submissions to Leads Core
instead of Resend. A success response requires a durable receipt. Reusing the
submission ID returns the same receipt; changing its content returns a conflict.
No Google or Clarity event is emitted from the form, and no contact fields enter
Marketing Core. Existing Resend delivery stays available when this mode is unset.

Configure only server-side `.env.local` values:

```dotenv
CONTACT_DELIVERY_MODE=leads
CONTACT_WEBSITE_INTAKE_URL=http://127.0.0.1:3164/api/v1/website-inquiries
CONTACT_WEBSITE_INTAKE_TOKEN_FILE=/absolute/protected/installation-write-token
```

The token file must be a regular file with no group/world permissions. Local HTTP
works only in development; a deployed installation requires HTTPS and a reachable
Leads ingress. The current local installation is bound to Triodelab's registered
website ID/revision and `development`. Pulse's outcome panel defaults to production;
select Development to inspect local receipts. Production totals and GA4 key events
are separate, and bookings remain unavailable until a booking provider confirms them.
Changing a site's URL/revision requires an authorized installation update. Never reuse
an installation credential for another website or organization.

# TriodeLab – nettside

En moderne, profesjonell nettside for TriodeLab - et Oslo-basert digitalbyrå som spesialiserer seg på webutvikling, design og digital strategi.

## 🚀 Funksjoner

- **Moderne Design**: Clean, minimalistisk design med fokus på brukeropplevelse
- **Responsiv**: Optimalisert for alle enheter og skjermstørrelser
- **SEO-optimalisert**: Inkluderer meta-tagger, sitemap og robots.txt
- **Ytelse**: Next.js-bildeoptimalisering, lokal WOFF2-font og utsatt lasting av chat
- **Tilgjengelighet**: Skip-lenker, semantisk HTML og redusert animasjon ved brukerpreferanse; ikke en full WCAG-sertifisering

## Markedsføringsmåling og provider-tilkoblinger

Denne delen beskriver nåværende kildekode etter oppdateringen til den
engelsk/norske nettsidestrukturen. Den må ikke forveksles med en produksjons-
deploy: endringene nedenfor ligger lokalt og må bygges, gjennomgås og godkjennes
før publisering.

- **Google Search Console:** `GOOGLE_SITE_VERIFICATION` leveres som metadata fra
  `getRootMetadata`. Dette er kun nettstedseiers verifikasjon; OAuth-bearer og
  rapportering skal gå via Integration Core/Marketing Core, aldri nettleseren.
- **GA4 og Microsoft Clarity:** `ConsentAnalytics` i delt `RootDocument` laster
  leverandørskriptene først når `NEXT_PUBLIC_ANALYTICS_ENABLED=true`, Cookiebot
  Statistics er valgt, domenet/ruten er tillatt og DNT/GPC ikke er aktivert.
  Nåværende offentlige installasjons-ID-er er `G-ZEWPHF6X3K` og `z5gsvus553`;
  ingen OAuth-grant eller Clarity eksporttoken ligger i frontend.
- Målingen er begrenset til godkjente norske og engelske offentlige sider.
  Kontakt-, juridiske og ukjente sider, URL-er med query/hash og sensitive
  skjema-/chatinteraksjoner utelates. Hele dokumentet maskeres for Clarity.
- **Google Ads og PageSpeed Insights:** disse er ikke nettlesertagger i dette
  prosjektet. PageSpeed-kjøringer går server-side gjennom Integration Core og
  Marketing Core. Integration Core har Google Ads-scope-katalog, men mangler
  fortsatt Google Ads API-handlinger; kampanjeoppretting/-styring er derfor ikke
  implementert. Ingen Google Ads conversion-ID/pixel er konfigurert her. Ikke
  legg til en før handlinger, måleformål, consent-kategori og ID er avklart.
  Kampanjeendringer må alltid ha eksplisitt godkjenning.
- Lokal standard er `NEXT_PUBLIC_ANALYTICS_ENABLED=false`. GA4/Clarity IDs er
  offentlige måle-ID-er, ikke hemmeligheter. Sett aldri provider-token, API-nøkkel
  eller OAuth-bearer i `NEXT_PUBLIC_*`-variabler.

## 🛠️ Teknologier

- **Framework**: Next.js 15 (App Router)
- **Styling**: Tailwind CSS
- **Animasjoner**: Framer Motion
- **Type Safety**: TypeScript
- **Icons**: Lucide React
- **Forms**: React Hook Form + Zod
- **Deployment**: Vercel (anbefalt)

## 📁 Prosjektstruktur

```
frontend/
├── src/
│   ├── app/                 # Next.js App Router
│   ├── components/          # Gjenbrukbare komponenter
│   │   ├── ui/             # UI-komponenter (Button, Input, etc.)
│   │   ├── layout/         # Layout-komponenter (Navigation, Footer)
│   │   └── chat/           # Chat-widget komponenter
│   ├── features/           # Side-seksjoner
│   │   ├── hero/           # Hero-seksjon
│   │   ├── about/          # Om oss-seksjon
│   │   ├── services/       # Tjenester-seksjon
│   │   ├── portfolio/      # Portfolio-seksjon
│   │   ├── team/           # Team-seksjon
│   │   ├── technologies/   # Teknologier-seksjon
│   │   ├── pricing/        # Priser-seksjon
│   │   ├── faq/            # FAQ-seksjon
│   │   └── contact/        # Kontakt-seksjon
│   ├── lib/                # Utility-funksjoner og konfigurasjon
│   └── styles/             # Globale stiler
├── public/                 # Statiske filer
└── package.json
```

## 🚀 Kom i gang

### Forutsetninger

- Node.js 20.9+ (bruk en støttet LTS-versjon, anbefalt 22 eller nyere)
- npm eller yarn

### Installasjon

1. **Klon repositoriet**

   ```bash
   git clone https://github.com/triodelab/triodelab-web.git
   cd triodelab-web/frontend
   ```

2. **Installer avhengigheter**

   ```bash
   npm install
   # eller
   yarn install
   ```

3. **Start utviklingsserveren**

   ```bash
   npm run dev
   # eller
   yarn dev
   ```

4. **Åpne nettleseren**
   ```
   http://localhost:3000
   ```

## 📝 Miljøvariabler

Bruk `.env.example` som mal for `.env.local`. Hemmelige nøkler skal aldri sjekkes inn i Git eller ha `NEXT_PUBLIC_`-prefiks. Offentlige installasjons-ID-er for besøksmåling er ikke hemmelige nøkler; se den separate aktiveringen nedenfor.

| Variabel                   | Bruk                                                                      |
| -------------------------- | ------------------------------------------------------------------------- |
| `RESEND_API_KEY`           | Servernøkkel for innsending gjennom Resend                                |
| `CONTACT_FROM_EMAIL`       | Ren e-postadresse på et domene som er verifisert i Resend                 |
| `CONTACT_TO_EMAIL`         | Mottaker; standard `post@triodelab.no`                                    |
| `UPSTASH_REDIS_REST_URL`   | HTTPS REST-endepunkt til Upstash Redis                                    |
| `UPSTASH_REDIS_REST_TOKEN` | Servernøkkel med tilgang til GET og EVAL-skriptenes Redis-operasjoner     |
| `GOOGLE_SITE_VERIFICATION` | Valgfri Search Console-verifisering; DNS-verifisering kan brukes i stedet |

Kontaktsiden viser en direkte e-postlenke til påkrevd serverkonfigurasjon er gyldig. Ingen innsending vises som vellykket uten at e-postleverandøren har akseptert den. Gyldige miljøvariabler alene beviser ikke at domene, nøkler eller levering fungerer.

### Aktivering av kontaktskjema

1. Opprett/velg Resend og Upstash, og avklar databehandleravtaler, lagring/region og faktisk bruk før aktivering.
2. Verifiser avsenderdomenet i Resend med leverandørens DNS-oppsett, og velg en godkjent avsenderadresse.
3. Legg de fire påkrevde variablene over i riktig Vercel-miljø. Legg eventuelt til en annen mottaker.
4. Deploy etter godkjenning. Test én avtalt reell innsending og kontroller både leverandørstatus og mottatt e-post. De automatiske testene sender aldri ekte e-post.

Misbruksvernet tillater fem forsøk per nettverksidentifikator per ti minutter og 100 samlet. På Vercel brukes plattformens betrodde IP-header; på andre verter deles en konservativ grense til en betrodd proxy er konfigurert i koden. Manglende/feilende misbruksvern stenger innsendingen, men direkte e-post er fortsatt tilgjengelig.

Produksjonsdomenet er observert bak Cloudflare. Verifiser betrodd proxy-oppsett og at separate besøkende faktisk får separate begrensningsnøkler før skjemaet aktiveres; ellers kan flere besøkende dele Cloudflares IP-grense. Ikke stol direkte på `CF-Connecting-IP` uten et beskyttet og verifisert origin-oppsett. Se [Vercels forespørselshoder](https://vercel.com/docs/headers/request-headers).

### Måling uten besøksprofilering

Vi teller bare aksepterte skjemaforespørsler, ikke garantert levering til innboks, e-postlenkeklikk eller inngåtte avtaler. Kilde følger tjenestelenken til `/kontakt?tjeneste=<tjenesteslug>`. Bare en avgrenset liste tjenestenavn godtas; ukjente verdier blir `direct`.

Se `HGETALL contact:leads:YYYY-MM-DD` i Upstash-konsollen (kalenderdag i Europe/Oslo). `projectType:*` og `source:*` er to fordelinger av de samme henvendelsene: summer bare én for å få totalen. Disse daglige aggregatene slettes etter 400 dager. De har ingen navn, e-post, meldingstekst eller IP-adresse.

Separat teknisk lagring bruker HMAC-pseudonymiserte IP-er i ti minutter og leveringskvittering med nøkkelhåndtert fingeravtrykk av innsendingen i 48 timer. Resend-idempotens og kvitteringen begrenser duplikater ved umiddelbare forsøk på nytt. Dette er ikke en garanti for nøyaktig én levering ved alle leverandørfeil. Ikke logg innhold eller nøkler ved feilsøking. Operasjonelle feil bruker kun `contact_rate_limit_unavailable`, `contact_delivery_unconfirmed` og `contact_metrics_unavailable`.

Besøksmåling er deaktivert som standard. Chatten kontakter Agenci først etter besøkendes aktivering. Gjennomgå personvern- og cookieinformasjonen på nytt før nye sporingsverktøy aktiveres.

### Historikk: tidligere GA4-publisering (forrige nettsideversjon)

Dette beskriver den forrige produksjonsversjonen, ikke den nye engelsk/norske
`main`-strukturen. GA4 med `G-ZEWPHF6X3K` ble tidligere publisert og testet.
Clarity er siden konfigurert med prosjekt-ID `z5gsvus553`. Sporingsmodulen,
rutekartet og personverntekstene er nå gjeninnført lokalt i den nye strukturen;
se «Markedsføringsmåling og provider-tilkoblinger» ovenfor. Den nye koden er ikke
publisert. Variablene er offentlige installasjons-ID-er:

| Variabel | Bruk |
| --- | --- |
| `NEXT_PUBLIC_ANALYTICS_ENABLED` | Må være nøyaktig `true` for å tillate oppstart; la den være unset/`false` til godkjent aktivering |
| `NEXT_PUBLIC_GA4_MEASUREMENT_ID` | Standard er `G-ZEWPHF6X3K`; en eksplisitt gyldig verdi overstyrer den. Tom/ugyldig overstyring deaktiverer GA4 fremfor å falle tilbake. Ikke property-ID eller API-nøkkel |
| `NEXT_PUBLIC_CLARITY_PROJECT_ID` | Offentlig prosjekt-ID fra riktig Clarity-prosjekt, aldri eksporttoken |

OAuth-grant og Clarity-eksporttoken skal bare håndteres av Integration Core via
godkjent hemmelighetslagring. De skal ikke finnes i denne nettsiden, en
`NEXT_PUBLIC_`-variabel, kildekode, nettadresse eller chat.

`NEXT_PUBLIC_`-verdier bygges inn i klientpakken ved `next build`; endrede
installasjons-ID-er krever en ny godkjent build/deploy. Det er ikke nok å legge
til en måle-ID for å starte sporing: aktiveringsflagget må også være nøyaktig
`true`, og de vanlige samtykke-, domene- og rutebegrensningene gjelder fortsatt.

Sporing krever Cookiebot **Statistics**-samtykke, gyldig leverandør-ID og en
eksplisitt tillatt offentlig rute på triodelab.no/www.triodelab.no. Kontakt,
juridiske sider og ukjente/private ruter er ekskludert i første versjon. Preview
og localhost starter ikke ekte sporing. Reklamesamtykke gis ikke implisitt.
Fravær av samtykke skal hindre at skriptene lastes, ikke bare nekte kapsler.
Tilbaketrekking stopper målingen og laster dokumentet på nytt; bare å fjerne et
script-element ville ikke stoppe observatører som allerede kjører.

GA4-klienten er klargjort for å beholde **Enhanced Measurement uendret** i
nettstrømmen. Den sender ett automatisk `config` med `send_page_view: true` per
dokument og ingen egen `page_view`-hendelse. Etter oppstart brukes ny dokumentlast
ved interne URL-endringer, ikke ekstra manuelle SPA-sidevisninger. Dette er en
kode-/syntetisk testverifikasjon, ikke et bevis på antall faktiske Google-kall.
Leverandørens automatiske hendelser og parametere må kontrolleres ved godkjent,
kontrollert aktivering før innsamlingen erklæres verifisert; innstillingen er
ikke endret herfra.

Enhanced Measurement leser selv enkelte nettadresser og hendelsesdata. Derfor
starter ingen sporingsskript på sider med query/hash. Tillatte referrere er tom
referrer, eksplisitt tillatte offentlige Triodelab-sider uten query/hash, og rene
HTTPS-rotnettadresser fra `google.com`, `www.google.com`, `google.no`,
`www.google.no`, `bing.com`, `www.bing.com`, `duckduckgo.com` og
`www.duckduckgo.com`. Søkemotorreferrere med sti, port, query/hash eller
brukerinfo avvises; ingen søkeord samles inn herfra. Bare den validerte offentlige
referreren sendes som `page_referrer`, slik at disse søkemotorbesøkene ikke
automatisk mister kildeinformasjonen. Andre eksterne henvisninger er fortsatt
utelatt fra besøksmålingen; rapportene har derfor bevisst begrenset dekning.
Tidlige capture-lyttere deaktiverer GA4 og nekter videre målingssignaler før
skjema/chat og utgående, mailto/tel-, nedlastings- eller modifiserte/andre
mål-lenkehandlinger. Besøkendes navigasjon og skjemainndata fortsetter uten
tvangslasting; målingen starter ikke igjen i samme dokument. Cookiebots egne
kontroller kan fortsatt gi første samtykke. Clarity er ikke aktivert for Triodelab.

Sjekkliste ved nye aktiveringer (ikke alle punktene er utført i live-testen):

1. Bekreft at den eksisterende GA4-nettstrømmen `G-ZEWPHF6X3K` tilhører riktig
   konto/property for `post@triodelab.no`; ikke opprett en duplikatstrøm.
   Kontroller Norge/Europe-Oslo, NOK og virksomhetsdetaljer med eier.
   Behold **Enhanced Measurement uendret**, slik eier har bedt om. Ikke kombiner
   automatisk måling med en egen manuell `page_view`. Gjennomgå faktisk
   automatisk hendelsesomfang og parametere i den godkjente nettverkstesten;
   `page_location`-overstyring alene sanerer ikke alle Enhanced-parametere.
   Pulse har nå oppdaget property `560102923` og strøm `16177443323` gjennom
   Integration Core. Strømmen/registrert nettsted bruker `https://triodelab.no`,
   mens den offentlige siden videresender til `https://www.triodelab.no`.
   Denne forskjellen er nå eksplisitt konfigurert i Pulse: GA4-rapporteringsverten
   er `www.triodelab.no`, mens website/strøm og domenebasert GSC beholdes.
   Integration Core krever ny domeneeierkontroll og filtrerer eksakt www-vert og
   strøm-ID. Ikke fjern vertsfilteret. De to godkjente testsidevisningene er nå
   verifisert i Googles sanntidsrapport.
2. Velg/godkjenn et separat Triodelab Clarity-prosjekt med korrekt nettadresse.
   Gjennomgå Strict masking, samtykkekrav, eventuell reklamedeling, lagring og
   sensitive ruter. Kroppsmaskering i koden erstatter ikke prosjektinnstillingene.
3. Eier må gjennomgå `/personvern` og `/cookies` før nye verktøy aktiveres.
   De reviderte tekstene ble godkjent og publisert 22. oktober. Oppgi faktisk
   formål, leverandører, data/kapsler og deres varighet, samtykke og hvordan det
   trekkes tilbake. Ingen juridisk etterlevelse er bekreftet av en teknisk test.
4. Godkjenn produksjonsendringen før publisering. Begge lokale `.vercel`-koblinger
   peker på prosjektet `triodelabs`; bekreft riktig kilde/build og gjennomgå
   eksisterende lokale endringer før en deploy. Nåværende frontend, inkludert
   tidligere lokale endringer, ble eksplisitt godkjent før produksjonsdeploy.
5. Test ingen valg, avvisning, kun Statistics, tilbaketrekking, ny innlasting,
   navigasjon og skjema/chat. Kontroller faktiske nettverkskall og kapsler, samt
   at samme tag ikke lastes dobbelt. De automatiske testene bruker syntetiske
   porter og sender aldri ekte måledata.
6. Etter godkjent aktivering, bruk et avtalt ikke-personlig besøk uten
   skjemainnsending. Bekreft det i GA4 Realtime/DebugView og riktig Clarity-prosjekt
   før rapportering via Integration Core testes. Ikke send syntetiske hendelser
   som om de var reelle besøk.

Pulse bruker aggregerte rapporter, ikke disse leverandørenes realtime-visning.
Den nåværende GA4-kvartalsperioden slutter på foregående kalenderdag; et helt
nytt prosjekt vil ikke straks ha historiske tall. Clarity-eksporten har opptil
72 timers rullerende UTC-vindu, lav leverandørkvote og seks timers mellomlagring.
Registrer observasjonstid og skill manglende data fra et faktisk nullresultat.

Publiseringsstatus 22. oktober 2026: GA4 er koblet gjennom Integration Core,
og Pulse har en eksplisitt rapporteringsvert `www.triodelab.no` med ny
Google-domeneeierkontroll ved hver alternativ vertsrapport. Website-ID,
strømmens apex-URL og domenebasert GSC-kobling beholdes. Google-innstillingene
viser 2 måneder for hendelsesdata og 14 måneder for brukerdata, med tilbakestilling
ved ny aktivitet. De oppdaterte personvern-/cookietekstene og samtykkeknappen
er publisert etter eiers uttrykkelige godkjenning.

Vercel-koblingen er kontrollert: prosjekt `triodelabs`, team
`team_cW7J2entnq3fYKzQb3UutF9k`, Root Directory `frontend`. CLI brukes fra
foreldremappen `triodelabs`, ikke fra en ny konto eller et nytt prosjekt.
Foreldremappens testede `.vercelignore` tillater bare frontend-kilder og
ressurser, og utelater legacy-backend, `.env`-/nøkkelfiler, avhengigheter og
byggeutdata. Ikke last ned produksjonshemmeligheter til en lokal `.env` for
denne publiseringen; eksisterende serverhemmeligheter blir i Vercel.
Produksjonsdeploy `dpl_5ziPRo2uXckkcpz6JEPR6FBB6owK` er Ready og aliasert til
begge produksjonsvertene: <https://triodelabs-pjw1fqy79-royal-revue.vercel.app>.
Offentlige produksjonsvariabler er `NEXT_PUBLIC_ANALYTICS_ENABLED=true` og
`NEXT_PUBLIC_GA4_MEASUREMENT_ID=G-ZEWPHF6X3K`. Clarity-ID ble eksplisitt holdt
tom i bygget; eksisterende serverhemmeligheter ble ikke lest eller flyttet.
Virkelig CLI dry-run bekreftet 263 frontend-filer og ingen backend-, env-,
nøkkel-, dependency- eller genererte byggfiler før opplasting. Den tidligere
produksjonsdeployen `dpl_GwqRGAzX9tNHAV9ZdmuDp2KsQXhT` er rollback-kandidat.

Live-test ca. 01:55–01:57 Europe/Oslo 22. oktober: to reelle, rene dokumentbesøk
til `/` og `/om-oss`, uten skjema/chat eller syntetiske analysehendelser.
Første besøk hadde eksisterende Cookiebot-samtykke til alle kategorier; før
andre besøk ble det begrenset til kun Nødvendig + Statistikk. Koden nekter
annonseringssignaler uansett. Google Realtime viste én aktiv bruker og eksakt
to `page_view` (én per side), samt `first_visit`, `session_start` og én automatisk
`scroll`. Enhanced Measurement er uendret. Begge dokumentene hadde én korrekt
GA4-script-tag og ingen Clarity-script; ingen console errors/warnings ble observert.
Dette beviser mottak av testbesøkene, ikke en full nettverks-/kapselrevisjon eller
at avvisning/tilbaketrekking/skjemavern er live-testet i denne produksjonsrunden.
Pulse viste samtidig tilkoblet www-skopet GA4 med tom historikk for 1.–20. oktober;
dagens besøk ligger utenfor perioden og skal ikke fylles inn med oppdiktede tall.
Publiserte `/personvern` og `/cookies` returnerte HTTP 200 med reviderte tekster.

Ny verifikasjon: 171 tester bestått, én valgfri CLI-preflight hoppet over i
normalsuiten; den reelle CLI-preflighten besto separat (3 tester). Typekontroll,
målrettet lint og produksjonsbygg besto. Produksjonsavhengighetenes audit hadde
ingen kjente sårbarheter; Vercels fulle install-audit rapporterte 13 funn i
utviklingsavhengigheter (5 moderate, 8 høye), som ikke er utbedret her.

Lokal verifikasjon 21. oktober: 127 tester bestått, inkludert 45 syntetiske
sporings-/adapter-/konfigurasjonstester. Berørte sporingsmoduler har 100 %
linjedekning og 93,85 % grendekning; dette er ikke en dekningspåstand for hele nettstedet.
Typekontroll, målrettet ESLint, sikkerhetsgjennomgang og produksjonsbygg besto.
In-app browser på localhost:3335 viste forside og Om oss uten GA4-/Clarity-script.
Cookiebot varslet at 127.0.0.1 ikke er godkjent for eksisterende domenegruppe;
derfor er ekte CMP-/nettverkstesting med aktiv sporing ikke verifisert lokalt.
Ikke legg til domener i Cookiebot eller omgå vertsgrensen uten godkjenning.
Det lokale treet inneholder også tidligere nettstedendringer; gjennomgå hele
publiseringsgrunnlaget, ikke bare sporingsfilene, før produksjonsdeploy.

Oppdatering 22. oktober 2026: Triodelabs Clarity-prosjekt er opprettet med
offentlig prosjekt-ID `z5gsvus553`. Den lokale frontend-konfigurasjonen bruker
denne ID-en gjennom samme Statistics-samtykke-, DNT/GPC-, rute- og
interaksjonsvern som GA4; det legges ikke inn en ekstra ubetinget `<script>`.
Cookiebot Statistics-samtykke kreves før Clarity-scriptet lastes, og
annonseringslagring holdes avslått. Nettstedets globale `data-clarity-mask="True"`
maskerer innhold i replay, og inputfelter maskeres av Clarity. Den nye koden og
oppdaterte personvern-/cookieopplysningene er lokale endringer og er ikke
produksjonspublisert ennå. In-app-browserens Clarity-installasjonsside viser
prosjektets tracking-kode; reell mottakelse i prosjektet er ikke verifisert.

### Ny bilingual release — publiseringsgrunnlag 22. oktober 2026

Eier har bedt om å pushe og publisere den nåværende norsk/engelske nettsiden,
inkludert samtykkestyrt GA4 `G-ZEWPHF6X3K`, Clarity `z5gsvus553`, tidligere
SEO-/innholdsarbeid, Innsikt-sidene og kontaktfallback. Før release ble
`origin/main` hentet og kontrollert lik lokal `main` (`47ff26f`); staged og
ustaged frontendarbeid beholdes samlet. Ingen produksjonshemmeligheter er lest.

Forhåndskontroll: 201 tester bestått og én valgfri CLI-test hoppet over i
normalsuiten. Den virkelige Vercel CLI-dry-run-testen besto separat (3 tester),
inkludert begge språkgruppenes sidefiler og uten backend, miljø-/nøkkelfiler,
avhengigheter eller byggutdata. Typekontroll og produksjonsbygg (42 sider)
besto. ESLint har ingen feil og én eksisterende advarsel om `<head>` i delt
root-dokument. Produksjonsavhengighetenes audit rapporterte null sårbarheter.

Sikkerhetsgjennomgangen gjenopprettet `data-consentmode="disabled"` på den
delte Cookiebot-taggen: applikasjonen er eneste myndighet for de avslåtte
annonseringssignalene. Regresjonstester dekker begge språk. Microsofts
personvernlenke vises nå faktisk på begge språk, ikke bare som lenkedata.
Enhancement-innstillingene hos Google er ikke endret. Dette dokumenterer
publiseringsgrunnlaget; ny commit/deploy og virkelig leverandørmottak må
registreres etter at publisering og nettlesertesten er gjennomført.

Primærkilder: [GA4-sidevisninger](https://developers.google.com/analytics/devguides/collection/ga4/views),
[Cookiebot-hendelser](https://www.cookiebot.com/en/developer/),
[Clarity-samtykke](https://learn.microsoft.com/en-us/clarity/setup-and-installation/clarity-consent-api-v2),
[Clarity personvern](https://learn.microsoft.com/en-us/clarity/setup-and-installation/clarity-data),
[Clarity datalagring](https://learn.microsoft.com/en-us/clarity/setup-and-installation/data-retention),
[Microsoft personvernerklæring](https://privacy.microsoft.com/privacystatement).

### Førsteparts Web Vitals — ny innhentingsgrense

`ConsentWebVitals` i delt `RootDocument` laster standardbiblioteket `web-vitals`
først etter Cookiebot **Statistikk**. DNT/GPC, ukjente/private sider,
søkestrenger, fragmenter og skjema-/chatinteraksjon stopper innhenting.
Dokumentmåling stoppes etter SPA-navigasjon for å unngå feil sidetilknytning.
Klienten sender bare LCP/INP/CLS, godkjent sidesti, vindusbreddegruppe,
tilfeldig dokument-ID og sekvensnummer. Ingen cookies, DOM-attribusjon,
henvisningsadresse eller brukeridentifikator sendes til samleren.

Flyt: nettleser → samme-opphav `POST /api/web-vitals` → Redis → autentisert
`GET /api/web-vitals/export` → Integration Core → Marketing Core → Pulse.
Eksporttoken og Redis-passord er serverhemmeligheter, aldri `NEXT_PUBLIC_*`.
Sammenknytt nettsted-ID, gjeldende revisjon, eksakt rapporteringsopphav og
miljø i begge serveres konfigurasjon. Nettleserdata er uverifiserte
ytelsessignaler, ikke en kryptografisk attestasjon av besøk eller samtykke.

Se `.env.example`. Produksjon krever `rediss://` til varig, ikke-evikterende
Redis-lagring, korrekt eier-/tilgangsgodkjent behandling og eksplisitt
aktivering av begge Web Vitals-bryterne. Lokal utvikling bruker eget miljø,
egen eksportnøkkel og `http://localhost:3000`; lokale tester må aldri merkes
som produksjon. `*_FILE`-varianter krever absolutte, vanlige mode-0600-filer.
Manglende lagring feiler lukket med 503, ikke falske tomme rapporter.

Siste sekvens per dokument-ID/metrikk erstatter tidligere måling. Råmålinger
og dagsindekser har absolutte utløpstider: maksimalt syv UTC-kalenderdager,
inkludert første mottaksdag. Oppdateringer forlenger ikke lagringstiden.
Grensen er 5 000 målinger og 120 innsendinger per minutt per installasjon.
Pulse viser separat p75 og antall per metrikk/vindusbredde, varsler om små
utvalg og lar manglende INP være ukjent. Dette er ikke CrUX-dekning,
Lighthouse-poeng eller en samlet bestått Core Web Vitals-vurdering.
Import i Pulse er foreløpig eksplisitt; denne endringen aktiverer ikke en
bakgrunnsjobb for eksportimport. Live aktivering og publisering gjenstår
inntil produksjonslagring og reviderte personvernopplysninger er godkjent.

Lokal kontroll 23. oktober 2026: eksport uten legitimasjon ga 401, samleren
avviste forespørsel uten korrekt opphav med 403, og autentisert eksport ga
200 med tomt måleutvalg. Import fra Pulse gikk gjennom Integration Core og
Marketing Core og viste «Venter på samtykkebaserte nettlesermålinger» i
utviklingsmiljøet. Ingen syntetiske målinger ble lagt inn i dashboardet.
Cookiebot-gruppen godkjenner ikke `localhost`, så ekte nettleserinnsamling
var først blokkert. Et separat, eksplisitt lokalt samtykke er nå tilgjengelig
kun med `NODE_ENV=development`, Web Vitals-bryteren på og nøyaktig
`http://localhost:3000` på en godkjent offentlig side. Panelet «Local Web
Vitals test» krever klikk på «Allow local Web Vitals» før biblioteket lastes.
Det setter ikke Cookiebot-/GA4-/Clarity-samtykke, bruker ikke cookies eller
nettleserlagring, og begynner avslått i hvert nytt dokument. DNT/GPC gjelder
fortsatt. «Stop local Web Vitals» stanser nye rapporter og laster siden på
nytt; allerede mottatte målinger slettes ikke retroaktivt. Ved SPA-navigasjon
må dokumentet lastes på nytt før en ny test.

Verifisert i Codex in-app browser 23. oktober 2026: null målinger før lokalt
samtykke; vanlig menyinteraksjon og full navigasjon ga faktisk LCP, INP og
CLS fra nettleseren i lokal Dragonfly. Pulse-import gjennom Integration Core
og Marketing Core viste tre målinger i utviklingsmiljøet med varsel om lite
utvalg. Ingen syntetiske verdier ble injisert. Tilbaketrekking og nytt
dokument viste samtykke av; ingen GA4-/Clarity-script ble lastet. Tallene er
dev-pipelinebevis, ikke en produksjonsytelse eller bestått CWV-vurdering.
Produksjons-Redis og live publisering er fortsatt ikke konfigurert.
Guard-kontroll: produksjonsbygget ble også bygget med Web Vitals-flagget på
og kjørt midlertidig på localhost:3000. Etter hydrering og vanlig
menyinteraksjon var dev-samtykkepanelet fraværende. Deretter ble den lokale
dev-serveren startet igjen. Cookiebots produksjonsoppsett er uendret.
Redis-integrasjonstester bruker tilfeldig isolert nettsted/namespace i
testdatabase 15, aldri nettstedets rapporteringsnamespace, og rydder bare
egne testnøkler. Oppdatert dev-oppsett bruker en dedikert Docker Dragonfly på
`127.0.0.1:6481` (egen nøkkel og volum), ikke den delte cachen på 6480.
Cache-eviksjon er av. Faste snapshot-filer overskrives hvert minutt og ved
grasiøs stopp. Omstartstest bekreftet gjenoppretting med uendret utløpstid.
Krasj kan miste data siden siste vellykkede snapshot, og offline snapshot-filer
blir liggende til neste kjøring/lagring. Dette er utviklingspersistens, ikke
produksjonsgarantier. Etter brukerens valg fortsetter vi kun i dev; ingen
Upstash-konto eller live publisering skal opprettes for dette arbeidet.

## 🏗️ Bygg for produksjon

```bash
# Bygg prosjektet
npm run build

# Start produksjonsserver
npm start

# Lint kode
npm run lint

# Type check
npm run type-check
```

## 📱 Responsiv Design

Nettsiden er optimalisert for følgende breakpoints:

- **Mobile**: 320px - 768px
- **Tablet**: 768px - 1024px
- **Desktop**: 1024px+

## 🎨 Design System

### Farger

- **Primary**: #000000 (Svart)
- **Secondary**: #64748b (Slate)
- **Background**: #ffffff (Hvit)
- **Accent**: #3b82f6 (Blå)

### Typografi

- **Font**: Manrope, lokalt lastet som variabel WOFF2-font
- **Headings**: Font-weight 900 (Black)
- **Body**: Font-weight 400 (Regular)

### Spacing

- **Container**: max-w-7xl (1280px)
- **Section padding**: py-24 (96px)
- **Component spacing**: gap-6 (24px)

## 🔧 Tilpasninger

### Endre innhold

- **Konstanter**: `src/lib/constants.ts`
- **Tekster**: Direkte i komponentene
- **Bilder**: `public/images/`

### Endre styling

- **Globale stiler**: `src/styles/globals.css`
- **Tailwind config**: `tailwind.config.ts`
- **Komponenter**: Direkte i komponentene

### Endre navigasjon

- **Navigasjonsmeny**: `src/components/layout/navigation.tsx`
- **Side-struktur**: `src/app/page.tsx`

## 📊 SEO og Performance

### SEO-funksjoner

- Unike sidetitler, beskrivelser, canonical-URL-er og delingsbilder.
- `src/app/sitemap.ts` er eneste kilde for sitemap: 16 lokaliserte rutepaar (32 URL-er) samt innsiktsindeksen og tre norske fagartikler (36 URL-er totalt).
- `src/app/robots.ts` genererer robots.txt. Kontroller også at hosting ikke sender `X-Robots-Tag: noindex` til produksjon.
- Strukturerte data for virksomhet, nettsted, tjenester og brødsmuler må samsvare med synlig innhold. Ingen oppdiktede anmeldelser, resultater eller kundecaser.
- Tjenestene beskriver faktiske leveranser, samarbeid og kvalitetskontroll, med kontekstuelle interne lenker. Konsulentbistand har egen side; forsiden beholder seks tjenestekort.
- Spørsmål og svar er nyttig sideinnhold, men vi lover ikke FAQ-rich results. Google avviklet disse 7. mai 2026. Ingen særskilt AI-schema eller `llms.txt` er nødvendig for Google.

### Performance-optimalisering

- Next.js-bildeoptimalisering og lazy loading uten prioritering av bilder under første skjerm.
- Lokal variabel WOFF2-font: 54 320 byte, mot 167 160 byte for original TTF.
- Agenci lastes først etter aktivering, ikke ved første sidevisning.
- Mål en produksjonsbygging, ikke utviklingsserveren. Lighthouse er en laboratorietest, ikke feltdata for Core Web Vitals.

### Kontroll før og etter deploy

1. Kjør `npm run test -- --run`, `npm run type-check`, `npm run lint` og `npm run build`.
2. Start produksjonsbygget på en ledig port, for eksempel `npm start -- -p 3003`, og kjør `PLAYWRIGHT_PORT=3003 npm run test:e2e`. Ikke kjør `next dev` og bygg mot samme `.next`-mappe samtidig.
3. Kontakttesten dekker normalt e-postfallback. For å teste selve skjemaet: start en separat server med fiktive, formatgyldige leverandørinnstillinger og kjør med `PLAYWRIGHT_CONTACT_ENABLED=1`. Testen avskjærer `/api/contact`; backendtester mocker begge leverandørene. Bruk aldri ekte mottakere eller nøkler i automatiske tester.
4. Kontroller produksjonsdomenet `https://www.triodelab.no`, HTTPS og permanente omdirigeringer fra eventuelle alternative vertsnavn. Behold `/personvern`, `/cookies`, `/vilkar` og `/salgsvilkar`; ved senere URL-endringer brukes permanente 301/308-omdirigeringer.
5. Verifiser Search Console, send inn `/sitemap.xml`, inspiser viktige URL-er og test strukturerte data. Indeksering og rangering kan ikke garanteres.
6. Etter publisering: følg faktiske søk, klikk, relevante henvendelser og mobilfeltdata. Mål ved 75-persentilen: LCP ≤ 2,5 sekunder, INP ≤ 200 ms og CLS ≤ 0,1. Det kreves nok faktisk trafikk før feltdata er tilgjengelig.
7. Bruk Google Trends for Norge og Search Console til å prioritere nytt innhold om reelle kundebehov. Sammenlign for eksempel «KI», «AI», «automatisering» og «systemintegrasjon» over både 12 måneder og fem år. Trends er relativ interesse, ikke absolutt søkevolum. Ikke endre alle sidene etter kortvarige topper; publiser bare faglig innhold virksomheten kan stå inne for.

Kilder: [Googles AI-veiledning](https://developers.google.com/search/docs/fundamentals/ai-optimization-guide), [Search-oppdateringer](https://developers.google.com/search/updates#may-2026), [Core Web Vitals](https://developers.google.com/search/docs/appearance/core-web-vitals), [Google Trends-data](https://support.google.com/trends/answer/4365533).

### Avhengigheter og sikkerhetskontroll

Next.js er oppdatert til 15.5.28 og Sharp til 0.35.5. De avgrensede `overrides` i `package.json` sørger for oppdatert PostCSS under Next og source-map-js. Vitest og dekningstillegget er oppdatert sammen til 4.1.12; Vite 6.4.4 beholder kompatibilitet med React-pluginen. Se [Next.js-varselet](https://github.com/vercel/next.js/security/advisories/GHSA-2464-fhr4-w9xw), [Sharp-varselet](https://github.com/advisories/GHSA-wq5f-xc86-pv6w) og [Vitest-varselet](https://github.com/vitest-dev/vitest/security/advisories/GHSA-82fw-gwwq-j7x9).

Kontroll 8. oktober 2026: `npm audit --omit=dev` rapporterer null kjente sårbarheter. Full audit har fortsatt ti funn i utviklingsverktøyenes mønster-/CSS-parsere (åtte høye, to moderate), blant annet Tailwind 3-kjeden. De er ikke en bekreftet angrepsvei gjennom nettsiden, men skal følges opp separat; ikke bruk `npm audit fix --force` for å tvinge en Tailwind 4-migrering inn i en SEO-endring. Hold utviklings- og testservere private.

Kontaktmodulene kan kontrolleres med `npm run test -- --run --coverage --coverage.include='src/lib/contact/**' --coverage.include=src/features/contact/contact-sources.ts --coverage.thresholds.lines=80 --coverage.thresholds.functions=80 --coverage.thresholds.branches=80 --coverage.thresholds.statements=80`. Dette måler de nye kontaktmodulene, ikke full komponentdekning for hele nettstedet.

## 🚀 Deployment

### Vercel (Anbefalt)

1. Koble til GitHub-repositoriet
2. Vercel vil automatisk bygge og deploye
3. Miljøvariabler kan settes i Vercel-dashboard

### Andre plattformer

```bash
# Build
npm run build

# Deploy build-mappen til din hosting-provider
```

## 🤝 Bidrag

1. Fork repositoriet
2. Opprett en feature branch (`git checkout -b feature/amazing-feature`)
3. Commit endringene (`git commit -m 'Add amazing feature'`)
4. Push til branchen (`git push origin feature/amazing-feature`)
5. Opprett en Pull Request

## 📄 Lisens

Dette prosjektet er proprietært og tilhører TriodeLab.

## 📞 Kontakt

- **E-post**: post@triodelab.no
- **Telefon**: +47 988 46 460
- **Adresse**: Sigurd Hoels vei 114, 0655 Oslo

---

Utviklet med ❤️ av TriodeLab-teamet

Lokal oppdatering 29. oktober 2026: utviklingssamleren kan også bruke
`http://localhost:3001` når port 3000 er opptatt. Start med
`pnpm dev --port 3001` og angi samme `WEB_VITALS_REPORTING_ORIGIN` i den
beskyttede serverkonfigurasjonen; Integration Core må ha samsvarende
`developmentBackendOrigin`. Begge ender må fortsatt bruke gjeldende nettsted-ID
og revisjon. Andre porter og produksjon med lokal HTTP avvises.
