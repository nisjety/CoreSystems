import { ArrowButton } from "@/components/ui/ArrowButton";
import { Eyebrow } from "@/components/ui/SectionHeading";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
	certifications,
	dataClasses,
	liveControls,
	procurementEvidence,
	subprocessors,
} from "./trust-data";

const SECURITY_EMAIL = "sikkerhet@verevon.ai";

function TrustHero() {
	return (
		<section
			className="relative isolate overflow-hidden border-b border-verevon-j-text/8 bg-[linear-gradient(180deg,var(--verevon-trust-tint)_0%,var(--background)_60%)] px-[var(--verevon-edge)] pb-[clamp(72px,8vw,120px)] pt-[clamp(72px,10vh,128px)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="top"
		>
			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-0 z-0 bg-[radial-gradient(circle_at_84%_-10%,rgba(238,122,80,0.12),transparent_38%)]"
			/>

			<div className="relative z-[1] mx-auto max-w-[1680px]">
				<Eyebrow marker>Trust Center</Eyebrow>

				<h1 className="mt-7 max-w-[18ch] font-manrope text-[clamp(2.7rem,5.2vw,5.6rem)] font-light leading-[0.96] tracking-[-0.06em] text-verevon-j-text text-balance">
					Sikkerhet, personvern og AI-styring.
				</h1>

				<p className="mt-8 max-w-[64ch] font-geist text-[var(--text-body-lg)] font-light leading-[1.5] text-verevon-text-muted text-pretty">
					Verevon er bygget for virksomheter som må vite hvor dataene deres går.
					Her er den ærlige tilstanden: mange kontroller er
					håndhevet i kode, noen er bare erklært eller valgfrie per forespørsel,
					og vi holder ennå ingen tredjeparts­sertifiseringer. Vi viser alt dette,
					uten pynt.
				</p>

				<div className="mt-10 flex flex-wrap items-center gap-x-9 gap-y-4">
					<div className="flex flex-wrap items-center gap-3">
						<StatusBadge level="live" label="Kontroller i kode" />
						<StatusBadge level="planned" label="Sertifiseringer på vei" />
					</div>

					<ArrowButton href={`mailto:${SECURITY_EMAIL}`} variant="coral">
						Kontakt sikkerhets­teamet
					</ArrowButton>
				</div>
			</div>
		</section>
	);
}

function ProcurementEvidenceSection() {
	return (
		<section
			className="border-b border-verevon-j-text/8 bg-verevon-surface-soft/40 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="bevis"
		>
			<div className="mx-auto max-w-[1680px]">
				<div className="max-w-[760px]">
					<Eyebrow marker>Dokumentasjon for innkjøp</Eyebrow>
					<h2 className="verevon-h2 mt-6 text-balance">
						Hva dere kan verifisere i dag.
					</h2>
					<p className="verevon-body-lg mt-7 max-w-[62ch] text-pretty">
						Dette er statusen på materialet en sikkerhets- eller
						innkjøpsprosess vanligvis ber om. Vi omtaler ikke en kontroll som
						dokumentert før både omfanget og begrensningen kan vises her.
					</p>
				</div>

				<ul className="mt-[clamp(40px,5vw,72px)] flex flex-col border-b border-verevon-j-text/12">
					{procurementEvidence.map((item) => (
						<li
							className="grid gap-x-8 gap-y-5 border-t border-verevon-j-text/12 py-7 lg:grid-cols-[minmax(190px,0.75fr)_minmax(0,1fr)_minmax(0,1fr)]"
							data-procurement-evidence
							key={item.title}
						>
							<div className="flex items-start justify-between gap-4 lg:block">
								<h3 className="verevon-h3 text-[clamp(1.2rem,1.3vw,1.6rem)] text-balance">
									{item.title}
								</h3>
								<div className="mt-0 shrink-0 lg:mt-4">
									<StatusBadge level={item.level} />
								</div>
							</div>
							<div>
								<p className="verevon-eyebrow mb-3">Tilgjengelig nå</p>
								<p className="verevon-body text-pretty">{item.available}</p>
							</div>
							<div>
								<p className="verevon-eyebrow mb-3">Ikke dokumentert ennå</p>
								<p className="verevon-body text-pretty">{item.pending}</p>
							</div>
						</li>
					))}
				</ul>
			</div>
		</section>
	);
}

function ControlsSection() {
	return (
		<section
			className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="kontroller"
		>
			<div className="mx-auto max-w-[1680px]">
				<div className="max-w-[760px]">
					<Eyebrow>Det vi kan stå for</Eyebrow>
					<h2 className="verevon-h2 mt-6 text-balance">
						De forsvarbare forskjellene — og hvor hver av dem stopper.
					</h2>
					<p className="verevon-body-lg mt-7 max-w-[600px] text-pretty">
						Den late påstanden «amerikanske leverandører lekker dataene dine»
						holder ikke mot en moden konkurrent — og Verevon bruker selv Azures
						AI-tjenester. Derfor sier vi i stedet hva hver kontroll gjør, og hvor den stopper.
					</p>
				</div>

				<div className="mt-[clamp(48px,6vw,80px)] grid gap-x-[clamp(32px,3.4vw,72px)] gap-y-[clamp(34px,3.6vw,52px)] sm:grid-cols-2 xl:grid-cols-3">
					{liveControls.map((control) => (
						<div
							className="flex flex-col gap-4 border-t border-verevon-j-text/12 pt-7"
							key={control.title}
						>
							<div className="flex items-start justify-between gap-4">
								<h3 className="verevon-h3 max-w-[20ch] text-balance">
									{control.title}
								</h3>
								<StatusBadge level={control.level} />
							</div>
							<p className="verevon-body text-pretty">{control.body}</p>
						</div>
					))}
				</div>
			</div>
		</section>
	);
}

function CertificationsSection() {
	return (
		<section
			className="border-b border-verevon-j-text/8 bg-verevon-surface-soft/40 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="sertifiseringer"
		>
			<div className="mx-auto max-w-[1680px]">
				<div className="max-w-[820px]">
					<Eyebrow>Sertifiseringer & løype</Eyebrow>
					<h2 className="verevon-h2 mt-6 text-balance">
						Vi har mange av kontrollene — men ennå ikke sertifiseringene.
					</h2>
					<p className="verevon-body-lg mt-7 max-w-[640px] text-pretty">
						Det er den ubehagelige sannheten en tillitsledet posisjon må eie. På
						innkjøps­modent papir kan en Vanta-støttet konkurrent med SOC&nbsp;2 og
						ISO&nbsp;42001 i dag dokumentere mer enn oss. Å lukke gapet er en
						prioritet — og vi har valgt{" "}
						<strong className="font-medium text-verevon-j-text">
							ISO&nbsp;42001 og EU&nbsp;AI&nbsp;Act
						</strong>{" "}
						som vår AI-spydspiss.
					</p>
				</div>

				<ul className="mt-[clamp(40px,5vw,72px)] flex flex-col">
					{certifications.map((cert) => (
						<li
							className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-6 gap-y-3 border-t border-verevon-j-text/12 py-6 last:border-b sm:grid-cols-[minmax(220px,0.9fr)_auto_minmax(0,1.6fr)] sm:items-center"
							key={cert.name}
						>
							<h3 className="verevon-h3 text-[clamp(1.2rem,1.3vw,1.6rem)]">
								{cert.name}
							</h3>
							<div className="row-start-1 justify-self-end sm:row-start-auto sm:justify-self-start">
								<StatusBadge level={cert.level} />
							</div>
							<p className="col-span-2 font-geist text-[var(--text-body-sm)] font-light leading-[1.5] text-verevon-text-muted text-pretty sm:col-span-1">
								{cert.plan}
							</p>
						</li>
					))}
				</ul>
			</div>
		</section>
	);
}

function ResidencySection() {
	return (
		<section
			className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="dataflyt"
		>
			<div className="mx-auto grid max-w-[1680px] gap-x-[clamp(40px,5vw,96px)] gap-y-12 xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
				<div>
					<Eyebrow marker>Datalagring & dataflyt</Eyebrow>
					<h2 className="verevon-h2 mt-6 text-balance">
						Hvor dataene dine går — og hva vi ikke kan love.
					</h2>
					<p className="verevon-body-lg mt-7 max-w-[52ch] text-pretty">
						Standardmodellene kjører hovedsakelig på Azure OpenAI med regionen satt til Sweden
						Central. Regionen er en etikett vi selv setter, og ikke alle ruter har
						den: Claude-modellene går via Azure AI Foundry uten oppgitt region,
						dokument-embeddings via Cohere på Azure AI Foundry, og en ChatGPT-modell
						brukeren selv velger går via brukerens egen OpenAI-konto med global
						behandling og uten EU- eller ZDR-garanti. Nett-søk går via eksterne
						søkemotorer. Tale (TTS) bruker Azure Speech og Azure OpenAI med en
						konfigurert region; bare en overstyring av Azure OpenAI-tale til et
						ikke-EU-endepunkt nektes med mindre den eksplisitt åpnes. Hvor Verevons
						egne lagre (app-data, samtaler, kjørehistorikk, revisjon og vektorlager)
						kjører, oppgir vi ikke her før vi kan dokumentere det.
					</p>

					<div className="mt-9">
						<p className="verevon-eyebrow mb-5">Dataklassifisering — seks klasser</p>
						<ul className="flex flex-wrap gap-2.5">
							{dataClasses.map((klass) => (
								<li
									className="rounded-full border border-verevon-j-text/14 bg-verevon-surface px-3.5 py-1.5 font-geist text-[0.82rem] font-light text-verevon-j-text/72"
									key={klass}
								>
									{klass}
								</li>
							))}
						</ul>
						<p className="verevon-body mt-5 max-w-[52ch] text-pretty">
							Data Plane nekter å sende ZDR-merket innhold til eksterne embeddings.
							I chat brukes klassen ennå ikke til å velge modell-leverandør.
							GDPR-policy­metadata (formål, lovlig grunnlag, lagringstid, residens,
							personvern­klasse, sletteomfang) kreves på varige poster i Spaces, men
							styrer ennå ikke valg av leverandør i chat.
						</p>
					</div>
				</div>

				<aside className="flex flex-col gap-7">
					<div className="rounded-[var(--radius-lg)] border border-verevon-trust-line/60 bg-verevon-trust-tint/70 p-[clamp(24px,2.6vw,40px)]">
						<div className="mb-4 flex items-center gap-3">
							<StatusBadge level="progress" label="Åpent oppgitt" />
						</div>
						<h3 className="font-manrope text-[clamp(1.35rem,1.7vw,2rem)] font-light leading-[1.06] tracking-[-0.04em] text-verevon-j-text">
							Åpenhet, ikke immunitet: CLOUD Act
						</h3>
						<p className="verevon-body mt-5 text-pretty">
							EU-residens er ikke det samme som datasuverenitet. En
							leverandør med hovedkontor i USA — også Microsoft Azure, og OpenAI på
							ChatGPT-ruten — kan nås under den amerikanske CLOUD Act uavhengig av
							fysisk plassering. Vi hevder aldri at residens gir immunitet.
						</p>
						<p className="verevon-body mt-4 text-pretty">
							Dette er ført som en åpent oppgitt restrisiko i vårt utkast til
							Schrems&nbsp;II-overførings­vurdering. Utkastet er ikke gjennomgått av
							norsk personvernjurist ennå og nevner foreløpig bare Microsoft, ikke
							OpenAI. Vi sier det rett ut framfor å pakke det bort.
						</p>
					</div>

					<div className="rounded-[var(--radius-lg)] border border-verevon-j-text/10 bg-verevon-surface p-[clamp(24px,2.6vw,40px)]">
						<h3 className="font-manrope text-[clamp(1.2rem,1.4vw,1.6rem)] font-light leading-[1.1] tracking-[-0.04em] text-verevon-j-text">
							ZDR — ærlig omfang
						</h3>
						<p className="verevon-body mt-5 text-pretty">
							Zero Data Retention er valgfritt og av som standard. En organisasjon kan
							slå det på for alle forespørsler, men bare på Pro- eller Enterprise-plan;
							en bruker kan også velge «Midlertidig samtale» for én samtale. Når ZDR
							er på, sender plattformen en ZDR-forespørsel bare til
							en leverandør vi har en registrert og datert ZDR-godkjenning for, og
							avviser den ellers. Godkjenningen er vår egen registrering, ikke en
							uavhengig verifisering, og om leverandøren faktisk holder avtalen er et
							kontraktsspørsmål. I miljøet vi har gjennomgått er ingen slik
							godkjenning registrert, så en ZDR-forespørsel avvises der.
							ChatGPT-ruten støtter ikke ZDR. ZDR gjelder modell-leverandøren, ikke
							Verevons egne lagre: Verevon beholder metadata og revisjonshendelser, og
							for arbeid uten ZDR beholdes samtaler og kjørehistorikk til noen sletter
							dem; de slettes ikke automatisk etter alder.
						</p>
					</div>
				</aside>
			</div>
		</section>
	);
}

function SubprocessorsSection() {
	return (
		<section
			className="border-b border-verevon-j-text/8 bg-verevon-surface-soft/40 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="underleverandorer"
		>
			<div className="mx-auto max-w-[1680px]">
				<div className="max-w-[760px]">
					<Eyebrow>Underleverandører</Eyebrow>
					<h2 className="verevon-h2 mt-6 text-balance">
						Hvem som kan få dataene dine — og statusen på hver enkelt.
					</h2>
					<p className="verevon-body-lg mt-7 max-w-[600px] text-pretty">
						Listen tar med leverandørene og mottakerne koden kan sende data til.
						Hvilke som er aktive, avhenger av oppsettet og funksjonene en
						organisasjon bruker, og produksjonsoppsettet er ikke dokumentert her.
						Der residens eller ZDR ikke er bekreftet ennå, står det «under
						avklaring» — ikke et grønt flagg. For mottakere uten oppgitt region har
						vi ikke bekreftet region eller databehandleravtale. Ikke tatt med er
						tilkoblinger og innlogging som kunden eller brukeren selv godkjenner på
						egen konto (blant annet Google, Microsoft, Apple, Vipps, Slack og
						Notion), offentlige kilder vi bare leser fra, tjenester som bare ser en
						besøkendes IP-adresse, og to små oppslag: stedsnavn til OpenStreetMap og
						en kort hash-prefiks til en passordsjekk.
					</p>
				</div>

				<div
					aria-label="Tabell over underleverandører og mottakere"
					className="mt-[clamp(40px,5vw,72px)] overflow-x-auto focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-verevon-j-text/40"
					role="region"
					tabIndex={0}
				>
					<table className="w-full min-w-[680px] border-collapse text-left">
						<caption className="sr-only">
							Underleverandører og mottakere, med formål, region og status
						</caption>
						<thead>
							<tr className="border-b border-verevon-j-text/16">
								<th className="verevon-eyebrow pb-4 pr-6 font-medium" scope="col">
									Leverandør
								</th>
								<th className="verevon-eyebrow pb-4 pr-6 font-medium" scope="col">
									Formål
								</th>
								<th className="verevon-eyebrow pb-4 pr-6 font-medium" scope="col">
									Region
								</th>
								<th className="verevon-eyebrow pb-4 font-medium" scope="col">
									Status
								</th>
							</tr>
						</thead>
						<tbody>
							{subprocessors.map((sub) => (
								<tr
									className="border-b border-verevon-j-text/10 align-top"
									key={sub.name}
								>
									<td className="py-5 pr-6">
										<span className="block font-manrope text-[1.05rem] font-normal leading-snug text-verevon-j-text">
											{sub.name}
										</span>
										<span className="mt-1.5 block font-geist text-[0.86rem] font-light leading-snug text-verevon-text-muted/85">
											{sub.note}
										</span>
									</td>
									<td className="py-5 pr-6 font-geist text-[0.92rem] font-light leading-snug text-verevon-text-muted">
										{sub.purpose}
									</td>
									<td className="py-5 pr-6 font-geist text-[0.92rem] font-light leading-snug text-verevon-text-muted">
										{sub.region}
									</td>
									<td className="py-5">
										<StatusBadge level={sub.level} />
									</td>
								</tr>
							))}
						</tbody>
					</table>
				</div>
			</div>
		</section>
	);
}

function ContactSection() {
	return (
		<section
			className="px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="kontakt-sikkerhet"
		>
			<div className="mx-auto grid max-w-[1680px] items-end gap-x-[clamp(40px,5vw,96px)] gap-y-10 xl:grid-cols-[minmax(0,1fr)_auto]">
				<div>
					<Eyebrow marker>Kontakt sikkerhet</Eyebrow>
					<h2 className="verevon-h2 mt-6 max-w-[20ch] text-balance">
						Trenger sikkerhets­teamet ditt mer? Ta kontakt.
					</h2>
					<p className="verevon-body-lg mt-7 max-w-[58ch] text-pretty">
						Databehandleravtale (DPA), Schrems&nbsp;II-vurdering og dataflyt-diagram
						er under utarbeidelse og ennå ikke gjennomgått av norsk
						personvernjurist. Ta kontakt, så sier vi hva som er klart — og vi
						holder deg oppdatert etter hvert som sertifiseringene lander.
					</p>
				</div>

				<div className="flex flex-col items-start gap-5 xl:items-end">
					<a
						className="font-manrope text-[clamp(1.3rem,1.6vw,1.9rem)] font-light tracking-[-0.04em] text-verevon-j-text underline-offset-[6px] transition-colors hover:text-verevon-coral-deep hover:underline"
						href={`mailto:${SECURITY_EMAIL}`}
					>
						{SECURITY_EMAIL}
					</a>
					<ArrowButton href={`mailto:${SECURITY_EMAIL}`} variant="coral">
						Kontakt sikkerhet
					</ArrowButton>
				</div>
			</div>
		</section>
	);
}

export function TrustCenter() {
	return (
		<main className="bg-background text-verevon-j-text">
			<TrustHero />
			<ProcurementEvidenceSection />
			<ControlsSection />
			<CertificationsSection />
			<ResidencySection />
			<SubprocessorsSection />
			<ContactSection />
		</main>
	);
}

export default TrustCenter;
