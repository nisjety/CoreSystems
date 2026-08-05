import { ArrowButton } from "@/components/ui/ArrowButton";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { StatusBadge, type StatusLevel } from "@/components/ui/StatusBadge";

type TrustPillar = {
	title: string;
	body: string;
	level: StatusLevel;
};

// Lead with the DEFENSIBLE, LIVE differentiators (VEREVON.md: "Where Verevon's
// trust wedge is real"). Levels are held to the honesty gate.
const pillars: TrustPillar[] = [
	{
		title: "EU som standard",
		body: "Chat, søk og agentoppgaver kjøres i Azure Sweden Central. EU er utgangspunktet, ikke et tillegg.",
		level: "live",
	},
	{
		title: "Promptene lagres ikke hos modellen",
		body: "Verevon håndhever Zero Data Retention i modell-laget, slik at ZDR-forespørsler ikke havner i varig prompt-cache.",
		level: "live",
	},
	{
		title: "Godkjenning per handling",
		body: "Risikofylte handlinger stopper ved Godkjenn eller Avvis. Mennesket sier ja før noe sendes, publiseres eller utføres.",
		level: "live",
	},
	{
		title: "Se hva AI-en brukte",
		body: "Tillitssenteret viser hvilke kilder og datakategorier AI-en har brukt, ikke bare en generell innsynsoversikt.",
		level: "live",
	},
	{
		title: "Dataklassifisering",
		body: "Data merkes etter risiko, slik at lagring, søk, deling og tredjepartsbehandling styres riktig.",
		level: "live",
	},
	{
		title: "Norskforankret",
		body: "Norske virksomheter kobles mot Enhetsregisteret, slik at Verevon jobber med ekte selskapsdata.",
		level: "live",
	},
];

function PillarCard({ pillar }: { pillar: TrustPillar }) {
	return (
		<div className="group relative flex flex-col gap-4 border-t border-verevon-j-text/12 pt-7">
			<div className="flex items-center justify-between gap-4">
				<h3 className="verevon-h3 max-w-[18ch] text-balance">{pillar.title}</h3>
				<StatusBadge level={pillar.level} />
			</div>

			<p className="verevon-body max-w-[42ch] text-pretty">{pillar.body}</p>
		</div>
	);
}

export function TrustBandSection() {
	return (
		<section
			aria-label="Tillit, sikkerhet og datasuverenitet"
			className="relative isolate overflow-hidden border-y border-verevon-j-text/8 bg-[linear-gradient(180deg,var(--verevon-trust-tint)_0%,var(--background)_46%)] px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] text-verevon-j-text max-[760px]:px-[var(--verevon-page-pad)]"
			id="trust"
		>
			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-0 z-0 bg-[radial-gradient(circle_at_82%_8%,rgba(238,122,80,0.10),transparent_34%)]"
			/>

			<div className="relative z-[1] mx-auto max-w-[1680px]">
				<div className="grid items-end gap-x-[clamp(40px,5vw,96px)] gap-y-10 xl:grid-cols-[minmax(0,1.05fr)_minmax(0,0.95fr)]">
					<SectionHeading
						eyebrow="Tillit og datasuverenitet"
						eyebrowMarker
						title={
							<>
								Bygget for virksomheter som ikke kan gå på akkord med hvor
								dataene ligger.
							</>
						}
							lede="Verevon gjør tillit konkret: data i EU som standard, godkjenning før risikofylte handlinger og et spor som viser hva AI-en faktisk brukte."
					/>

					<div className="flex flex-col items-start gap-6 xl:items-end xl:text-right">
						<p className="verevon-body max-w-[46ch] text-pretty xl:ml-auto">
							Vi viser hva som er live i dag, hva som er på vei, hvilke
							underleverandører som brukes og hvordan data flyter gjennom
							Verevon.
						</p>

						<ArrowButton href="/trust" variant="coral">
							Åpne tillitssenteret
						</ArrowButton>
					</div>
				</div>

				<div className="mt-[clamp(48px,6vw,88px)] grid gap-x-[clamp(32px,3.4vw,72px)] gap-y-[clamp(34px,3.6vw,56px)] sm:grid-cols-2 xl:grid-cols-3">
					{pillars.map((pillar) => (
						<PillarCard key={pillar.title} pillar={pillar} />
					))}
				</div>

				<p className="mt-[clamp(40px,4vw,64px)] max-w-[78ch] border-t border-verevon-j-text/8 pt-7 font-protokoll text-[var(--text-body-sm)] font-light leading-[1.5] text-verevon-text-muted/85">
					Vi lover ikke mer enn vi kan stå for: EU-residens er ikke det samme
					som full datasuverenitet. Derfor viser vi leverandører, dataflyt og
					restrisiko åpent i tillitssenteret.{" "}
					<a
						className="text-verevon-coral-deep underline-offset-4 hover:underline"
						href="/trust"
					>
						Les mer
					</a>
					.
				</p>
			</div>
		</section>
	);
}

export default TrustBandSection;
