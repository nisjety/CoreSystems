import Image from "next/image";
import { ArrowButtonLabel } from "@/components/ui/ArrowButton";
import { Eyebrow } from "@/components/ui/SectionHeading";
import { EditorialGrid } from "@/components/home/extras/EditorialGrid";
import { Reveal } from "@/components/home/sections/Reveal";
import { cn } from "@/lib/utils";

type IndexCard = {
	body: string;
	href: string;
	kicker: string;
	label: string;
	title: string;
};

type GalaxyImage = {
	alt: string;
	/** Larger, higher-opacity "anchor" photo vs. the small faded ambient shots. */
	anchor?: boolean;
	aspect: string;
	className: string;
	rotate: number;
	src: string;
	width: number;
};

// A restrained Wolverine-style "image galaxy" scattered around the credo
// statement's whitespace, curated against Velion's actual visual language
// (see public/velion-vibe/*: soft glossy renders, macro folds, warm glow on
// a muted palette — never literal business/office photography). The anchor
// is a lone door on a hillside — a threshold/decision image, not a stock
// "person in an office" cliché — and the small stills echo assets already in
// the brand set (a dune with a glowing orb ~ soft-orb.png + signal-ridge.png;
// a twisted ribbon render ~ glass-hour.png; the backlit bird IS warm-flight.png
// under its pre-curation filename). Desktop-only (max-[760px]:hidden below) —
// there isn't room to scatter imagery once the statement wraps to full width.
const galaxyImages: GalaxyImage[] = [
	{
		src: "/a95a19e05613baa759395e7dfc3241e5.jpg",
		alt: "",
		anchor: true,
		width: 176,
		aspect: "aspect-[3/4]",
		rotate: 2,
		className: "right-[4%] top-[1%] opacity-90",
	},
	{
		src: "/b41e54607ac9cc5f6424268082c19bec.jpg",
		alt: "",
		width: 108,
		aspect: "aspect-[3/4]",
		rotate: -4,
		className: "bottom-[1%] left-[2%] opacity-40",
	},
	{
		src: "/agent-run-console-running.jpg",
		alt: "",
		width: 78,
		aspect: "aspect-[3/4]",
		rotate: 5,
		className: "bottom-[10%] right-[16%] opacity-28",
	},
	{
		src: "/00631c87cef97a24798fbee7c406201d.jpg",
		alt: "",
		width: 60,
		aspect: "aspect-square",
		rotate: -7,
		className: "left-[9%] top-[5%] opacity-25",
	},
	{
		src: "/27aab72a25a11d3d63d1302d8d310515.jpg",
		alt: "",
		width: 66,
		aspect: "aspect-[3/4]",
		rotate: 6,
		className: "left-[38%] top-[14%] opacity-20",
	},
	{
		src: "/71fc97238af756817bf76c9ad6230a99.jpg",
		alt: "",
		width: 72,
		aspect: "aspect-[4/3]",
		rotate: -5,
		className: "bottom-[-2%] right-[33%] opacity-20",
	},
];

// Renders one galaxy photo as an absolutely positioned, faded thumbnail.
// Purely decorative (aria-hidden, no alt text needed) — the statement and
// cards below already carry the section's meaning for assistive tech.
function GalaxyThumb({ image }: { image: GalaxyImage }) {
	return (
		<div
			className={cn(
				"group-hover/galaxy:opacity-100 absolute overflow-hidden rounded-[2px] shadow-[0_18px_48px_rgba(23,23,23,0.12)] transition-opacity duration-700",
				image.aspect,
				image.className,
			)}
			style={{
				transform: `rotate(${image.rotate}deg)`,
				width: image.width,
			}}
		>
			<Image
				alt={image.alt}
				className={cn(
					"select-none object-cover",
					image.anchor
						? "saturate-[0.62] contrast-[1.02] sepia-[0.06]"
						: "saturate-[0.35] grayscale-[0.35]",
				)}
				draggable={false}
				fill
				sizes={`${image.width}px`}
				src={image.src}
			/>
		</div>
	);
}

// The 3 index cards below the credo — each one points into a later section
// (or /trust) instead of restating the thesis, so this section stays a
// single, uncluttered move: state the problem, then hand off.
const cards: IndexCard[] = [
	{
		kicker: "01",
		title: "Svar uten kilder",
		body: "Et svar uten synlig kilde er en gjetning med god selvtillit.",
		href: "#kunnskap",
		label: "Se hvordan kildene vises",
	},
	{
		kicker: "02",
		title: "Handling uten godkjenning",
		body: "Automatisering som sender selv, er en risiko ingen har bedt om.",
		href: "/trust",
		label: "Se godkjenningsmodellen",
	},
	{
		kicker: "03",
		title: "Kunnskap som ikke brukes",
		body: "Det som ligger i dokumenter og innbokser, hjelper ingen før det er koblet til arbeidet.",
		href: "#flyt",
		label: "Se arbeidsflyten",
	},
];

/**
 * ProblemSectionV3 — "the problem", stated in almost no words.
 *
 * Sits right after the brand-logos strip and before the senses/beliefs
 * section. Wolverine/fluid.glass big-statement pattern: a deliberate empty
 * "drumroll" before the credo, then the credo itself scattered with a small
 * image galaxy (one grounded institutional photo + a few faded abstract
 * stills) instead of sitting on plain white — no product screenshots. The
 * whole job here is to land the thesis, then index into the rest of the page
 * via the 3 cards below it.
 */
export function ProblemSectionV3() {
	return (
		<section
			className="relative isolate overflow-hidden bg-background px-[var(--velion-edge)] py-[var(--velion-section-vpad)] text-velion-j-text max-[760px]:px-[var(--velion-page-pad)]"
			id="problemet"
		>
			<EditorialGrid className="absolute inset-0 z-0" tone="light" />

			<div className="relative z-10">
				{/* Drumroll: eyebrow, then deliberate empty space before the
				    statement lands. The galaxy of scattered photos below lives
				    in that same whitespace instead of leaving it empty —
				    Wolverine's "portfolio" section device, restrained to one
				    grounded anchor photo plus a handful of faded abstract
				    stills, rather than the dozens Wolverine scatters. */}
				<div className="group/galaxy relative">
					<div
						aria-hidden="true"
						className="pointer-events-none absolute inset-0 z-0 max-[760px]:hidden"
					>
						{galaxyImages.map((image) => (
							<GalaxyThumb image={image} key={image.src} />
						))}
					</div>

					<div className="relative z-[1]">
						<Eyebrow marker>01 / Problemet</Eyebrow>

						<div
							aria-hidden="true"
							className="min-h-[30vh] max-[760px]:min-h-0"
						/>

						<h2 className="m-0 max-w-[14ch] font-arbeit text-[clamp(2.6rem,5.5vw,6.5rem)] font-light leading-[0.98] tracking-[-0.06em] text-velion-j-text text-balance">
							<Reveal>
								<span className="block">
									Svar er billige nå.
								</span>
							</Reveal>

							<Reveal delay={90}>
								<span className="block">
									Handling som kan godkjennes, er det som
									mangler.
								</span>
							</Reveal>
						</h2>

						<Reveal delay={180}>
							<p className="velion-body-lg mt-8 max-w-[640px] text-pretty">
								Alle kan koble på en språkmodell. Færre kan vise
								kilden, la et menneske si ja — og likevel handle
								i tide.
							</p>
						</Reveal>
					</div>
				</div>

				<div className="mt-[clamp(56px,7vw,104px)] grid grid-cols-3 gap-x-10 gap-y-10 max-[760px]:grid-cols-1">
					{cards.map((card) => (
						<a
							className={cn(
								"group block border-t border-velion-j-text/12 pt-7 transition-all duration-300",
								"hover:-translate-y-0.5 hover:border-velion-j-text/30",
							)}
							href={card.href}
							key={card.kicker}
						>
							<span className="font-protokoll text-[0.72rem] uppercase tracking-[0.14em] text-velion-j-text/40">
								{card.kicker}
							</span>

							<h3 className="mt-4 font-arbeit text-[clamp(1.5rem,2vw,2.1rem)] font-light leading-[1.05] tracking-[-0.03em] text-velion-j-text">
								{card.title}
							</h3>

							<p className="velion-body mt-3 max-w-[34ch] text-pretty">
								{card.body}
							</p>

							<ArrowButtonLabel className="mt-5">
								{card.label}
							</ArrowButtonLabel>
						</a>
					))}
				</div>
			</div>
		</section>
	);
}

export default ProblemSectionV3;
