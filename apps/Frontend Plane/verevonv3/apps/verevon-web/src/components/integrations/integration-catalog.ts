export const INTEGRATION_CATEGORIES = [
	"commerce",
	"collaboration",
	"customer",
	"microsoft",
	"public-data",
	"finance",
	"developer",
	"knowledge",
] as const;

export type IntegrationCategory = (typeof INTEGRATION_CATEGORIES)[number];

export type IntegrationAvailability = "available" | "public-source" | "pilot" | "planned" | "system-example";

export type Integration = {
	id: string;
	name: string;
	category: IntegrationCategory;
	provides: string;
	logo?: string;
	mark?: string;
	scope: "arbeidsdata" | "dokumenter" | "kunde og marked" | "økonomi og drift" | "offentlige data" | "teknisk kontekst";
	availability: IntegrationAvailability;
};

const directlyConnectedIds = new Set(["slack", "gmail", "notion", "microsoft-sharepoint", "outlook"]);
const publicSourceIds = new Set(["enhetsregisteret"]);
const pilotIds = new Set(["visma-net"]);
const plannedIds = new Set(["altinn", "zendesk"]);

function availabilityFor(id: string): IntegrationAvailability {
	if (directlyConnectedIds.has(id)) return "available";
	if (publicSourceIds.has(id)) return "public-source";
	if (pilotIds.has(id)) return "pilot";
	if (plannedIds.has(id)) return "planned";
	return "system-example";
}

// The text marks are deliberately typographic until licensed source artwork is
// available. They keep this directory accurate without recreating vendor logos.
const integrationCatalogueEntries: readonly Omit<Integration, "availability">[] = [
	{ id: "shopify", name: "Shopify", category: "commerce", provides: "Ordre, produkter, kunder og lagerstatus fra nettbutikken.", logo: "shopify.svg", scope: "kunde og marked" },
	{ id: "woocommerce", name: "WooCommerce", category: "commerce", provides: "Ordre, produktkatalog og kundeaktivitet fra WordPress-butikken.", mark: "woo", scope: "kunde og marked" },
	{ id: "meta", name: "Meta", category: "commerce", provides: "Kampanjer, annonser og henvendelser fra Meta-kanalene.", logo: "meta.svg", scope: "kunde og marked" },
	{ id: "aftership", name: "AfterShip", category: "commerce", provides: "Sporing, leveringsstatus og avvik i forsendelser.", mark: "aftership", scope: "økonomi og drift" },
	{ id: "helthjem", name: "Helthjem", category: "commerce", provides: "Leveringsoppdateringer og distribusjonsstatus i Norge.", mark: "helthjem", scope: "økonomi og drift" },
	{ id: "postnord", name: "PostNord", category: "commerce", provides: "Forsendelser, sporingshendelser og leveringsavvik.", mark: "postnord", scope: "økonomi og drift" },
	{ id: "packiyo", name: "Packiyo", category: "commerce", provides: "Plukk, pakking, lagerbevegelser og ordrestatus.", mark: "packiyo", scope: "økonomi og drift" },
	{ id: "shiphero", name: "ShipHero", category: "commerce", provides: "Lageroperasjoner, ordre og fulfillment-status.", logo: "shiphero.svg", scope: "økonomi og drift" },
	{ id: "bring", name: "Bring", category: "commerce", provides: "Forsendelser, sporing og leveringshendelser.", logo: "bring.svg", scope: "økonomi og drift" },
	{ id: "ingrid", name: "Ingrid", category: "commerce", provides: "Valg av levering, utsendinger og kundeopplevelse etter kjøp.", mark: "ingrid", scope: "kunde og marked" },
	{ id: "centra", name: "Centra", category: "commerce", provides: "Produkter, ordre og handelsdata for merkevarer.", mark: "centra", scope: "kunde og marked" },
	{ id: "magento", name: "Magento", category: "commerce", provides: "Produktkatalog, ordre og kundedata fra nettbutikken.", mark: "magento", scope: "kunde og marked" },
	{ id: "omnium", name: "Omnium", category: "commerce", provides: "Ordreorkestrering og lagerstatus på tvers av kanaler.", mark: "omnium", scope: "økonomi og drift" },
	{ id: "viskan", name: "Viskan", category: "commerce", provides: "Handelsdata, ordre og kundereiser i nettbutikk.", mark: "viskan", scope: "kunde og marked" },
	{ id: "quickbutik", name: "Quickbutik", category: "commerce", provides: "Produktsortiment, ordre og kundeinformasjon.", mark: "quickbutik", scope: "kunde og marked" },
	{ id: "dpd", name: "DPD", category: "commerce", provides: "Leveringsløp, sporingsstatus og avvik i transport.", mark: "dpd", scope: "økonomi og drift" },
	{ id: "ongoing-wms", name: "Ongoing WMS", category: "commerce", provides: "Lagerbeholdning, plukk og leveringsklare ordre.", mark: "ongoing", scope: "økonomi og drift" },

	{ id: "slack", name: "Slack", category: "collaboration", provides: "Samtaler, kanaler og arbeidsoppfølging i teamet.", logo: "slack.svg", scope: "arbeidsdata" },
	{ id: "notion", name: "Notion", category: "collaboration", provides: "Sider, arbeidsrom og teamdokumentasjon.", logo: "notion.svg", scope: "dokumenter" },
	{ id: "gmail", name: "Gmail", category: "collaboration", provides: "E-posttråder og avtalt oppfølging fra innboksen.", logo: "gmail.svg", scope: "arbeidsdata" },
	{ id: "outlook", name: "Outlook", category: "collaboration", provides: "E-post, kalenderkontekst og oppfølgingspunkter.", logo: "outlook.svg", scope: "arbeidsdata" },
	{ id: "trello", name: "Trello", category: "collaboration", provides: "Kort, lister, ansvar og fremdrift i oppgaver.", mark: "trello", scope: "arbeidsdata" },
	{ id: "google-calendar", name: "Google Calendar", category: "collaboration", provides: "Kalenderavtaler, deltakere og planlagt arbeid.", mark: "calendar", scope: "arbeidsdata" },
	{ id: "mailchimp", name: "Mailchimp", category: "customer", provides: "Kampanjer, målgrupper og respons fra e-postkommunikasjon.", mark: "mailchimp", scope: "kunde og marked" },
	{ id: "gorgias", name: "Gorgias", category: "customer", provides: "Kundesaker, ordrehenvendelser og supporthistorikk.", mark: "gorgias", scope: "kunde og marked" },
	{ id: "hubspot", name: "HubSpot", category: "customer", provides: "Kontakter, selskaper, avtaler og salgsaktivitet.", mark: "hubspot", scope: "kunde og marked" },
	{ id: "zendesk", name: "Zendesk", category: "customer", provides: "Supporthenvendelser, saker og kundehistorikk.", mark: "zendesk", scope: "kunde og marked" },
	{ id: "superoffice", name: "SuperOffice", category: "customer", provides: "Kunder, relasjoner, aktivitet og salgsoppfølging.", logo: "superoffice.svg", scope: "kunde og marked" },
	{ id: "prospect-crm", name: "Prospect CRM", category: "customer", provides: "Kundeemner, relasjoner og salgsarbeid.", mark: "prospect", scope: "kunde og marked" },
	{ id: "flowcase", name: "Flowcase", category: "customer", provides: "Kompetanseprofiler, CV-data og tilbudsgrunnlag.", mark: "flowcase", scope: "kunde og marked" },
	{ id: "brandmaster", name: "Brandmaster", category: "customer", provides: "Godkjente merkevarefiler, kampanjemateriell og metadata.", mark: "brandmaster", scope: "dokumenter" },

	{ id: "azure", name: "Azure", category: "microsoft", provides: "Godkjente tjenester, dataflyt og driftssammenheng i Azure.", mark: "azure", scope: "teknisk kontekst" },
	{ id: "azure-ad", name: "Azure AD", category: "microsoft", provides: "Identitet, grupper og tilgangskontekst.", mark: "azure ad", scope: "teknisk kontekst" },
	{ id: "microsoft-365", name: "Microsoft 365", category: "microsoft", provides: "Felles arbeidsflate for dokumenter, e-post og samarbeid.", logo: "microsoft-365.svg", scope: "arbeidsdata" },
	{ id: "microsoft-dynamics", name: "Microsoft Dynamics", category: "microsoft", provides: "Forretningsprosesser, kunder, ordre og driftsdata.", mark: "dynamics", scope: "økonomi og drift" },
	{ id: "microsoft-sharepoint", name: "Microsoft SharePoint", category: "microsoft", provides: "Dokumenter, intranettinnhold og felles arbeidsområder.", logo: "sharepoint.svg", scope: "dokumenter" },
	{ id: "microsoft-exchange", name: "Microsoft Exchange", category: "microsoft", provides: "E-post, kalendere og organisasjonskontekst.", mark: "exchange", scope: "arbeidsdata" },
	{ id: "microsoft-teams", name: "Microsoft Teams", category: "microsoft", provides: "Møter, samtaler, kanaler og oppfølgingspunkter.", logo: "teams.svg", scope: "arbeidsdata" },

	{ id: "lovdata", name: "Lovdata", category: "public-data", provides: "Lov- og regelgrunnlag som kan knyttes til saken dere arbeider med.", mark: "lovdata", scope: "offentlige data" },
	{ id: "doffin", name: "Doffin", category: "public-data", provides: "Kunngjøringer og anbudsgrunnlag fra offentlige anskaffelser.", mark: "doffin", scope: "offentlige data" },
	{ id: "altinn", name: "Altinn", category: "public-data", provides: "Godkjente skjema, meldinger og virksomhetsdialog der tilgangen er avgrenset.", mark: "altinn", scope: "offentlige data" },
	{ id: "ssb", name: "SSB", category: "public-data", provides: "Offisiell statistikk, tabeller og dokumentert datagrunnlag.", mark: "ssb", scope: "offentlige data" },
	{ id: "bronnoysundregistrene", name: "Brønnøysundregistrene", category: "public-data", provides: "Registertjenester og virksomhetsopplysninger som kan avgrenses til en konkret oppgave.", mark: "brreg", scope: "offentlige data" },
	{ id: "enhetsregisteret", name: "Enhetsregisteret", category: "public-data", provides: "Virksomhetsopplysninger fra et kontrollert registeroppslag når en sak trenger kontroll.", mark: "brreg", scope: "offentlige data" },
	{ id: "sentrale-registre", name: "Sentrale registre", category: "public-data", provides: "Strukturerte registeropplysninger for kontroll og berikelse.", mark: "registre", scope: "offentlige data" },
	{ id: "scb", name: "SCB", category: "public-data", provides: "Svensk offisiell statistikk og sammenlignbare nordiske data.", mark: "scb", scope: "offentlige data" },
	{ id: "sveve", name: "Sveve", category: "public-data", provides: "SMS-kommunikasjon og leveringsstatus ved varsling.", mark: "sveve", scope: "arbeidsdata" },

	{ id: "tripletex", name: "Tripletex", category: "finance", provides: "Regnskap, fakturaer, prosjekter og økonomisk status.", mark: "tripletex", scope: "økonomi og drift" },
	{ id: "visma-net", name: "Visma Net", category: "finance", provides: "Økonomi, leverandører, kunder og driftsoppfølging.", mark: "visma net", scope: "økonomi og drift" },
	{ id: "huma", name: "Huma", category: "finance", provides: "Ansattdata, roller og HR-prosesser.", mark: "huma", scope: "økonomi og drift" },
	{ id: "xledger", name: "Xledger", category: "finance", provides: "Regnskap, prosjektøkonomi og økonomirapportering.", mark: "xledger", scope: "økonomi og drift" },
	{ id: "poweroffice", name: "PowerOffice", category: "finance", provides: "Bilag, fakturaer, prosjekt og regnskapsstatus.", mark: "poweroffice", scope: "økonomi og drift" },
	{ id: "fiken", name: "Fiken", category: "finance", provides: "Fakturering, utgifter og enkelt regnskapsgrunnlag.", logo: "fiken.svg", scope: "økonomi og drift" },
	{ id: "visma-business", name: "Visma Business", category: "finance", provides: "ERP-data, ordre, økonomi og forretningsprosesser.", mark: "visma business", scope: "økonomi og drift" },
	{ id: "maconomy", name: "Maconomy", category: "finance", provides: "Prosjektøkonomi, ressursplanlegging og virksomhetsstyring.", mark: "maconomy", scope: "økonomi og drift" },
	{ id: "xero", name: "Xero", category: "finance", provides: "Regnskapsdata, fakturaer og økonomisk oversikt.", mark: "xero", scope: "økonomi og drift" },
	{ id: "odoo", name: "Odoo", category: "finance", provides: "ERP, lager, salg og driftsprosesser i én forretningsmodell.", mark: "odoo", scope: "økonomi og drift" },
	{ id: "simployer", name: "Simployer", category: "finance", provides: "HR-data, retningslinjer og personalprosesser.", mark: "simployer", scope: "økonomi og drift" },

	{ id: "jira", name: "Jira", category: "developer", provides: "Saker, ansvar, fremdrift og tekniske beslutninger.", mark: "jira", scope: "teknisk kontekst" },
	{ id: "zapier", name: "Zapier", category: "developer", provides: "Automatiserte hendelser og koblinger mellom godkjente systemer.", mark: "zapier", scope: "teknisk kontekst" },
	{ id: "gitlab", name: "GitLab", category: "developer", provides: "Kode, endringer, saker og dokumentasjon for produktarbeid.", mark: "gitlab", scope: "teknisk kontekst" },
	{ id: "google-cloud-portal", name: "Google Cloud Portal", category: "developer", provides: "Prosjekt-, drifts- og tjenestekontekst fra Google Cloud.", mark: "google cloud", scope: "teknisk kontekst" },
	{ id: "confluence", name: "Confluence", category: "developer", provides: "Teknisk dokumentasjon, beslutninger og teamkunnskap.", mark: "confluence", scope: "dokumenter" },
	{ id: "generic-sql", name: "Generic SQL", category: "developer", provides: "Godkjente strukturerte data fra virksomhetens egne databaser.", mark: "sql", scope: "teknisk kontekst" },
	{ id: "webconnector", name: "Webconnector", category: "developer", provides: "Avtalte nettbaserte datakilder og kontrollerte arbeidsflyter.", mark: "web", scope: "teknisk kontekst" },

	{ id: "google-drive", name: "Google Drive", category: "knowledge", provides: "Delte dokumenter, mapper og arbeidsmateriale.", logo: "google.svg", scope: "dokumenter" },
	{ id: "dropbox", name: "Dropbox", category: "knowledge", provides: "Filer, mapper og delt dokumentasjon.", mark: "dropbox", scope: "dokumenter" },
	{ id: "documaster", name: "Documaster", category: "knowledge", provides: "Journalførte dokumenter, metadata og arkivkontekst.", mark: "documaster", scope: "dokumenter" },
	{ id: "acos-websak", name: "Acos WebSak", category: "knowledge", provides: "Saker, journalposter og dokumenter fra sak- og arkivarbeid.", mark: "acos", scope: "dokumenter" },
	{ id: "aris", name: "Aris", category: "knowledge", provides: "Prosessmodeller, rutiner og virksomhetsarkitektur.", mark: "aris", scope: "dokumenter" },
	{ id: "corepublish", name: "CorePublish", category: "knowledge", provides: "Publisert innhold, redaksjonelle sider og dokumenter.", mark: "corepublish", scope: "dokumenter" },
	{ id: "edocs", name: "eDocs", category: "knowledge", provides: "Dokumenter, metadata og saksrelatert informasjon.", mark: "edocs", scope: "dokumenter" },
	{ id: "ephorte", name: "ePhorte", category: "knowledge", provides: "Journal, saker og dokumentasjon fra offentlig saksbehandling.", mark: "ephorte", scope: "dokumenter" },
	{ id: "fileserver", name: "FileServer", category: "knowledge", provides: "Avtalte filområder, dokumenter og katalogstruktur.", mark: "fileserver", scope: "dokumenter" },
	{ id: "imanage", name: "iManage", category: "knowledge", provides: "Arbeidsdokumenter, klientmapper og kunnskapsinnhold.", mark: "imanage", scope: "dokumenter" },
	{ id: "mailstore", name: "MailStore", category: "knowledge", provides: "Arkivert e-post og søkbar kommunikasjonshistorikk.", mark: "mailstore", scope: "dokumenter" },
	{ id: "netdocuments", name: "NetDocuments", category: "knowledge", provides: "Dokumentstyring, versjoner og kontrollerte arbeidsflater.", mark: "netdocuments", scope: "dokumenter" },
	{ id: "p360", name: "P360", category: "knowledge", provides: "Saker, journalposter og dokumentert saksgrunnlag.", mark: "p360", scope: "dokumenter" },
	{ id: "qualiware", name: "Qualiware", category: "knowledge", provides: "Styringssystem, prosesser og kvalitetssikret dokumentasjon.", mark: "qualiware", scope: "dokumenter" },
	{ id: "worksite", name: "Worksite", category: "knowledge", provides: "Saker, dokumenter og kontekst fra profesjonelt arbeid.", mark: "worksite", scope: "dokumenter" },
];

export const INTEGRATION_CATALOG: readonly Integration[] = integrationCatalogueEntries.map((integration) => ({
	...integration,
	availability: availabilityFor(integration.id),
}));

function integrationsById(ids: readonly string[]): readonly Integration[] {
	return ids.map((id) => {
		const integration = INTEGRATION_CATALOG.find((item) => item.id === id);
		if (!integration) throw new Error(`Unknown integration: ${id}`);
		return integration;
	});
}

export const DEFAULT_CARD_INTEGRATIONS = integrationsById([
	"slack", "shopify", "meta", "notion", "hubspot", "microsoft-sharepoint", "outlook", "microsoft-teams", "microsoft-365", "google-drive", "gmail", "jira",
]);

export const NORWEGIAN_CARD_INTEGRATIONS = integrationsById([
	"lovdata", "doffin", "altinn", "ssb", "bronnoysundregistrene", "sveve", "bring", "postnord", "helthjem", "ingrid", "visma-net", "visma-business", "tripletex", "huma", "xledger", "poweroffice", "fiken", "superoffice",
]);
