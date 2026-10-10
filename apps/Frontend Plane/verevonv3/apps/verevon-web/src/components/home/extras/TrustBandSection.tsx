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
		title: "EU-region på standardmodellene",
		body: "Standardmodellene kjører på Azure med regionen satt til Sweden Central. Noen ruter har ikke den regionen: Claude via Azure AI Foundry, søk via eksterne søkemotorer og ChatGPT-modeller du selv kobler til. Trust Center viser hvilke.",
		level: "progress",
	},
	{
		title: "Zero Data Retention når du slår det på",
		body: "Når ZDR er på, sender Verevon forespørselen bare til en leverandør med registrert ZDR-godkjenning, og avviser den ellers. ZDR-forespørsler skrives aldri til vår egen inferens-cache.",
		level: "progress",
	},
	{
		title: "Godkjenning per handling",
		body: "Utplasserte agenter stopper risikofylte handlinger ved Godkjenn eller Avvis, før noe sendes, publiseres eller utføres. I vanlig chat finnes ingen slik port.",
		level: "live",
	},
	{
		title: "Se hva AI-en brukte",
		body: "Tillitssenteret i produktet er bygget for å vise hvilke verktøy og datakategorier AI-en har brukt per tilkoblet app. Sporet er ikke verifisert ende til ende ennå.",
		level: "progress",
	},
	{
		title: "Dataklassifisering",
		body: "Data merkes etter risiko. Data Plane nekter å sende ZDR-merket innhold til eksterne embeddings; i chat styrer klassen ennå ikke valg av leverandør.",
		level: "progress",
	},
	{
		title: "Norskforankret",
		body: "Norske virksomheter kan slås opp i Enhetsregisteret gjennom et eget verktøy.",
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
			aria-label="Tillit, sikkerhet og dataflyt"
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
						eyebrow="Tillit og dataflyt"
						eyebrowMarker
						title={
							<>
								Bygget for virksomheter som må vite hvor dataene deres
								går.
							</>
						}
							lede="Verevon gjør tillit konkret: EU-region på standardmodellene, godkjenning før risikofylte handlinger for utplasserte agenter og et spor som er bygget for å vise hva AI-en brukte."
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

				<p className="mt-[clamp(40px,4vw,64px)] max-w-[78ch] border-t border-verevon-j-text/8 pt-7 font-geist text-[var(--text-body-sm)] font-light leading-[1.5] text-verevon-text-muted/85">
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
