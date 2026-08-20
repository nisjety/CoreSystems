import Image from "next/image";
import { FeatureCardFilm } from "./FeatureCardFilms";
// The card copy itself lives in a framework-free sibling module so it can be
// unit-tested without React / next/image in the module graph. Re-exported here
// so existing `from "./FeatureWorkflowCards"` call sites keep working.
import type { ModuleCard, WorkflowCard } from "./feature-workflow-cards";
import { moduleCards, workflowCards } from "./feature-workflow-cards";

export type { ModuleCard, WorkflowCard };
export { moduleCards, workflowCards };


/** Photographic module card used by the public platform carousel. */
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
	return (
		<a
			aria-label={`${card.module}: ${card.title}`}
			className={`group flex min-w-0 flex-col gap-3${animated ? " md:invisible" : ""} ${className}`}
			data-feature-index={index}
			href={card.href}
			{...(animated ? { "data-feature-card": "" } : {})}
		>
			<div className="flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-verevon-j-text/42">
				<span className="tracking-[0.18em]">{card.beat}</span>
				<span>{`0${index + 1} / ${String(total).padStart(2, "0")}`}</span>
			</div>

			<div className="relative aspect-[4/5] overflow-hidden bg-verevon-j-text/[0.04]">
				<Image
					alt={card.imageAlt}
					className="object-cover transition-transform duration-[900ms] ease-out will-change-transform group-hover:scale-[1.04] motion-reduce:transition-none motion-reduce:group-hover:scale-100"
					fill
					sizes="(max-width: 1140px) 92vw, 22vw"
					src={card.image}
					style={{ objectPosition: card.imagePosition ?? "center" }}
				/>
				{/* Keeps the overlaid module name legible on light photographs. */}
				<div
					aria-hidden="true"
					className="absolute inset-0 bg-[linear-gradient(180deg,rgba(23,23,23,0.34)_0%,rgba(23,23,23,0)_38%)]"
				/>
				<p className="absolute left-3 top-3 m-0 font-protokoll text-[10px] font-medium uppercase tracking-[0.17em] text-white/92">
					{card.module}
				</p>
			</div>

			<h3 className="m-0 max-w-full break-words font-arbeit text-[clamp(1.18rem,1.42vw,1.6rem)] font-light leading-[1.06] tracking-[-0.05em] text-verevon-j-text">
				{card.title}
			</h3>

			<div
				aria-label={`${card.module}: ${card.features.join(", ")}`}
				className="flex flex-wrap gap-1.5"
			>
				{card.features.map((feature) => (
					<span
						className="rounded-full border border-verevon-j-text/10 px-2 py-1 font-protokoll text-[9px] uppercase tracking-[0.12em] text-verevon-j-text/58"
						key={feature}
					>
						{feature}
					</span>
				))}
			</div>

			<p className="m-0 font-protokoll text-[clamp(0.82rem,0.82vw,0.94rem)] font-light leading-[1.45] text-verevon-text-muted/90">
				{card.text}
			</p>
		</a>
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
			<div className="flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-verevon-j-text/42">
				<span className="size-[5px] rounded-full bg-verevon-coral/70" />
				<span>{`0${index + 1} / ${String(total).padStart(2, "0")}`}</span>
			</div>

			<div className="relative aspect-[4/5] overflow-hidden bg-transparent">
				<FeatureCardFilm kind={card.film} />
			</div>

			<h3 className="m-0 max-w-full break-words font-arbeit text-[clamp(1.25rem,1.55vw,1.75rem)] font-light leading-[1.04] tracking-[-0.055em] text-verevon-j-text">
				{card.title}
			</h3>

			<p className="m-0 font-protokoll text-[9px] font-medium uppercase tracking-[0.17em] text-verevon-coral/80">
				{card.kicker}
			</p>

			<div
				aria-label={`${card.kicker}: ${card.features.join(", ")}`}
				className="flex flex-wrap gap-1.5"
			>
				{card.features.map((feature) => (
					<span
						className="rounded-full border border-verevon-j-text/10 px-2 py-1 font-protokoll text-[9px] uppercase tracking-[0.12em] text-verevon-j-text/58"
						key={feature}
					>
						{feature}
					</span>
				))}
			</div>

			<p className="m-0 font-protokoll text-[clamp(0.82rem,0.82vw,0.94rem)] font-light leading-[1.4] text-verevon-text-muted/90">
				{card.text}
			</p>
		</a>
	);
}
