import Image from "next/image";
import Link from "next/link";
import { ArrowButtonLabel } from "@/components/ui/ArrowButton";
import { FeatureCardFilm } from "./FeatureCardFilms";
// The card copy itself lives in a framework-free sibling module so it can be
// unit-tested without React / next/image in the module graph. Re-exported here
// so existing `from "./FeatureWorkflowCards"` call sites keep working.
import type { ModuleCard, WorkflowCard } from "./feature-workflow-cards";
import { moduleCards, workflowCards } from "./feature-workflow-cards";

export type { ModuleCard, WorkflowCard };
export { moduleCards, workflowCards };

function ModuleVisual({ card }: { card: ModuleCard }) {
	// Account for the full image width needed to cover the portrait frame.
	// A landscape source needs more pixels than the visible card width alone.
	const coverScale = Math.max(1, (card.imageWidth / card.imageHeight) / (4 / 5));
	const imageSizes = `(max-width: 767px) ${Math.ceil(78 * coverScale)}vw, (max-width: 1140px) ${Math.ceil(88 * coverScale)}vw, ${Math.ceil(32 * coverScale)}vw`;

	return (
		<>
			<Image
				alt={card.imageAlt}
				className="object-cover brightness-[0.94] saturate-[0.86] contrast-[1.05] transition-[transform,filter] duration-[900ms] ease-out group-hover:scale-[1.045] group-hover:brightness-100 group-hover:saturate-100 motion-reduce:transition-none motion-reduce:group-hover:scale-100"
				fill
				quality={90}
				sizes={imageSizes}
				src={card.image}
				style={{ objectPosition: card.imagePosition ?? "center" }}
			/>
			<div aria-hidden="true" className="absolute inset-0 bg-[linear-gradient(180deg,rgba(6,13,17,0.08),transparent_38%,rgba(5,10,13,0.28))]" />
		</>
	);
}

/** Product-area card used by the public platform carousel. */
export function ModuleWorkflowCard({
	card,
	index,
	total,
	animated = true,
	className = "",
}: {
	card: ModuleCard;
	index: number;
	total: number;
	animated?: boolean;
	className?: string;
}) {
	const titleId = `module-card-${index}-title`;

	return (
		<Link
			aria-labelledby={titleId}
			className={`verevon-cta-card group flex min-w-0 cursor-pointer flex-col gap-3 no-underline outline-none focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-verevon-coral${animated ? " md:invisible" : ""} ${className}`}
			data-feature-index={index}
			data-module-card-link=""
			href={card.href}
			{...(animated ? { "data-feature-card": "" } : {})}
		>
			<div className="flex h-[15px] select-none items-center justify-between font-geist text-[0.6875rem] font-medium uppercase leading-none tracking-[0.08em] text-verevon-j-text/42">
				<span>{card.area}</span>
				<span>{`0${index + 1} / ${String(total).padStart(2, "0")}`}</span>
			</div>

			<div className="relative aspect-[4/5] overflow-hidden bg-verevon-j-text/[0.04]">
				<ModuleVisual card={card} />
			</div>

			<h3 className="m-0 max-w-full break-words font-manrope text-[clamp(1.18rem,1.42vw,1.6rem)] font-light leading-[1.06] tracking-[-0.05em] text-verevon-j-text" id={titleId}>
				{card.title}
			</h3>

			<p className="m-0 font-manrope text-[clamp(0.9375rem,0.9vw,1rem)] font-light leading-[1.5] tracking-[-0.015em] text-verevon-text-muted/90">
				{card.text}
			</p>

			<span className="verevon-cta mt-auto text-verevon-j-text">
				<ArrowButtonLabel>{card.linkLabel}</ArrowButtonLabel>
			</span>
		</Link>
	);
}

export function FeatureWorkflowCard({
	active = true,
	animated = true,
	card,
	index,
	total = 4,
	className = "",
}: {
	active?: boolean;
	animated?: boolean;
	card: WorkflowCard;
	index: number;
	total?: number;
	className?: string;
}) {
	return (
		<a
			aria-label={`${card.title}: ${card.text}`}
			className={`group flex min-w-0 flex-col gap-3${animated ? " md:invisible" : ""} ${className}`}
			data-feature-active={active ? "true" : "false"}
			data-feature-index={index}
			href={card.href}
			{...(animated ? { "data-feature-card": "" } : {})}
		>
			<div className="flex h-[15px] select-none items-center justify-between font-geist text-[0.6875rem] font-medium uppercase leading-none tracking-[0.08em] text-verevon-j-text/42">
				<span className="size-[5px] rounded-full bg-verevon-coral/70" />
				<span>{`0${index + 1} / ${String(total).padStart(2, "0")}`}</span>
			</div>

			<div className="relative aspect-[4/5] overflow-hidden bg-transparent">
				<FeatureCardFilm kind={card.film} />
			</div>

			<h3 className="m-0 max-w-full break-words font-manrope text-[clamp(1.25rem,1.55vw,1.75rem)] font-light leading-[1.04] tracking-[-0.055em] text-verevon-j-text">
				{card.title}
			</h3>

			<p className="m-0 font-geist text-[0.6875rem] font-medium uppercase tracking-[0.08em] text-verevon-coral/80">
				{card.kicker}
			</p>

			<div
				aria-label={`${card.kicker}: ${card.features.join(", ")}`}
				className="flex flex-wrap gap-1.5"
			>
				{card.features.map((feature) => (
					<span
						className="rounded-full border border-verevon-j-text/10 px-2 py-1 font-geist text-[0.6875rem] font-medium uppercase tracking-[0.08em] text-verevon-j-text/58"
						key={feature}
					>
						{feature}
					</span>
				))}
			</div>

			<p className="m-0 font-manrope text-[clamp(0.9375rem,0.9vw,1rem)] font-light leading-[1.5] tracking-[-0.015em] text-verevon-text-muted/90">
				{card.text}
			</p>
		</a>
	);
}
