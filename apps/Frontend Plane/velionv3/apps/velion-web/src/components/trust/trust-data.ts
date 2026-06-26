import type { StatusLevel } from "@/components/ui/StatusBadge";

/**
 * Trust Center content model.
 *
 * Every entry carries an honest `level`. The hard rule (VELION.md): Velion
 * holds the controls but NOT the third-party certifications yet — so certs
 * render as Planlagt/Under arbeid, never as held. Nothing roadmap is dressed
 * up as live.
 */

export type TrustControl = {
	title: string;
	body: string;
	level: StatusLevel;
};

/** Defensible, mostly-live differentiators — the trust wedge that is real. */
export const liveControls: TrustControl[] = [
	{
		title: "EU-residens som standard",
		body: "Primær inferens og embeddings kjører på Azure OpenAI Sweden Central (EU/EØS). Hver kjøring stemples med en residens-verdi (default swedencentral). App-data, samtaler, kjørehistorikk, revisjon og vektorlageret ligger i EU/EØS.",
		level: "live",
	},
	{
		title: "Zero Data Retention på modell-laget",
		body: "ZDR er håndhevet i koden: inferens-cachen kortslutter både lesing og skriving når forespørselen er merket ZDR — den treffer aldri den varige prompt-cachen. Dekket av en regresjonstest. ZDR binder modell-leverandøren; Velion beholder egen historikk etter sin sletteplan.",
		level: "live",
	},
	{
		title: "Godkjenning per handling",
		body: "Lese-verktøy kjører ugatet; risikofylte handlinger (utrulling, betaling, refusjon, shell, publisering, sletting, alle MCP-verktøy) stopper ved en Godkjenn/Avvis-port. Mennesket sier ja før noe utføres.",
		level: "live",
	},
	{
		title: "«Brukt av AI?»-revisjonsspor",
		body: "Revisjonstjenesten abonnerer på modell-planet på tvers av planene. Et oppslag på tool_action returnerer hendelse, plan og detaljer (datakategori, ZDR, verktøy) — per kategori, ikke en generisk «innsyn»-flis.",
		level: "live",
	},
	{
		title: "Dataklassifisering",
		body: "Et taksonomi i seks klasser styrer lagring, indeksering og deling, med default-deny tredjeparts­behandling for beskyttede klasser.",
		level: "live",
	},
	{
		title: "Norskforankret grunnlag",
		body: "Entiteter forankres i Enhetsregisteret (Brreg) — live-verifisert mot ekte register, ikke en hallusinert global oppføring. Bokmål-grensesnittet rulles ut gradvis.",
		level: "live",
	},
	{
		title: "SSO, 2FA & passkey",
		body: "Pålogging med SSO og 2FA-registrering er koblet gjennom gateway. Rate-limiting (token-bucket, 429 + Retry-After) og sikkerhetsheadere er aktive.",
		level: "live",
	},
	{
		title: "Rett til sletting (erasure)",
		body: "Primitiver for hard sletting og anonymisering er live i user-core og kalles via admin/eier-gatede, reviderte endepunkter. Sletting av avledede kopier på tvers av planene er under arbeid.",
		level: "progress",
	},
	{
		title: "Søk i egen infrastruktur",
		body: "Søk forsøker en gratis in-infra-kjede først (Tantivy → Stract → SearXNG) og faller bare tilbake til betalte eksterne søk som siste utvei. Utelat nøklene, og du får null ekstern trafikk. En hard, policy-håndhevet «0-SaaS»-bryter er på veikartet.",
		level: "progress",
	},
	{
		title: "Kundevendt DSAR / eksport",
		body: "En intern Art. 15-eksportvei finnes; et kundevendt mottaks­endepunkt bygges nå (ikke GA ennå).",
		level: "progress",
	},
];

export type CertItem = {
	name: string;
	level: StatusLevel;
	plan: string;
};

/**
 * Trust assets & certifications. Honest: in-app Trust Center is live and this
 * public page is live; all third-party certs are roadmap. Lead on ISO 42001 +
 * EU AI Act as the sovereign-AI wedge.
 */
export const certifications: CertItem[] = [
	{
		name: "Offentlig Trust Center",
		level: "live",
		plan: "Du leser den nå — underleverandører, kontroller, dataflyt og tilgangs­forespørsel.",
	},
	{
		name: "In-app Trust Center («Brukt av AI?»)",
		level: "live",
		plan: "Lever i produktet, matet fra revisjonssporet på tvers av planene.",
	},
	{
		name: "ISO/IEC 42001 (AI-styringssystem)",
		level: "planned",
		plan: "Vår spydspiss: mer naturlig for en suveren EU-AI-plattform enn for en amerikansk. Mål: den ISO-42001-klare norske agent-plattformen.",
	},
	{
		name: "EU AI Act-beredskap",
		level: "planned",
		plan: "Risikovurdering og beredskaps­dokumentasjon på plass før kravene trer i kraft.",
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
};

export const subprocessors: Subprocessor[] = [
	{
		name: "Microsoft Azure — Azure OpenAI",
		purpose: "Inferens, embeddings og tale (TTS på EU-endepunkt)",
		region: "Sweden Central (EU/EØS)",
		level: "live",
		note: "ZDR + EØS bekreftet på denne stien i dag.",
	},
	{
		name: "Anthropic",
		purpose: "Reserve modell-leverandør (fallback)",
		region: "Under avklaring",
		level: "progress",
		note: "Region- og ZDR-status under avklaring (TBC).",
	},
	{
		name: "OpenAI",
		purpose: "Reserve modell-leverandør (fallback)",
		region: "Under avklaring",
		level: "progress",
		note: "Region- og ZDR-status under avklaring (TBC).",
	},
	{
		name: "Eksterne søke-leverandører (Brave / Serper)",
		purpose: "Valgfritt nett-søk, kun ved fallback",
		region: "Ekstern",
		level: "progress",
		note: "Av som standard via in-infra-rekkefølge; utelat nøklene for null egress.",
	},
];

export const dataClasses: string[] = [
	"Offentlig, ikke-personlig",
	"Kundeintern",
	"Personopplysning",
	"Sensitiv personopplysning",
	"Legitimasjon / hemmelighet",
	"ZDR-flyktig",
];
