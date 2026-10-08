/**
 * The evidence-backed claims the public Trust Center and its neighbours may make, as data.
 *
 * Each entry ties wording on the site to a row of docs/compliance/evidence/claims-vs-code.md and to
 * the code that row rests on. trust-data.test.ts reads this list and fails when
 *
 *   - a sentence in `mustSay` is gone from the surface it is listed for (a narrowing was undone or
 *     reworded without a new reading of the evidence),
 *   - a phrase in `mustNotSay` is back on any surface (a contradicted claim returned),
 *   - an `evidence` anchor no longer appears in its file (the code the claim rests on changed:
 *     re-read the code, then update the sentence or the anchor),
 *   - a claim marked `pending` is missing from docs/compliance/evidence/trust-claims-pending-reverification.md.
 *
 * Text is matched after normalisation: soft hyphens removed, non-breaking spaces and runs of
 * whitespace collapsed to one space. A surface is one of
 *
 *   control:<title>          a `liveControls` entry (title and body)
 *   subprocessor:<name>      a `subprocessors` row (all its fields)
 *   certification:<name>     a `certifications` entry
 *   file:<name>              a source file listed in SITE_FILES in trust-data.test.ts
 *
 * Keep it small: it holds the factual statements a reader could rely on, not every sentence.
 */

/** A build item that will change what a claim says. See trust-claims-pending-reverification.md. */
export type PendingItem =
	| "a: contextual retrieval rework (metering, ZDR handling, backfill)"
	| "b: Data Plane rerank capability for Quarry"
	| "c: per-connection zero-retention (ZDR) consent flow";

export type Surface =
	| `control:${string}`
	| `subprocessor:${string}`
	| `certification:${string}`
	| `file:${string}`;

export type TrustClaim = {
	/** Short stable id. */
	id: string;
	/** Rows of claims-vs-code.md that the claim answers (T1a, C6, P5 and so on). */
	rows: string[];
	/** What the page may say, in one plain English sentence. */
	statement: string;
	/** Set when unbuilt work will change the claim; the final pass re-checks and removes it. */
	pending?: PendingItem;
	/** Sentences that must stay on the named surface. */
	mustSay: { in: Surface; text: string }[];
	/**
	 * Phrases no page may contain (the claims the code contradicts). A plain string is checked against
	 * every surface; `{ in, text }` against one surface, for a word that is true elsewhere.
	 */
	mustNotSay: (string | { in: Surface; text: string })[];
	/** Code or live-read lines the claim rests on. `anchor` must occur in the file. */
	evidence: { path: string; anchor: string }[];
};

const FALLBACK = "apps/Model Plane/rust/services/inference-core/src/provider/fallback.rs";
const CODEX = "apps/Model Plane/rust/services/inference-core/src/provider/codex_subscription.rs";
const LIVE_2026_10_03 = "docs/compliance/evidence/live-reads-trust-claims-2026-10-03.txt";

export const trustClaims: TrustClaim[] = [
	{
		id: "eu-region-is-a-label",
		rows: ["T1a", "T1b", "T1c", "T1d", "T13c", "C3"],
		statement:
			"The EU region is a configured label on the default Azure OpenAI route only. Foundry Claude, Foundry Cohere embeddings and the ChatGPT route do not carry it, the deployment type is unset, and no hosting location is stated.",
		mustSay: [
			{ in: "control:EU-region på standardmodellene", text: "Regionen er en etikett vi selv setter og ennå ikke har bevist uavhengig" },
			{ in: "control:EU-region på standardmodellene", text: "Claude-modellene går via Azure AI Foundry uten oppgitt region" },
			{ in: "control:EU-region på standardmodellene", text: "ChatGPT-modeller du selv kobler til via din egen OpenAI-konto har global behandling uten EU-garanti" },
			{ in: "control:EU-region på standardmodellene", text: "Balance og Genius sender komplekse turer til Claude" },
			{ in: "control:EU-region på standardmodellene", text: "ikke er et målt behandlingssted" },
			{ in: "control:EU-region på standardmodellene", text: "Hvor Verevons egne lagre kjører, oppgir vi ikke her før vi kan dokumentere det" },
			{ in: "subprocessor:Microsoft Azure — Azure OpenAI og Azure AI-tjenester", text: "Distribusjonstypen (regional eller global) er ikke satt" },
			{ in: "file:TrustCenter.tsx", text: "Regionen er en etikett vi selv setter" },
			{ in: "file:TrustCenter.tsx", text: "oppgir vi ikke her før vi kan dokumentere det" },
		],
		mustNotSay: [
			"EU-residens som standard",
			"EØS-residens bekreftet",
			"bekreftet på denne stien",
			"ligger i EU/EØS",
			"Primær inferens og embeddings kjører på Azure OpenAI",
			"EU/EØS-lagre under Verevons kontroll",
		],
		evidence: [
			{ path: FALLBACK, anchor: "Phase 0's deployment-type gate deliberately only warns" },
			{ path: FALLBACK, anchor: "AZURE_OPENAI_DEPLOYMENT_TYPE is unset" },
			{ path: "apps/Model Plane/rust/services/inference-core/src/provider/routing_policy.rs", anchor: "claude-sonnet-4-6" },
			{ path: "apps/Data Plane v2/services/embedding-engine-rs/src/provider/mod.rs", anchor: "ZDR content must not egress to the Cohere Embed v4 text path" },
			{ path: LIVE_2026_10_03, anchor: "AZURE_OPENAI_DEPLOYMENT_TYPE\tempty" },
			{ path: LIVE_2026_10_03, anchor: "AZURE_ANTHROPIC_REGION\tempty" },
		],
	},
	{
		id: "zdr-is-a-gate-not-a-guarantee",
		rows: ["T2a", "T2b", "T2c", "C7", "H2", "V1"],
		pending: "c: per-connection zero-retention (ZDR) consent flow",
		statement:
			"ZDR is optional and off by default. An organisation can turn it on for all requests only on a Pro or Enterprise plan; a user can also choose Midlertidig samtale (Temporary chat) for one conversation, and nothing checks the plan on that path. When on, a request goes only to a provider with a recorded attestation and is refused otherwise. The attestation is our own record, none is set on the stack read, and the ChatGPT route refuses ZDR. ZDR concerns the model vendor, not Verevon's own stores: metadata and audit events are kept, conversations and run history of non-ZDR work stay until someone deletes them and are not deleted by age.",
		mustSay: [
			{ in: "control:Zero Data Retention — valgfritt tillegg", text: "ZDR er valgfritt og av som standard. En organisasjon kan slå det på for alle forespørsler, men bare på Pro- eller Enterprise-plan; en bruker kan også velge «Midlertidig samtale» for én samtale" },
			{ in: "file:TrustCenter.tsx", text: "men bare på Pro- eller Enterprise-plan; en bruker kan også velge «Midlertidig samtale» for én samtale" },
			{ in: "control:Zero Data Retention — valgfritt tillegg", text: "Uten slik godkjenning avvises forespørselen i stedet for å sendes videre" },
			{ in: "control:Zero Data Retention — valgfritt tillegg", text: "Godkjenningen er vår egen registrering og er ikke uavhengig verifisert" },
			{ in: "control:Zero Data Retention — valgfritt tillegg", text: "I miljøet vi har gjennomgått er ingen slik godkjenning registrert, så en ZDR-forespørsel avvises der" },
			{ in: "control:Zero Data Retention — valgfritt tillegg", text: "ChatGPT-ruten støtter ikke ZDR, og en ZDR-forespørsel mot den avvises" },
			{ in: "control:Zero Data Retention — valgfritt tillegg", text: "ZDR gjelder modell-leverandøren, ikke Verevons egne lagre" },
			{ in: "control:Zero Data Retention — valgfritt tillegg", text: "de slettes ikke automatisk etter alder" },
			{ in: "file:TrustCenter.tsx", text: "så en ZDR-forespørsel avvises der" },
			{ in: "file:TrustCenter.tsx", text: "ChatGPT-ruten støtter ikke ZDR" },
			{ in: "file:TrustCenter.tsx", text: "ZDR gjelder modell-leverandøren, ikke Verevons egne lagre" },
			{ in: "file:TrustBandSection.tsx", text: "ZDR-forespørsler skrives aldri til vår egen inferens-cache" },
			{ in: "file:trust-scroll.tsx", text: "sender forespørselen bare til en leverandør med registrert ZDR-godkjenning" },
		],
		mustNotSay: [
			"betalt Pro/Enterprise",
			"Pro/Enterprise-tillegg",
			// The plan gate covers the organisation-wide setting only; a per-conversation request needs no plan.
			"som bare organisasjoner på Pro- eller Enterprise-plan kan slå på",
			"som organisasjoner på Pro- eller Enterprise-plan kan slå på",
			"binder modell-leverandøren",
			"ZDR binder da",
			"Promptene lagres ikke hos modellen",
			"uten at promptene dine lagres",
			"Kun Azure OpenAI Sweden Central-stien støtter",
			"etter sin sletteplan",
			"etter sin lagringsplan",
		],
		evidence: [
			{ path: FALLBACK, anchor: "if req.zdr && !caps.supports_zdr" },
			{ path: "apps/Model Plane/rust/services/session-core/src/terminalization.rs", anchor: "zdr-redacted" },
			{ path: "apps/Control Plane/org-core/internal/org/service_enhanced.go", anchor: "PlanAllowsZeroDataRetention(current.Plan)" },
			{ path: "apps/Control Plane/org-core/internal/org/types.go", anchor: "var zeroDataRetentionPlans" },
			{ path: "apps/Frontend Plane/verevonv3/apps/gateway/src/zdr.rs", anchor: "Either one alone means" },
			{ path: "docs/compliance/evidence/retention-and-erasure-evidence.md", anchor: "Nothing deletes conversations, messages, runs, events or memory by age" },
			{ path: "apps/Model Plane/rust/services/inference-core/src/provider/zdr.rs", anchor: "What the digest does not prove" },
			{ path: CODEX, anchor: "has no verified ZDR commitment" },
			{ path: "apps/Model Plane/rust/services/inference-core/src/cache.rs", anchor: "Skips insertion for ZDR requests" },
			{ path: LIVE_2026_10_03, anchor: "AZURE_OPENAI_ZDR_DIGEST\tempty" },
			{ path: LIVE_2026_10_03, anchor: "AZURE_ANTHROPIC_ZDR_DIGEST\tempty" },
		],
	},
	{
		id: "approval-gate-is-for-deployed-agents",
		rows: ["T3", "H6", "C2"],
		statement:
			"The Approve or Reject stop belongs to deployed agents (the ask posture). Interactive chat has no general gate, and a few tools wait for approval whatever the posture.",
		mustSay: [
			{ in: "control:Godkjenning per handling", text: "Utplasserte agenter stopper risikofylte handlinger" },
			{ in: "control:Godkjenning per handling", text: "I vanlig interaktiv chat finnes ingen generell godkjenningsport" },
			{ in: "file:TrustBandSection.tsx", text: "I vanlig chat finnes ingen slik port" },
			{ in: "file:TechnologySection.tsx", text: "Utplasserte agenter stopper risikofylte handlinger" },
			{ in: "file:trust-scroll.tsx", text: "Når en utplassert agent skal sende et svar" },
		],
		mustNotSay: ["Mennesket sier ja før noe", "Risikofylte handlinger stoppes", "Når Verevon vil sende et svar"],
		evidence: [
			{ path: "apps/Model Plane/rust/services/model-gateway/src/profile.rs", anchor: "no approval gates" },
			{ path: "apps/Model Plane/rust/services/execution-core/src/permission/mod.rs", anchor: "PermissionMode::Ask if is_risky_call" },
		],
	},
	{
		id: "audit-trail-not-verified-end-to-end",
		rows: ["T4", "T11b", "H3", "H7"],
		statement:
			"The Used by AI? trail is built but not verified end to end: on the stack read no model-plane event reached audit-core.",
		mustSay: [
			{ in: "control:«Brukt av AI?»-revisjonsspor", text: "Det er ikke verifisert ende til ende" },
			{ in: "control:«Brukt av AI?»-revisjonsspor", text: "ble ingen av modell-planets hendelser levert til revisjonstjenesten" },
			{ in: "certification:In-app Trust Center («Brukt av AI?»)", text: "ikke verifisert ende til ende" },
			{ in: "file:TrustBandSection.tsx", text: "Sporet er ikke verifisert ende til ende ennå" },
		],
		mustNotSay: [
			"Et oppslag på tool_action returnerer",
			"matet fra revisjonssporet på tvers av planene",
			"Revisjonstjenesten abonnerer på modell-planet",
			"hva AI-en faktisk brukte",
			"ser du hvilke kilder AI-en brukte",
		],
		evidence: [
			{ path: "apps/Model Plane/rust/services/session-core/src/audit_publisher.rs", anchor: "verevon.audit.v2.model.session-core.tool_action" },
			{ path: LIVE_2026_10_03, anchor: "no stream found for given subject" },
		],
	},
	{
		id: "classification-does-not-steer-chat",
		rows: ["T5a", "T5b", "C5", "H3"],
		pending: "b: Data Plane rerank capability for Quarry",
		statement:
			"The class labels data. The Data Plane refuses to send ZDR-marked content to external embeddings, and a sovereign-required search is kept off Azure embedding and rerank. In chat the class does not yet choose the provider.",
		mustSay: [
			{ in: "control:Dataklassifisering", text: "I chat brukes klassen ennå ikke til å velge modell-leverandør" },
			{ in: "control:Dataklassifisering", text: "søk som krever suveren behandling holdes unna Azure-embedding og rerank" },
			{ in: "file:TrustCenter.tsx", text: "I chat brukes klassen ennå ikke til å velge modell-leverandør" },
			{ in: "file:TrustCenter.tsx", text: "styrer ennå ikke valg av leverandør i chat" },
		],
		mustNotSay: ["Default-deny tredjeparts", "styrer lagring, indeksering og deling", "styres riktig"],
		evidence: [
			{ path: "apps/Model Plane/rust/services/session-core/src/grpc.rs", anchor: "_privacy_processing_flags" },
			{ path: "apps/Data Plane v2/services/embedding-engine-rs/src/provider/mod.rs", anchor: "ZDR content must not egress to the Cohere Embed v4 text path" },
			{ path: "apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs", anchor: "cloud_egress_restricted" },
		],
	},
	{
		id: "user-erasure-scope",
		rows: ["T8", "P5"],
		statement:
			"A per-user erasure removes the Control Plane account data and does not delete conversations, memory, retrieval traces or cost rows. The routine is not run end to end on the stack read, and organisation erasure is built but not verified end to end.",
		mustSay: [
			{ in: "control:Rett til sletting (erasure)", text: "Den sletter ikke samtaler, minne, søkespor eller kostnadsrader i de andre planene" },
			{ in: "control:Rett til sletting (erasure)", text: "ikke kjørt ende til ende i miljøet vi har gjennomgått" },
			{ in: "control:Rett til sletting (erasure)", text: "sletting av en hel organisasjon er bygget for å fjerne organisasjonens kopier, men er ikke verifisert ende til ende" },
		],
		mustNotSay: [
			"Sletting av avledede kopier på tvers av planene er under arbeid",
			"fjerner samtaler",
			"fjernes gjennom den separate slettingsprosessen",
		],
		evidence: [
			{ path: "apps/Model Plane/rust/services/session-core/src/gdpr.rs", anchor: "NOT a request to erase that org" },
			{ path: "apps/Model Plane/rust/services/session-core/src/gdpr.rs", anchor: "return Ok(None)" },
			{ path: "apps/Model Plane/rust/services/session-core/src/gdpr_nats.rs", anchor: "Ok(Outcome::Skipped)" },
			{ path: "apps/Control Plane/user-core/internal/http/gdpr_handlers.go", anchor: "func (s *Server) hardEraseUser" },
		],
	},
	{
		id: "search-forwards-queries",
		rows: ["T9a", "T9b", "T9c", "T13f", "V3", "A7"],
		statement:
			"The free search chain ends in our SearXNG, which forwards the query to external engines, ZDR included. Paid providers need an explicit per-request permission, never for ZDR, never in agent runs; they are a backup when the free chain returns too little and can run early for a few terse queries. A switch for the paid providers exists; none stops all external search traffic.",
		mustSay: [
			{ in: "control:Søk: egen kjede først", text: "brukes bare når en forespørsel eksplisitt har fått lov til det" },
			{ in: "control:Søk: egen kjede først", text: "kan brukes tidlig for noen korte søk" },
			{ in: "control:Søk: egen kjede først", text: "SearXNG sender spørringen videre til eksterne søkemotorer" },
			{ in: "control:Søk: egen kjede først", text: "også for ZDR-forespørsler" },
			{ in: "control:Søk: egen kjede først", text: "en bryter som stopper all ekstern søketrafikk finnes ikke" },
			{ in: "subprocessor:Eksterne søkemotorer via vår SearXNG, og Brave og Serper (valgfrie)", text: "Gjelder også ZDR-forespørsler for den gratis kjeden" },
		],
		mustNotSay: ["null ekstern trafikk", "null egress", "du får null ekstern", "Søk i egen infrastruktur", "bare som siste utvei", "bare som betalt siste utvei"],
		evidence: [
			{ path: "apps/Ingestion Plane/config/searxng/settings.yml", anchor: "google cse and bing carry the query" },
			{ path: "apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/smart_router.rs", anchor: "the narrow eager path" },
			{ path: "apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/smart_router.rs", anchor: "is all a zero-retention query ever gets" },
			{ path: "apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs", anchor: "if cfg.zero_saas_search {" },
		],
	},
	{
		id: "dsar-covers-control-plane-only",
		rows: ["T10"],
		statement: "The data-access export covers Control Plane data only.",
		mustSay: [
			{ in: "control:Kundevendt DSAR / eksport", text: "dekker Control Plane-data (profil, medlemskap og API-nøkkelmetadata)" },
			{ in: "control:Kundevendt DSAR / eksport", text: "Samtaler, minne og dokumenter i de andre planene er ikke med i eksporten ennå" },
		],
		mustNotSay: [],
		evidence: [{ path: "apps/Control Plane/user-core/internal/users/gdpr.go", anchor: "DSARControlPlaneDisclosure" }],
	},
	{
		id: "terra-route-disclosed",
		rows: ["T13e", "P4"],
		pending: "c: per-connection zero-retention (ZDR) consent flow",
		statement:
			"The ChatGPT-account route is named: chosen by the user's own connection, global processing, no EU or ZDR guarantee, a ZDR request is refused, and the whole prompt goes to OpenAI under the user's own account.",
		mustSay: [
			{ in: "subprocessor:OpenAI — brukerens egen ChatGPT-konto", text: "Global, ingen regionsforpliktelse" },
			{ in: "subprocessor:OpenAI — brukerens egen ChatGPT-konto", text: "en bruker kobler til sin egen ChatGPT-konto og velger en ChatGPT-modell" },
			{ in: "subprocessor:OpenAI — brukerens egen ChatGPT-konto", text: "Støtter ikke ZDR, og en ZDR-forespørsel mot ruten avvises" },
			{ in: "subprocessor:OpenAI — brukerens egen ChatGPT-konto", text: "sendes til OpenAI under brukerens egen konto og vilkår" },
			{ in: "file:TrustCenter.tsx", text: "en ChatGPT-modell brukeren selv velger går via brukerens egen OpenAI-konto med global behandling og uten EU- eller ZDR-garanti" },
		],
		// "svaret merkes «Behandlet: ...»" was dropped on 2026-10-03: the per-answer line is uncommitted work of
		// another session (privacy-tier.ts answerProcessingNote, sse_events.rs), not yet shipped or deployed.
		mustNotSay: ["svaret merkes", "Behandlet: global"],
		evidence: [
			{ path: CODEX, anchor: "has no verified ZDR commitment" },
			{ path: CODEX, anchor: "subscription_connection_id is required" },
			{ path: "apps/Frontend Plane/verevonv3/src/shared/api/privacy-tier.ts", anchor: "Ingen regionsbegrensning" },
		],
	},
	{
		id: "cloud-act-names-openai",
		rows: ["C6"],
		statement:
			"The CLOUD Act exposure covers Microsoft and OpenAI on the ChatGPT route. The draft transfer assessment is not counsel-reviewed and names Microsoft only.",
		mustSay: [
			{ in: "file:TrustCenter.tsx", text: "og OpenAI på ChatGPT-ruten" },
			{ in: "file:TrustCenter.tsx", text: "nevner foreløpig bare Microsoft, ikke OpenAI" },
		],
		mustNotSay: ["med supplerende tiltak"],
		evidence: [{ path: "docs/compliance/transfer-assessment.md", anchor: "CLOUD Act" }],
	},
	{
		id: "dpa-and-assessments-are-drafts",
		rows: ["C9"],
		statement:
			"The DPA, the transfer assessment and the data-flow diagram are in preparation and not counsel-reviewed, so the page does not offer them.",
		mustSay: [{ in: "file:TrustCenter.tsx", text: "er under utarbeidelse og ennå ikke gjennomgått av norsk personvernjurist" }],
		mustNotSay: ["Vi deler databehandler", "sikkerhetsarkitektur", "Be om pakken", "Be om sikkerhetspakken"],
		evidence: [{ path: "docs/compliance/README.md", anchor: "None of these documents has been reviewed" }],
	},
	{
		id: "no-certifications-held",
		rows: ["T12", "K2"],
		statement:
			"No third-party certification is held; every certification entry is planned. The page does not call the platform sovereign: no provider is classed Norwegian-sovereign and there is no audited sovereignty claim.",
		mustSay: [],
		mustNotSay: [
			"suveren EU-AI-plattform",
			"suverene AI-spydspissen",
			"suveren AI-plattform",
			// No certification work is evidenced, so the page claims neither leadership nor a date.
			"vi leder an på",
			"før kravene trer i kraft",
		],
		evidence: [
			{ path: "VEREVON.md", anchor: "no third-party certifications yet" },
			{ path: "apps/Model Plane/rust/services/inference-core/src/provider/mod.rs", anchor: "Do **not** classify an Azure" },
		],
	},
	{
		id: "homepage-and-footer-narrowed",
		rows: ["H1", "H4", "H5", "H6", "C10"],
		statement:
			"The homepage trust band, scroll copy, technology statistics and footer say EU region on the default models, not EU by default, and no longer name data sovereignty or promise unstored prompts.",
		mustSay: [
			{ in: "file:Footer.tsx", text: "Standardmodeller satt til Sweden Central (EU/EØS)" },
			{ in: "file:Footer.tsx", text: "Dataflyt og underleverandører" },
			{ in: "file:TrustBandSection.tsx", text: "EU-region på standardmodellene" },
			{ in: "file:TechnologySection.tsx", text: "EU-region på standardmodellene" },
			{ in: "file:trust-scroll.tsx", text: "EU-region på standardmodellene" },
			{ in: "file:page.tsx", text: "EU-region på standardmodellene" },
		],
		mustNotSay: [
			"EU som standard",
			// The page says it cannot document where Verevon's own stores run, so no sentence promises a place.
			"ikke kan gå på akkord med hvor",
			"forspranget vårt skarpere",
			"Data i EU/EØS",
			"Tillit og datasuverenitet",
			"sikkerhet og datasuverenitet",
			"Datasuverenitet ↗",
			"kjører chat, søk og agentoppgaver i EU",
			"kjøres i Azure Sweden Central",
			"Forsvarbare, live kontroller",
		],
		evidence: [
			{ path: "apps/Frontend Plane/verevonv3/src/features/dashboard/home/submit-privacy-tier.ts", anchor: "An intent mode carries none" },
		],
	},
	{
		id: "demo-card-makes-no-region-or-zdr-claim",
		rows: ["H7"],
		statement: "The product demo card does not state a region or a ZDR status for the audit step.",
		mustSay: [{ in: "file:PromptSection.tsx", text: "Spor lagret i saken" }],
		mustNotSay: [
			{ in: "file:PromptSection.tsx", text: "EU/ZDR" },
			{ in: "file:PromptSection.tsx", text: "Sweden Central" },
			{ in: "file:PromptSection.tsx", text: "ZDR" },
			{ in: "file:PromptSection.tsx", text: "kilder, region" },
		],
		evidence: [{ path: LIVE_2026_10_03, anchor: "no stream found for given subject" }],
	},
	{
		id: "tts-gate-is-narrow",
		rows: ["T13b", "C4", "V6"],
		statement:
			"Only a non-EU override of Azure OpenAI text-to-speech is refused unless explicitly allowed; the Azure Speech region is a configured label.",
		mustSay: [
			{ in: "file:TrustCenter.tsx", text: "bare en overstyring av Azure OpenAI-tale til et ikke-EU-endepunkt nektes med mindre den eksplisitt åpnes" },
		],
		mustNotSay: ["Tale (TTS) står på EU-endepunktet", "nekter et konfigurert ikke-EU-endepunkt med mindre det eksplisitt åpnes"],
		evidence: [
			{ path: "apps/Model Plane/rust/services/inference-core/src/provider/speech.rs", anchor: "let allow_non_eu_tts" },
		],
	},
	{
		id: "anthropic-direct-is-the-reserve",
		rows: ["T13d"],
		statement:
			"The direct Anthropic API is the reserve and is registered only when Azure AI Foundry Claude is not. Claude models themselves are first-line for complex turns, through Foundry, which declares no region.",
		mustSay: [
			{ in: "subprocessor:Anthropic (direkte API)", text: "bare hvis Azure AI Foundry ikke er konfigurert" },
			{ in: "subprocessor:Microsoft Azure AI Foundry — Claude og Cohere", text: "Claude-modeller for komplekse samtaleturer og verktøyturer" },
		],
		mustNotSay: ["Reserve modell-leverandør (fallback)"],
		evidence: [
			{ path: FALLBACK, anchor: "if !registered_azure_anthropic" },
			{ path: "apps/Model Plane/rust/services/inference-core/src/provider/routing_policy.rs", anchor: "claude-sonnet-4-6" },
		],
	},
	{
		id: "cohere-command-chat-is-named",
		rows: ["T1a", "T13g"],
		statement:
			"A Cohere Command chat deployment on Azure AI Foundry is registered in inference-core and serves only its own model when it is chosen; its region is a configured label (swedencentral on the stack read) and it has no ZDR attestation. The page names it on the Foundry row.",
		mustSay: [
			{ in: "subprocessor:Microsoft Azure AI Foundry — Claude og Cohere", text: "Cohere Command-modellen for samtaler, når den velges" },
			{ in: "subprocessor:Microsoft Azure AI Foundry — Claude og Cohere", text: "Cohere Command: satt til Sweden Central (en etikett, ikke uavhengig verifisert)" },
		],
		mustNotSay: [],
		evidence: [
			{ path: FALLBACK, anchor: 'with_identity("cohere", azure_cohere_residency, true)' },
			{ path: LIVE_2026_10_03, anchor: "AZURE_COHERE_REGION\tset\tswedencentral" },
		],
	},
	{
		id: "foundry-cohere-embeds-and-reranks",
		rows: ["T1b", "T13g"],
		pending: "b: Data Plane rerank capability for Quarry",
		statement:
			"Document and query embeddings, and the Data Plane rerank, go to Cohere on Azure AI Foundry directly, outside the inference-core gates; ZDR content is refused. The public Cohere API is a reserve for rerank.",
		mustSay: [
			{ in: "subprocessor:Microsoft Azure AI Foundry — Claude og Cohere", text: "Cohere Embed v4 for embeddings av dokumenter og søk, samt rerank" },
			{ in: "subprocessor:Microsoft Azure AI Foundry — Claude og Cohere", text: "ZDR-merket innhold avvises" },
			{ in: "subprocessor:Cohere (offentlig API)", text: "Reserve for rerank" },
		],
		mustNotSay: [],
		evidence: [
			{ path: "apps/Data Plane v2/docker-compose.yml", anchor: "Cohere Embed v4, direct (not routed through the Model Plane" },
			{ path: "apps/Data Plane v2/services/retrieval-engine-rs/src/search/rerank.rs", anchor: "pub fn with_endpoint" },
		],
	},
	{
		id: "contextual-retrieval-sends-chunks-to-azure-openai",
		rows: ["T1b", "T13a"],
		pending: "a: contextual retrieval rework (metering, ZDR handling, backfill)",
		statement:
			"When contextual retrieval and entity extraction are on, document chunk text goes to the Azure OpenAI chain for summaries at indexing time; restricted (ZDR) documents are filtered out first.",
		mustSay: [
			{
				in: "subprocessor:Microsoft Azure — Azure OpenAI og Azure AI-tjenester",
				text: "oppsummeringer og entitetsuttrekk fra dokumentutdrag ved indeksering, når de er slått på",
			},
		],
		mustNotSay: [],
		evidence: [
			{ path: "apps/Data Plane v2/services/embedding-engine-rs/src/provider/contextualize.rs", anchor: "Restricted content never reaches this request" },
			{ path: "apps/Data Plane v2/docker-compose.yml", anchor: "CONTEXTUAL_RETRIEVAL_ENABLED" },
			{ path: LIVE_2026_10_03, anchor: "CONTEXTUAL_RETRIEVAL_ENABLED=true" },
		],
	},
];
