import type { StatusLevel } from "@/components/ui/StatusBadge";

/**
 * Trust Center content model.
 *
 * Every entry carries an honest `level`. The hard rule (VEREVON.md): Verevon
 * holds the controls but NOT the third-party certifications yet — so certs
 * render as Planlagt/Under arbeid, never as held. Nothing roadmap is dressed
 * up as live.
 */

export type TrustControl = {
	title: string;
	body: string;
	level: StatusLevel;
};

/**
 * The short procurement view of the detailed controls below. This is not a
 * document pack: it tells a buyer exactly which evidence is available today
 * and which evidence is still missing, so the detailed sections can be read
 * in the right light.
 */
export type TrustEvidenceItem = {
	title: string;
	level: StatusLevel;
	available: string;
	pending: string;
};

export const procurementEvidence: TrustEvidenceItem[] = [
	{
		title: "Sporing av AI-handlinger",
		level: "progress",
		available:
			"Modell-planet skriver tool_action-hendelser til en utboks, og Trust Center kan vise hendelser per tilkoblet app.",
		pending:
			"Leveringen til revisjonstjenesten er ikke verifisert ende til ende. Vi publiserer ikke et kontrollspor som bevis før det faktisk er levert og vist.",
	},
	{
		title: "Uavhengig sikkerhetstesting",
		level: "planned",
		available:
			"Rate-limiting, sikkerhetsheadere og påloggingskontroller er beskrevet som håndhevet i gateway og webserver.",
		pending:
			"En uavhengig pentest og et publiserbart sammendrag er planlagt. Vi har ingen rapport å dele ennå.",
	},
	{
		title: "DPA, overføringsvurdering og dataflyt",
		level: "progress",
		available:
			"Underleverandører, formål og oppgitt region er publisert nedenfor, med begrensningene for hver rute.",
		pending:
			"DPA, Schrems II-vurdering og dataflyt-diagram er under utarbeidelse og ikke gjennomgått av norsk personvernjurist.",
	},
	{
		title: "Datalagring og residens",
		level: "progress",
		available:
			"Modellruter og deres konfigurerte regioner er oppgitt per leverandør, inkludert ruter uten EU- eller ZDR-garanti.",
		pending:
			"Konfigurerte regioner er ikke uavhengig verifiserte behandlingssteder, og Verevons egne lagre publiseres ikke før de kan dokumenteres.",
	},
	{
		title: "Sletting og oppbevaring",
		level: "progress",
		available:
			"Sletting av brukerdata i Control Plane er bygget og testet mot en ekte database. Omfanget og unntakene er beskrevet nedenfor.",
		pending:
			"Sletting av hele brukeren og organisasjonen er ikke verifisert ende til ende. Samtaler og kjørehistorikk slettes ikke automatisk etter alder.",
	},
];

/**
 * The controls the page can stand behind, each worded no wider than the evidence in
 * docs/compliance/evidence/claims-vs-code.md and carrying its real level. The drift
 * test (trust-data.test.ts) pins the statements to trust-claims.ts.
 */
export const liveControls: TrustControl[] = [
	{
		title: "EU-region på standardmodellene",
		body: "Standardmodellene våre kjører hovedsakelig på Azure OpenAI med regionen satt til Sweden Central (EU/EØS). Regionen er en etikett vi selv setter og ennå ikke har bevist uavhengig, og ikke alle ruter har den: Claude-modellene går via Azure AI Foundry uten oppgitt region, dokument-embeddings via Cohere på Azure AI Foundry uten regionkontroll ved indeksering, og ChatGPT-modeller du selv kobler til via din egen OpenAI-konto har global behandling uten EU-garanti. Standardmodusene Balance og Genius sender komplekse turer til Claude. Modellvelgeren viser nivået for hver modell. Hver kjøring stemples med den konfigurerte regionen, en etikett som ikke følger leverandøren som faktisk svarte og ikke er et målt behandlingssted. Hvor Verevons egne lagre kjører, oppgir vi ikke her før vi kan dokumentere det.",
		level: "progress",
	},
	{
		title: "Zero Data Retention — valgfritt tillegg",
		body: "ZDR er valgfritt og av som standard. En organisasjon kan slå det på for alle forespørsler, men bare på Pro- eller Enterprise-plan; en bruker kan også velge «Midlertidig samtale» for én samtale. Når ZDR er på, hopper inferens-cachen over både lesing og skriving for merkede forespørsler (dekket av en test), og plattformen sender en ZDR-forespørsel bare til en leverandør vi har en registrert og datert ZDR-godkjenning for. Uten slik godkjenning avvises forespørselen i stedet for å sendes videre. Godkjenningen er vår egen registrering og er ikke uavhengig verifisert. I miljøet vi har gjennomgått er ingen slik godkjenning registrert, så en ZDR-forespørsel avvises der. ChatGPT-ruten støtter ikke ZDR, og en ZDR-forespørsel mot den avvises. ZDR gjelder modell-leverandøren, ikke Verevons egne lagre: Verevon beholder metadata og revisjonshendelser, og for arbeid uten ZDR beholdes samtaler og kjørehistorikk til noen sletter dem; de slettes ikke automatisk etter alder.",
		level: "progress",
	},
	{
		title: "Godkjenning per handling",
		body: "Utplasserte agenter stopper risikofylte handlinger (blant annet sending, publisering, betaling, utrulling og sletting, og alle eksterne MCP-verktøy) ved en Godkjenn/Avvis-port, mens lesende verktøy kjører uten stopp. Hva som regnes som risikofylt avgjøres av en regel på verktøynavn og operasjon. I vanlig interaktiv chat finnes ingen generell godkjenningsport; noen få verktøy venter likevel alltid på godkjenning.",
		level: "live",
	},
	{
		title: "«Brukt av AI?»-revisjonsspor",
		body: "Sporet er bygget: modell-planet legger en tool_action-hendelse i en utboks, revisjonstjenesten kan lese hendelser per kategori, og Trust Center i produktet viser dem per tilkoblet app. Det er ikke verifisert ende til ende. I miljøet vi har gjennomgått ble ingen av modell-planets hendelser levert til revisjonstjenesten (de ble liggende i utboksen fordi mottaksstrømmen manglet), så «Brukt av AI?» viser da at ingen hendelse kan knyttes til en tilkobling.",
		level: "progress",
	},
	{
		title: "Dataklassifisering",
		body: "En taksonomi i seks klasser merker data etter risiko. Data Plane nekter å sende ZDR-merket innhold til eksterne embeddings, og søk som krever suveren behandling holdes unna Azure-embedding og rerank. I chat brukes klassen ennå ikke til å velge modell-leverandør.",
		level: "progress",
	},
	{
		title: "Norskforankret grunnlag",
		body: "Norske virksomheter kan slås opp i Enhetsregisteret (Brreg) gjennom et eget verktøy. Bokmål-grensesnittet rulles ut gradvis.",
		level: "live",
	},
	{
		title: "SSO, 2FA & passkey",
		body: "Pålogging med SSO og 2FA-registrering er koblet gjennom gateway. Rate-limiting (token-bucket, 429 + Retry-After) og sikkerhetsheadere er satt opp i gateway og i webserverens konfigurasjon.",
		level: "live",
	},
	{
		title: "Rett til sletting (erasure)",
		body: "Sletting og anonymisering av en bruker er bygget i user-core og kalles via admin/eier-gatede, reviderte endepunkter; den fjerner kontoen, innloggingsdata, medlemskap og API-nøkler i Control Plane. Rutinen har tester mot en ekte database, men en hel brukersletting er ikke kjørt ende til ende i miljøet vi har gjennomgått. Den sletter ikke samtaler, minne, søkespor eller kostnadsrader i de andre planene: en bruker kan slette samtaler og minne selv mens vedkommende er innlogget, og sletting av en hel organisasjon er bygget for å fjerne organisasjonens kopier, men er ikke verifisert ende til ende.",
		level: "progress",
	},
	{
		title: "Søk: egen kjede først",
		body: "Søk går først gjennom en gratis kjede: egen indeks, Stract hvis konfigurert, og vår egen SearXNG-instans. SearXNG sender spørringen videre til eksterne søkemotorer (blant annet Google CSE og Bing), også for ZDR-forespørsler. Betalte leverandører (Brave) brukes bare når en forespørsel eksplisitt har fått lov til det, aldri for ZDR og aldri i agentkjøringer. De er en reserve når den gratis kjeden gir for lite, og kan brukes tidlig for noen korte søk. En bryter som slår av de betalte leverandørene finnes; en bryter som stopper all ekstern søketrafikk finnes ikke.",
		level: "progress",
	},
	{
		title: "Kundevendt DSAR / eksport",
		body: "En intern Art. 15-eksportvei finnes og dekker Control Plane-data (profil, medlemskap og API-nøkkelmetadata). Samtaler, minne og dokumenter i de andre planene er ikke med i eksporten ennå. Et kundevendt mottaks­endepunkt bygges nå (ikke GA ennå).",
		level: "progress",
	},
];

export type CertItem = {
	name: string;
	level: StatusLevel;
	plan: string;
};

/**
 * Trust assets & certifications. Honest: this public page is live, the in-app
 * Trust Center exists but its audit trail is not verified end to end, and all
 * third-party certs are roadmap. ISO 42001 + EU AI Act are the AI spearhead
 * (a choice of focus, not a claim of leadership).
 */
export const certifications: CertItem[] = [
	{
		name: "Offentlig Trust Center",
		level: "live",
		plan: "Du leser den nå — underleverandører, kontroller, dataflyt og kontakt med sikkerhets­teamet.",
	},
	{
		name: "In-app Trust Center («Brukt av AI?»)",
		level: "progress",
		plan: "Lever i produktet: tilkoblede apper, tilganger og om en AI-verktøyshendelse er knyttet til dem. Revisjonssporet fra modell-planet er ikke verifisert ende til ende (se «Brukt av AI?» ovenfor).",
	},
	{
		name: "ISO/IEC 42001 (AI-styringssystem)",
		level: "planned",
		plan: "Vår spydspiss: mer naturlig for en EU-basert AI-plattform enn for en amerikansk. Mål: den ISO-42001-klare norske agent-plattformen.",
	},
	{
		name: "EU AI Act-beredskap",
		level: "planned",
		plan: "Planlagt: risikovurdering og beredskaps­dokumentasjon. Vi har ingen uavhengig vurdering av hvor vi står, og lover ingen dato.",
	},
	{
		name: "SOC 2 Type II",
		level: "planned",
		plan: "Åpne observasjonsvinduet; kontinuerlige kontroller.",
	},
	{
		name: "ISO 27001",
		level: "planned",
		plan: "ISMS avgrenset til plattformen.",
	},
	{
		name: "Uavhengig penetrasjonstest",
		level: "planned",
		plan: "Web-app/API-pentest med publisert sammendrag.",
	},
];

export type Subprocessor = {
	name: string;
	purpose: string;
	region: string;
	level: StatusLevel;
	note: string;
	/**
	 * Ids of the vendors in the egress scan (docs/compliance/evidence/tools/scan-egress.mjs)
	 * that this row covers. trust-data.test.ts fails when the scan finds a vendor that is
	 * neither on a row here nor recorded in trust-recipients.ts with a reason.
	 */
	vendorIds: string[];
};

export const subprocessors: Subprocessor[] = [
	{
		name: "Microsoft Azure — Azure OpenAI og Azure AI-tjenester",
		purpose: "Inferens for standardmodellene; tale (TTS og STT), oversettelse, dokumentanalyse og språkanalyse; oppsummeringer og entitetsuttrekk fra dokumentutdrag ved indeksering, når de er slått på",
		region: "Konfigurert: Sweden Central (en etikett, ikke uavhengig verifisert)",
		level: "progress",
		note: "Distribusjonstypen (regional eller global) er ikke satt i konfigurasjonen vi har lest, så EU-grensen er ikke bevist. ZDR krever en registrert godkjenning, og ingen er registrert i miljøet vi har lest. Tale bruker Azure Speech (regionen satt til Sweden Central) og Azure OpenAI; produksjonsoppsettet er ikke uavhengig bekreftet.",
		vendorIds: ["microsoft-azure-ai"],
	},
	{
		name: "Microsoft Azure AI Foundry — Claude og Cohere",
		purpose: "Claude-modeller for komplekse samtaleturer og verktøyturer; Cohere Command-modellen for samtaler, når den velges; Cohere Embed v4 for embeddings av dokumenter og søk, samt rerank",
		region: "Ikke oppgitt for Claude og Cohere Embed. Cohere Command: satt til Sweden Central (en etikett, ikke uavhengig verifisert)",
		level: "progress",
		note: "Egne Azure-ressurser som ikke dekkes av regionen for Azure OpenAI. Embeddings av dokumenter går direkte hit uten regionkontroll ved indeksering; ZDR-merket innhold avvises.",
		vendorIds: ["microsoft-azure-ai"],
	},
	{
		name: "Anthropic (direkte API)",
		purpose: "Claude via api.anthropic.com, bare hvis Azure AI Foundry ikke er konfigurert",
		region: "Under avklaring",
		level: "progress",
		note: "Region- og ZDR-status under avklaring (TBC).",
		vendorIds: ["anthropic-api"],
	},
	{
		name: "OpenAI (direkte API)",
		purpose: "Reservemodell; registreres bare hvis en API-nøkkel er satt (ikke satt i miljøet vi har lest)",
		region: "Under avklaring",
		level: "progress",
		note: "Region- og ZDR-status under avklaring (TBC).",
		vendorIds: ["openai-api"],
	},
	{
		name: "OpenAI — brukerens egen ChatGPT-konto",
		purpose: "Valgfri modellrute: en bruker kobler til sin egen ChatGPT-konto og velger en ChatGPT-modell. Ruten brukes bare for den brukeren, og bare når en slik modell er valgt.",
		region: "Global, ingen regionsforpliktelse",
		level: "progress",
		note: "Støtter ikke ZDR, og en ZDR-forespørsel mot ruten avvises. Hele prompten (systemtekst, samtalehistorikk, minneblokk, hentede kilder og verktøyresultater) sendes til OpenAI under brukerens egen konto og vilkår, og det kan skje i flere runder per svar. Modellvelgeren viser nivået «Global».",
		vendorIds: ["openai-chatgpt-subscription"],
	},
	{
		name: "Eksterne søkemotorer via vår SearXNG, og Brave og Serper (valgfrie)",
		purpose: "Nett-søk: spørringen sendes videre til eksterne søkemotorer (blant annet Google CSE og Bing); Brave og Serper bare som betalt reserve, når en forespørsel har fått lov til det",
		region: "Ekstern, ikke oppgitt",
		level: "progress",
		note: "Gjelder også ZDR-forespørsler for den gratis kjeden. Betalt søk brukes aldri for ZDR og aldri i agentkjøringer.",
		vendorIds: ["searxng-upstream-engines", "brave-search", "serper-search"],
	},
	{
		name: "Fraktselskaper: Bring, DHL, FedEx og UPS",
		purpose: "Fraktbooking, prisoverslag og sporing når fraktfunksjonen brukes: avsender, mottaker og pakkedata sendes til fraktselskapet",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Hvilke av dem som bruker live-kontoer og ikke testkontoer, er ikke dokumentert her.",
		vendorIds: ["bring-carrier", "dhl-express", "fedex-carrier", "ups-carrier"],
	},
	{
		name: "Stripe",
		purpose: "Fakturering: e-post, organisasjonsnavn og -id som kundemetadata, og parametere for betalingsøkten. Kortopplysninger går til Stripes egen betalingsside.",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Verevons egen faktureringskonto.",
		vendorIds: ["stripe-billing"],
	},
	{
		name: "Nexi (Dibs)",
		purpose: "Betaling i kassen: ordrelinjer, beløp og valuta; nettleseren laster Nexis kasseskript",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Brukes bare der kassen er satt opp med Nexi.",
		vendorIds: ["nexi-checkout"],
	},
	{
		name: "Hyperswitch",
		purpose: "Betalingsorkestrering fra faktureringstjenesten: beløp, valuta og metadata",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Brukes bare hvis en API-nøkkel er satt (ikke satt i miljøet vi har lest).",
		vendorIds: ["hyperswitch"],
	},
	{
		name: "Resend",
		purpose: "E-post ved innlogging, verifisering og tilbakestilling av passord: mottakeradressen og selve meldingen",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Region og databehandleravtale er ikke bekreftet.",
		vendorIds: ["resend"],
	},
	{
		name: "Twilio Verify",
		purpose: "SMS-engangskoder: telefonnummeret sendes til Twilio, som lager og sender koden",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Region og databehandleravtale er ikke bekreftet.",
		vendorIds: ["twilio-verify"],
	},
	{
		name: "Cloudflare Turnstile",
		purpose: "Menneskesjekk ved registrering: nettleseren laster Turnstile-skriptet, og innloggingstjenesten verifiserer svaret. Besøkendes IP-adresse og nettlesersignaler når Cloudflare.",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Brukes bare når en hemmelig nøkkel er satt.",
		vendorIds: ["cloudflare-turnstile"],
	},
	{
		name: "Google: skrifter og favicon-tjeneste",
		purpose: "Nettleseren laster skrifter fra Google Fonts, og appen ber Googles favicon-tjeneste om ikonet til hvert søketreff (bare vertsnavnet). Besøkendes IP-adresse og nettleser følger med.",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Ingen kundeinnhold.",
		vendorIds: ["google-fonts", "google-search-and-favicons"],
	},
	{
		name: "Cohere (offentlig API)",
		purpose: "Reserve for rerank: spørringen og kandidatutdragene sendes hit bare hvis rerank-endepunktet ikke er satt",
		region: "Under avklaring",
		level: "progress",
		note: "I miljøet vi har gjennomgått er endepunktet satt til Azure AI Foundry, så det offentlige API-et brukes ikke.",
		vendorIds: ["cohere-api"],
	},
	{
		name: "Browserbase (hostet nettleser)",
		purpose: "Navigasjon og sideinnhold i en hostet nettleser, bare hvis Browserbase-nøkler er satt",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Av som standard. Vår egen konfigurasjon regner leverandøren som tredjepartsbehandling som ikke er egnet for styrte kjøringer.",
		vendorIds: ["browserbase"],
	},
	{
		name: "Proxy-leverandør for nettleserhenting (navngis ikke i koden)",
		purpose: "Henting av nettsider kan rutes gjennom proxyer som operatøren oppgir; trafikken til målsidene går da via leverandøren",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Listen er tom som standard, så henting går direkte.",
		vendorIds: ["quarry-proxy-pool"],
	},
	{
		name: "Postmark (e-post til oppgave)",
		purpose: "Hvis e-post-til-oppgave slås på, passerer videresendt e-post (avsender, emne, innhold, vedlegg og autentiseringshoder) Postmark før den når vår webhook",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Av så lenge leverandør og hemmelighet ikke er satt. Region og databehandleravtale er ikke bekreftet.",
		vendorIds: ["postmark"],
	},
	{
		name: "Novu (varsler)",
		purpose: "Utsending av varsler: nyttelast for varslingsarbeidsflyter og abonnentens identitet",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Levering er av som standard.",
		vendorIds: ["novu"],
	},
	{
		name: "Letta (verktøysøk)",
		purpose: "Søk i verktøydefinisjoner hos et eksternt endepunkt operatøren oppgir: en søkestreng på opptil 4096 byte",
		region: "Ikke oppgitt",
		level: "progress",
		note: "Av som standard.",
		vendorIds: ["letta-tool-search"],
	},
];

export const dataClasses: string[] = [
	"Offentlig, ikke-personlig",
	"Kundeintern",
	"Personopplysning",
	"Sensitiv personopplysning",
	"Legitimasjon / hemmelighet",
	"ZDR-flyktig (tillegg)",
];
