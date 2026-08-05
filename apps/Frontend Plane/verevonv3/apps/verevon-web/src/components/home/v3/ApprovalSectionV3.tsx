import { Check } from "lucide-react";
import Image from "next/image";
import { Reveal } from "@/components/home/sections/Reveal";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { Eyebrow } from "@/components/ui/SectionHeading";
import { StatusBadge, type StatusLevel } from "@/components/ui/StatusBadge";

type TrustPillar = {
	title: string;
	body: string;
	level: StatusLevel;
};

// Lead with the DEFENSIBLE, LIVE differentiators (VEREVON.md: "Where Verevon's
// trust wedge is real"). Levels are held to the honesty gate. Copied verbatim
// from TrustBandSection.tsx — see that file for the source of truth.
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

// The 4 hard boundaries. "Ingen" is the honesty-gate word, so it carries the
// coral accent — the rest of each line stays plain body copy.
const noListItems = [
	"Ingen utsendelse uten godkjenning.",
	"Ingen svar uten kilde.",
	"Ingen trening på deres data.",
	"Ingen data utenfor EU.",
];

const auditTrailEntries = [
	"Godkjent av Jonas · i går 14:12",
	"Godkjent av Mia · i går 09:40",
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

function ApprovalField({ label, value }: { label: string; value: string }) {
	return (
		<div className="flex flex-col gap-1.5">
			<dt className="font-protokoll text-[0.7rem] font-medium uppercase tracking-[0.1em] text-verevon-j-text/42">
				{label}
			</dt>
			<dd className="m-0 font-protokoll text-[0.94rem] font-light leading-[1.4] text-verevon-j-text">
				{value}
			</dd>
		</div>
	);
}

/**
 * ApprovalQueueCard — a decorative, non-functional mock of a single item in
 * Verevon's approval queue. Purely illustrative: nothing here is wired, so
 * every control is inert (matching the VerevonComposerPreview convention of
 * type="button" + tabIndex={-1} on decorative controls).
 */
function ApprovalQueueCard() {
	return (
		<div
			aria-label="Eksempel: en handling som venter på godkjenning"
			className="relative w-full max-w-[480px] border border-verevon-j-text/10 bg-white/52 p-[clamp(22px,2.6vw,32px)] shadow-[0_28px_72px_rgba(23,23,23,0.08)] backdrop-blur-[14px]"
		>
			<div className="flex items-center justify-between gap-3">
				<span className="font-protokoll text-[0.68rem] uppercase tracking-[0.16em] text-verevon-j-text/42">
					Venter på godkjenning
				</span>
				<span
					aria-hidden="true"
					className="size-1.5 shrink-0 rounded-full bg-verevon-coral motion-safe:animate-pulse"
				/>
			</div>

			<dl className="mt-6 flex flex-col gap-5">
				<ApprovalField
					label="Hva skjer"
					value="Svar til kunde om forsinket leveranse (ordre 44210)"
				/>
				<ApprovalField
					label="Hvorfor"
					value="Ny leveringsdato funnet i oppdatert fraktplan"
				/>
				<ApprovalField
					label="Hvem må godkjenne"
					value="Mia Solberg · Kundeservice"
				/>
			</dl>

			<div className="mt-7 flex flex-wrap items-center gap-3">
				<button
					className="rounded-full bg-verevon-coral px-5 py-2.5 font-protokoll text-[0.86rem] font-medium text-white"
					tabIndex={-1}
					type="button"
				>
					Godkjenn og send
				</button>
				<button
					className="rounded-full border border-verevon-j-text/20 px-5 py-2.5 font-protokoll text-[0.86rem] font-medium text-verevon-j-text"
					tabIndex={-1}
					type="button"
				>
					Avvis
				</button>
			</div>

			<div className="mt-6 border-t border-verevon-j-text/10 pt-5">
				<h4 className="font-protokoll text-[0.7rem] font-medium uppercase tracking-[0.1em] text-verevon-j-text/42">
					Revisjonsspor
				</h4>

				<ul className="mt-3 flex flex-col gap-2">
					{auditTrailEntries.map((entry) => (
						<li
							className="flex items-center gap-2 font-protokoll text-[0.86rem] font-light text-verevon-j-text/72"
							key={entry}
						>
							<Check className="size-3.5 shrink-0 text-verevon-coral" />
							{entry}
						</li>
					))}
				</ul>
			</div>
		</div>
	);
}

/**
 * ApprovalSectionV3 — the honest replacement for the fake-testimonial block
 * on the old V1 homepage. Nothing here is invented: the headline/lede are
 * copied verbatim from DetailGallerySection.tsx, the trust pillars and
 * closing paragraph are copied verbatim from TrustBandSection.tsx, and the
 * approval-queue card is clearly a decorative mock (inert controls), never
 * presented as a real customer quote or certification.
 */
export function ApprovalSectionV3() {
	return (
		<section
			className="relative isolate overflow-hidden border-y border-verevon-j-text/8 bg-[linear-gradient(180deg,var(--verevon-trust-tint)_0%,var(--background)_46%)] px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] text-verevon-j-text max-[760px]:px-[var(--verevon-page-pad)]"
			id="trust"
		>
			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-0 z-0 opacity-[0.08]"
			>
				<Image
					alt=""
					className="object-cover object-center"
					fill
					sizes="100vw"
					src="/glass-edge.png"
				/>
			</div>

			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-0 z-0 bg-[radial-gradient(circle_at_82%_8%,rgba(238,122,80,0.10),transparent_34%)]"
			/>

			<div className="relative z-[1] mx-auto max-w-[1680px]">
				<div className="grid items-start gap-x-[clamp(40px,5vw,96px)] gap-y-14 xl:grid-cols-[minmax(0,1.05fr)_minmax(0,0.95fr)]">
					<div className="max-w-[760px]">
						<Eyebrow marker>05 / Godkjenning</Eyebrow>

						<h2 className="m-0 mt-6 max-w-[820px] font-arbeit text-[clamp(3.35rem,6.4vw,8.6rem)] font-light leading-[0.9] tracking-[-0.078em] text-verevon-j-text text-balance">
							<Reveal>
								<span className="block">Godkjenningen er en del av produktet.</span>
							</Reveal>
						</h2>

						<Reveal delay={90}>
							<p className="mt-[clamp(28px,3vw,46px)] max-w-[560px] font-protokoll text-[clamp(1.02rem,1.05vw,1.22rem)] font-light leading-[1.5] text-verevon-text-muted text-pretty">
								Verevon kan foreslå arbeid på tvers av kundeservice, kunnskap og
								drift. Men handlinger med risiko stopper for vurdering, med
								kilder, kontekst og sporbarhet ved siden av.
							</p>
						</Reveal>
					</div>

					<div className="relative mx-auto w-full max-w-[480px] xl:mx-0 xl:ml-auto">
						<span
							aria-hidden="true"
							className="absolute -inset-3 -z-10 rotate-[-1.4deg] border border-verevon-j-text/8 bg-white/30 backdrop-blur-[10px] max-[760px]:hidden"
						/>

						<Reveal delay={140}>
							<ApprovalQueueCard />
						</Reveal>
					</div>
				</div>

				<Reveal delay={200}>
					<ul className="mt-[clamp(48px,6vw,88px)] flex max-w-[560px] flex-col gap-3">
						{noListItems.map((item) => (
							<li className="verevon-body-lg text-pretty" key={item}>
								<span className="text-verevon-coral">Ingen</span>
								{item.slice("Ingen".length)}
							</li>
						))}
					</ul>
				</Reveal>

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

				<div className="mt-[clamp(30px,3.8vw,52px)]">
					<ArrowButton href="/trust" variant="coral">
						Åpne tillitssenteret
					</ArrowButton>
				</div>
			</div>
		</section>
	);
}

export default ApprovalSectionV3;
