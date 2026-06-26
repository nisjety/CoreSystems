import { ArrowButton } from "@/components/ui/ArrowButton";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { StatusBadge, type StatusLevel } from "@/components/ui/StatusBadge";

type TrustPillar = {
	title: string;
	body: string;
	level: StatusLevel;
};

// Lead with the DEFENSIBLE, LIVE differentiators (VELION.md: "Where Velion's
// trust wedge is real"). Levels are held to the honesty gate.
const pillars: TrustPillar[] = [
	{
		title: "EU-residens som standard",
		body: "Inferens og embeddings kjører på Azure OpenAI Sweden Central (EU/EØS). Ikke «EU-region på forespørsel» — det er utgangspunktet.",
		level: "live",
	},
	{
		title: "Zero Data Retention som standard",
		body: "ZDR håndheves i koden på modell-laget: en ZDR-forespørsel treffer aldri den varige prompt-cachen. Ikke en kontraktsklausul i etterkant.",
		level: "live",
	},
	{
		title: "Godkjenning per handling",
		body: "Hver risikofylt verktøy­handling stopper ved en Godkjenn/Avvis-port. Mennesket sier ja før noe sendes, publiseres eller utføres.",
		level: "live",
	},
	{
		title: "«Brukt av AI?»-revisjonsspor",
		body: "Et revisjonsspor på tvers av planene viser hvilke data AI-en har rørt, per kategori — konkret styring, ikke en generisk «innsyn»-flis.",
		level: "live",
	},
	{
		title: "Dataklassifisering",
		body: "Et taksonomi i seks klasser styrer lagring, indeksering og deling, med default-deny tredjeparts­behandling for beskyttede klasser.",
		level: "live",
	},
	{
		title: "Norskforankret",
		body: "Entiteter forankres i Enhetsregisteret (Brreg) — ikke en hallusinert global oppføring. Bokmål-grensesnitt rulles ut gradvis.",
		level: "live",
	},
];

function PillarCard({ pillar }: { pillar: TrustPillar }) {
	return (
		<div className="group relative flex flex-col gap-4 border-t border-velion-j-text/12 pt-7">
			<div className="flex items-center justify-between gap-4">
				<h3 className="velion-h3 max-w-[18ch] text-balance">{pillar.title}</h3>
				<StatusBadge level={pillar.level} />
			</div>

			<p className="velion-body max-w-[42ch] text-pretty">{pillar.body}</p>
		</div>
	);
}

export function TrustBandSection() {
	return (
		<section
			aria-label="Tillit, sikkerhet og datasuverenitet"
			className="relative isolate overflow-hidden border-y border-velion-j-text/8 bg-[linear-gradient(180deg,var(--velion-trust-tint)_0%,var(--background)_46%)] px-[var(--velion-edge)] py-[var(--velion-section-vpad)] text-velion-j-text max-[760px]:px-[var(--velion-page-pad)]"
			id="trust"
		>
			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-0 z-0 bg-[radial-gradient(circle_at_82%_8%,rgba(238,122,80,0.10),transparent_34%)]"
			/>

			<div className="relative z-[1] mx-auto max-w-[1680px]">
				<div className="grid items-end gap-x-[clamp(40px,5vw,96px)] gap-y-10 xl:grid-cols-[minmax(0,1.05fr)_minmax(0,0.95fr)]">
					<SectionHeading
						eyebrow="Tillit & datasuverenitet"
						eyebrowMarker
						title={
							<>
								Bygget for virksomheter som ikke kan gå på akkord med hvor
								dataene ligger.
							</>
						}
						lede="Den late påstanden «amerikanske leverandører lekker dataene dine» holder ikke mot en moden konkurrent — og Velion kjører selv på Azure. Derfor er forspranget skarpere: håndhevet i koden, som standard, og forankret i Norge."
					/>

					<div className="flex flex-col items-start gap-6 xl:items-end xl:text-right">
						<p className="velion-body max-w-[46ch] text-pretty xl:ml-auto">
							Vi har kontrollene — men ennå ikke tredjeparts­sertifiseringene.
							Det er vi åpne om. Trust Center viser hva som er live i dag,
							sertifiserings­løypa (ISO&nbsp;42001, EU&nbsp;AI&nbsp;Act, SOC&nbsp;2),
							underleverandører og dataflyt.
						</p>

						<ArrowButton href="/trust" variant="coral">
							Åpne Trust Center
						</ArrowButton>
					</div>
				</div>

				<div className="mt-[clamp(48px,6vw,88px)] grid gap-x-[clamp(32px,3.4vw,72px)] gap-y-[clamp(34px,3.6vw,56px)] sm:grid-cols-2 xl:grid-cols-3">
					{pillars.map((pillar) => (
						<PillarCard key={pillar.title} pillar={pillar} />
					))}
				</div>

				<p className="mt-[clamp(40px,4vw,64px)] max-w-[78ch] border-t border-velion-j-text/8 pt-7 font-protokoll text-[var(--text-body-sm)] font-light leading-[1.5] text-velion-text-muted/85">
					Åpenhet, ikke immunitet: EU-residens er ikke det samme som
					datasuverenitet. En leverandør med hovedkontor i USA — også Microsoft
					Azure — kan nås under CLOUD Act uavhengig av fysisk plassering. Vi
					logger dette som en åpent oppgitt restrisiko i Schrems&nbsp;II-vurderingen,
					aldri som immunitet.{" "}
					<a
						className="text-velion-coral-deep underline-offset-4 hover:underline"
						href="/trust"
					>
						Les hele vurderingen
					</a>
					.
				</p>
			</div>
		</section>
	);
}

export default TrustBandSection;
