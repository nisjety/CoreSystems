import { ArrowButton } from "@/components/ui/ArrowButton";
import { Eyebrow } from "@/components/ui/SectionHeading";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
	certifications,
	dataClasses,
	liveControls,
	subprocessors,
} from "./trust-data";

const SECURITY_EMAIL = "sikkerhet@velion.ai";

function TrustHero() {
	return (
		<section
			className="relative isolate overflow-hidden border-b border-velion-j-text/8 bg-[linear-gradient(180deg,var(--velion-trust-tint)_0%,var(--background)_60%)] px-[var(--velion-edge)] pb-[clamp(72px,8vw,120px)] pt-[clamp(72px,10vh,128px)] max-[760px]:px-[var(--velion-page-pad)]"
			id="top"
		>
			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-0 z-0 bg-[radial-gradient(circle_at_84%_-10%,rgba(238,122,80,0.12),transparent_38%)]"
			/>

			<div className="relative z-[1] mx-auto max-w-[1680px]">
				<Eyebrow marker>Trust Center</Eyebrow>

				<h1 className="mt-7 max-w-[18ch] font-arbeit text-[clamp(2.7rem,5.2vw,5.6rem)] font-light leading-[0.96] tracking-[-0.06em] text-velion-j-text text-balance">
					Sikkerhet, personvern og AI-styring.
				</h1>

				<p className="mt-8 max-w-[64ch] font-protokoll text-[var(--text-body-lg)] font-light leading-[1.5] text-velion-text-muted text-pretty">
					Velion er bygget for virksomheter som ikke kan gå på akkord med hvor
					dataene ligger. Her er den ærlige tilstanden: kontrollene er reelle og
					stort sett live i dag — men vi holder ennå ingen tredjeparts­sertifiseringer.
					Vi viser begge deler, uten pynt.
				</p>

				<div className="mt-10 flex flex-wrap items-center gap-x-9 gap-y-4">
					<div className="flex flex-wrap items-center gap-3">
						<StatusBadge level="live" label="Kontrollene er live" />
						<StatusBadge level="planned" label="Sertifiseringer på vei" />
					</div>

					<ArrowButton href={`mailto:${SECURITY_EMAIL}`} variant="coral">
						Be om sikkerhets­pakken
					</ArrowButton>
				</div>
			</div>
		</section>
	);
}

function ControlsSection() {
	return (
		<section
			className="border-b border-velion-j-text/8 px-[var(--velion-edge)] py-[var(--velion-section-vpad)] max-[760px]:px-[var(--velion-page-pad)]"
			id="kontroller"
		>
			<div className="mx-auto max-w-[1680px]">
				<div className="max-w-[760px]">
					<Eyebrow>Det vi kan stå for</Eyebrow>
					<h2 className="velion-h2 mt-6 text-balance">
						De forsvarbare forskjellene — håndhevet, ikke lovet.
					</h2>
					<p className="velion-body-lg mt-7 max-w-[600px] text-pretty">
						Den late påstanden «amerikanske leverandører lekker dataene dine»
						holder ikke mot en moden konkurrent — og Velion kjører selv på Azure.
						Derfor er forspranget vårt skarpere og mer konkret.
					</p>
				</div>

				<div className="mt-[clamp(48px,6vw,80px)] grid gap-x-[clamp(32px,3.4vw,72px)] gap-y-[clamp(34px,3.6vw,52px)] sm:grid-cols-2 xl:grid-cols-3">
					{liveControls.map((control) => (
						<div
							className="flex flex-col gap-4 border-t border-velion-j-text/12 pt-7"
							key={control.title}
						>
							<div className="flex items-start justify-between gap-4">
								<h3 className="velion-h3 max-w-[20ch] text-balance">
									{control.title}
								</h3>
								<StatusBadge level={control.level} />
							</div>
							<p className="velion-body text-pretty">{control.body}</p>
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
			className="border-b border-velion-j-text/8 bg-velion-surface-soft/40 px-[var(--velion-edge)] py-[var(--velion-section-vpad)] max-[760px]:px-[var(--velion-page-pad)]"
			id="sertifiseringer"
		>
			<div className="mx-auto max-w-[1680px]">
				<div className="max-w-[820px]">
					<Eyebrow>Sertifiseringer & løype</Eyebrow>
					<h2 className="velion-h2 mt-6 text-balance">
						Vi har kontrollene — men ennå ikke sertifiseringene.
					</h2>
					<p className="velion-body-lg mt-7 max-w-[640px] text-pretty">
						Det er den ubehagelige sannheten en tillitsledet posisjon må eie. På
						innkjøps­modent papir kan en Vanta-støttet konkurrent med SOC&nbsp;2 og
						ISO&nbsp;42001 i dag dokumentere mer enn oss. Å lukke gapet er en
						prioritet — og vi leder an på{" "}
						<strong className="font-medium text-velion-j-text">
							ISO&nbsp;42001 og EU&nbsp;AI&nbsp;Act
						</strong>{" "}
						som den suverene AI-spydspissen.
					</p>
				</div>

				<ul className="mt-[clamp(40px,5vw,72px)] flex flex-col">
					{certifications.map((cert) => (
						<li
							className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-6 gap-y-3 border-t border-velion-j-text/12 py-6 last:border-b sm:grid-cols-[minmax(220px,0.9fr)_auto_minmax(0,1.6fr)] sm:items-center"
							key={cert.name}
						>
							<h3 className="velion-h3 text-[clamp(1.2rem,1.3vw,1.6rem)]">
								{cert.name}
							</h3>
							<div className="row-start-1 justify-self-end sm:row-start-auto sm:justify-self-start">
								<StatusBadge level={cert.level} />
							</div>
							<p className="col-span-2 font-protokoll text-[var(--text-body-sm)] font-light leading-[1.5] text-velion-text-muted text-pretty sm:col-span-1">
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
			className="border-b border-velion-j-text/8 px-[var(--velion-edge)] py-[var(--velion-section-vpad)] max-[760px]:px-[var(--velion-page-pad)]"
			id="dataflyt"
		>
			<div className="mx-auto grid max-w-[1680px] gap-x-[clamp(40px,5vw,96px)] gap-y-12 xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
				<div>
					<Eyebrow marker>Datalagring & dataflyt</Eyebrow>
					<h2 className="velion-h2 mt-6 text-balance">
						Hvor dataene dine ligger — og hvor de ikke gjør det.
					</h2>
					<p className="velion-body-lg mt-7 max-w-[52ch] text-pretty">
						App-data, samtaler, kjørehistorikk, revisjon og vektorlageret er
						EU/EØS-lagre under Velions kontroll. Primær inferens og embeddings
						kjører på Azure OpenAI Sweden Central. Tale (TTS) står på EU-endepunktet
						og nekter et konfigurert ikke-EU-endepunkt med mindre det eksplisitt
						åpnes.
					</p>

					<div className="mt-9">
						<p className="velion-eyebrow mb-5">Dataklassifisering — seks klasser</p>
						<ul className="flex flex-wrap gap-2.5">
							{dataClasses.map((klass) => (
								<li
									className="rounded-full border border-velion-j-text/14 bg-velion-surface px-3.5 py-1.5 font-protokoll text-[0.82rem] font-light text-velion-j-text/72"
									key={klass}
								>
									{klass}
								</li>
							))}
						</ul>
						<p className="velion-body mt-5 max-w-[52ch] text-pretty">
							Default-deny tredjeparts­behandling for beskyttede klasser.
							GDPR-policy­metadata (formål, lovlig grunnlag, lagringstid, residens,
							personvern­klasse, sletteomfang) følger varige poster.
						</p>
					</div>
				</div>

				<aside className="flex flex-col gap-7">
					<div className="rounded-[var(--radius-lg)] border border-velion-trust-line/60 bg-velion-trust-tint/70 p-[clamp(24px,2.6vw,40px)]">
						<div className="mb-4 flex items-center gap-3">
							<StatusBadge level="progress" label="Åpent oppgitt" />
						</div>
						<h3 className="font-arbeit text-[clamp(1.35rem,1.7vw,2rem)] font-light leading-[1.06] tracking-[-0.04em] text-velion-j-text">
							Åpenhet, ikke immunitet: CLOUD Act
						</h3>
						<p className="velion-body mt-5 text-pretty">
							EU-residens er ikke det samme som datasuverenitet. En
							leverandør med hovedkontor i USA — også Microsoft Azure — kan nås
							under den amerikanske CLOUD Act uavhengig av fysisk plassering. Vi
							hevder aldri at residens gir immunitet.
						</p>
						<p className="velion-body mt-4 text-pretty">
							Dette logges som en åpent oppgitt restrisiko i vår
							Schrems&nbsp;II-overførings­vurdering, med supplerende tiltak. Vi sier
							det rett ut framfor å pakke det bort.
						</p>
					</div>

					<div className="rounded-[var(--radius-lg)] border border-velion-j-text/10 bg-velion-surface p-[clamp(24px,2.6vw,40px)]">
						<h3 className="font-arbeit text-[clamp(1.2rem,1.4vw,1.6rem)] font-light leading-[1.1] tracking-[-0.04em] text-velion-j-text">
							ZDR — ærlig omfang
						</h3>
						<p className="velion-body mt-5 text-pretty">
							Zero Data Retention binder modell-leverandøren (ingen trening eller
							lagring på prompts). Velion beholder fortsatt egen kjørehistorikk,
							samtaler og revisjon etter sin lagringsplan. Kun Azure OpenAI
							Sweden Central-stien er bekreftet ZDR&nbsp;+&nbsp;EØS i dag — andre
							leverandører er under avklaring.
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
			className="border-b border-velion-j-text/8 bg-velion-surface-soft/40 px-[var(--velion-edge)] py-[var(--velion-section-vpad)] max-[760px]:px-[var(--velion-page-pad)]"
			id="underleverandorer"
		>
			<div className="mx-auto max-w-[1680px]">
				<div className="max-w-[760px]">
					<Eyebrow>Underleverandører</Eyebrow>
					<h2 className="velion-h2 mt-6 text-balance">
						En kort, EU-først liste — og statusen på hver enkelt.
					</h2>
					<p className="velion-body-lg mt-7 max-w-[600px] text-pretty">
						Vi holder lista stram. Der residens eller ZDR ikke er bekreftet ennå,
						står det «under avklaring» — ikke et grønt flagg.
					</p>
				</div>

				<div className="mt-[clamp(40px,5vw,72px)] overflow-x-auto">
					<table className="w-full min-w-[680px] border-collapse text-left">
						<thead>
							<tr className="border-b border-velion-j-text/16">
								<th className="velion-eyebrow pb-4 pr-6 font-medium">
									Leverandør
								</th>
								<th className="velion-eyebrow pb-4 pr-6 font-medium">Formål</th>
								<th className="velion-eyebrow pb-4 pr-6 font-medium">Region</th>
								<th className="velion-eyebrow pb-4 font-medium">Status</th>
							</tr>
						</thead>
						<tbody>
							{subprocessors.map((sub) => (
								<tr
									className="border-b border-velion-j-text/10 align-top"
									key={sub.name}
								>
									<td className="py-5 pr-6">
										<span className="block font-arbeit text-[1.05rem] font-normal leading-snug text-velion-j-text">
											{sub.name}
										</span>
										<span className="mt-1.5 block font-protokoll text-[0.86rem] font-light leading-snug text-velion-text-muted/85">
											{sub.note}
										</span>
									</td>
									<td className="py-5 pr-6 font-protokoll text-[0.92rem] font-light leading-snug text-velion-text-muted">
										{sub.purpose}
									</td>
									<td className="py-5 pr-6 font-protokoll text-[0.92rem] font-light leading-snug text-velion-text-muted">
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
			className="px-[var(--velion-edge)] py-[var(--velion-section-vpad)] max-[760px]:px-[var(--velion-page-pad)]"
			id="kontakt-sikkerhet"
		>
			<div className="mx-auto grid max-w-[1680px] items-end gap-x-[clamp(40px,5vw,96px)] gap-y-10 xl:grid-cols-[minmax(0,1fr)_auto]">
				<div>
					<Eyebrow marker>Be om tilgang</Eyebrow>
					<h2 className="velion-h2 mt-6 max-w-[20ch] text-balance">
						Trenger sikkerhets­teamet ditt mer? Be om pakken.
					</h2>
					<p className="velion-body-lg mt-7 max-w-[58ch] text-pretty">
						Vi deler databehandler­avtale (DPA), Schrems&nbsp;II-vurdering,
						dataflyt-diagram og sikkerhets­arkitektur på forespørsel — og holder
						deg oppdatert etter hvert som sertifiseringene lander.
					</p>
				</div>

				<div className="flex flex-col items-start gap-5 xl:items-end">
					<a
						className="font-arbeit text-[clamp(1.3rem,1.6vw,1.9rem)] font-light tracking-[-0.04em] text-velion-j-text underline-offset-[6px] transition-colors hover:text-velion-coral-deep hover:underline"
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
		<main className="bg-background text-velion-j-text">
			<TrustHero />
			<ControlsSection />
			<CertificationsSection />
			<ResidencySection />
			<SubprocessorsSection />
			<ContactSection />
		</main>
	);
}

export default TrustCenter;
