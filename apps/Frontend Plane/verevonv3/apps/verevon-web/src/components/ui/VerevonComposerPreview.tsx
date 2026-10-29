"use client";

import {
	ArrowRight,
	ArrowUp,
	AudioWaveform,
	CirclePlus,
	Clock3,
	ChevronDown,
	Globe2,
	ImagePlus,
	Lightbulb,
	Link2,
	Mic,
	MousePointer2,
	Search,
	SlidersHorizontal,
	Sparkles,
	ShoppingBag,
	Telescope,
	Zap,
} from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { cn } from "@/lib/utils";

export type VerevonComposerMode = "chat" | "crawl" | "search";

type VerevonComposerPreviewProps = {
	ambientMotion?: boolean;
	animateCharacters?: boolean;
	className?: string;
	id?: string;
	mode?: VerevonComposerMode;
	prompt: string;
};

const responseModes = [
	{ icon: Sparkles, label: "Agentmodus" },
	{ icon: Zap, label: "Hurtighandling" },
	{ icon: Lightbulb, label: "Forklar" },
];

const CRAWL_LINK_AUTO_SELECT_DELAY = 1_600;

function PromptPreview({
	ambientMotion,
	animateCharacters,
	prompt,
}: {
	ambientMotion: boolean;
	animateCharacters: boolean;
	prompt: string;
}) {
	const content = animateCharacters ? (
		<span aria-label={prompt}>
			{prompt.split("").map((char, index) => (
				<span
					aria-hidden="true"
					className="inline-block opacity-100"
					data-feature-char
					key={`${char}-${index}`}
				>
					{char === " " ? "\u00A0" : char}
				</span>
			))}
		</span>
	) : (
		prompt
	);

	if (!ambientMotion) return <>{content}</>;

	return (
		<span className="inline-flex items-center motion-safe:animate-[verevon-composer-idle-prompt-in_420ms_cubic-bezier(0.19,1,0.22,1)_both]">
			{content}
			<span
				aria-hidden="true"
				className="ml-1 inline-block h-[18px] w-px rounded-full bg-verevon-coral align-[-0.16em] motion-safe:animate-[verevon-composer-idle-caret_1.05s_steps(2,end)_infinite]"
				data-composer-idle-caret
			/>
		</span>
	);
}

function IconChip({
	active,
	children,
	label,
}: {
	active?: boolean;
	children: ReactNode;
	label: string;
}) {
	return (
		<button
			aria-label={label}
			aria-pressed={active}
			className={cn(
				"grid size-[34px] place-items-center rounded-[12px] border border-black/[0.07] bg-white/80 text-[#888] shadow-sm backdrop-blur-sm transition-colors",
				active ? "bg-white text-[#1a1a1a] ring-1 ring-black/[0.08]" : "",
			)}
			tabIndex={-1}
			title={label}
			type="button"
		>
			{children}
		</button>
	);
}

function ToolbarChip({
	active,
	children,
	label,
}: {
	active?: boolean;
	children: ReactNode;
	label: string;
}) {
	return (
		<button
			aria-label={label}
			aria-pressed={active}
			className={cn(
				"grid size-8 place-items-center rounded-lg text-[#777] transition-colors",
				active ? "bg-white text-[#1a1a1a] shadow-sm" : "bg-transparent",
			)}
			tabIndex={-1}
			title={label}
			type="button"
		>
			{children}
		</button>
	);
}

export function VerevonComposerPreview({
	ambientMotion = false,
	animateCharacters = false,
	className,
	id = "verevon-preview-composer",
	mode,
	prompt,
}: VerevonComposerPreviewProps) {
	if (mode === "search") {
		return (
			<SearchBarPreview
				ambientMotion={ambientMotion}
				className={className}
				prompt={prompt}
			/>
		);
	}

	if (mode === "crawl") {
		return (
			<CrawlBarPreview
				ambientMotion={ambientMotion}
				className={className}
				prompt={prompt}
			/>
		);
	}

	return (
		<ChatComposerPreview
			ambientMotion={ambientMotion}
			animateCharacters={animateCharacters}
			className={className}
			id={id}
			prompt={prompt}
		/>
	);
}

function SearchBarPreview({
	ambientMotion = false,
	className,
	prompt,
}: {
	ambientMotion?: boolean;
	className?: string;
	prompt: string;
}) {
	return (
		<div aria-hidden="true" className={cn("flex w-full items-center gap-2", className)} data-composer-mode="search">
			<button
				aria-label="Send søkekontekst til samtalen"
				className="grid size-11 shrink-0 place-items-center rounded-full border border-black/[0.07] bg-white/90 text-[#888] shadow-[0_8px_22px_rgba(0,0,0,0.06)] backdrop-blur-sm"
				tabIndex={-1}
				type="button"
			>
				<CirclePlus className="size-4" />
			</button>
			<div className="min-w-0 flex-1">
				<div
					className={cn(
						"flex h-12 w-full shrink-0 overflow-hidden rounded-full border border-black/[0.07] bg-white/90 pl-4 shadow-[0_10px_28px_rgba(0,0,0,0.07)] backdrop-blur-sm motion-safe:[will-change:width]",
						ambientMotion &&
							"motion-safe:animate-[verevon-composer-search-pill_4s_cubic-bezier(0.16,1,0.3,1)_infinite]",
					)}
					data-composer-search-pill={ambientMotion || undefined}
				>
					<Search className="my-auto size-4 shrink-0 text-[#9A9188]" />
					<div
						className={cn(
							"flex min-w-0 flex-1 items-center gap-2 pl-2 pr-1.5",
							ambientMotion &&
								"motion-safe:animate-[verevon-composer-search-content_4s_ease-out_infinite]",
						)}
						data-composer-search-content={ambientMotion || undefined}
					>
						<span className="min-w-0 flex-1 truncate text-left font-geist text-[14px] font-medium text-[#777]">
							{prompt}
						</span>
						<button
							aria-label="Søk"
							className="grid size-9 shrink-0 place-items-center rounded-full bg-[#171717] text-white shadow-[0_8px_22px_rgba(0,0,0,0.14)]"
							tabIndex={-1}
							type="button"
						>
							<ArrowRight className="size-4" />
						</button>
					</div>
				</div>
			</div>
		</div>
	);
}

function CrawlBarPreview({
	ambientMotion = false,
	className,
	prompt,
}: {
	ambientMotion?: boolean;
	className?: string;
	prompt: string;
}) {
	const [source, setSource] = useState<"lenke" | "crawl" | "produkter">("crawl");
	const [hasManualSourceSelection, setHasManualSourceSelection] = useState(false);
	const [prefersReducedMotion, setPrefersReducedMotion] = useState(false);

	useEffect(() => {
		const mediaQuery = window.matchMedia?.("(prefers-reduced-motion: reduce)");
		if (!mediaQuery) return;

		const updateMotionPreference = (event?: MediaQueryListEvent) => {
			setPrefersReducedMotion(event?.matches ?? mediaQuery.matches);
		};

		updateMotionPreference();
		mediaQuery.addEventListener("change", updateMotionPreference);
		return () => mediaQuery.removeEventListener("change", updateMotionPreference);
	}, []);

	useEffect(() => {
		if (hasManualSourceSelection || !ambientMotion || prefersReducedMotion) return;

		const timer = window.setTimeout(() => {
			setSource("lenke");
		}, CRAWL_LINK_AUTO_SELECT_DELAY);

		return () => window.clearTimeout(timer);
	}, [ambientMotion, hasManualSourceSelection, prefersReducedMotion]);
	const sources = [
		{
			icon: Link2,
			iconName: "link",
			label: "Lenke",
			placeholder: "Lim inn en lenke …",
			source: "lenke" as const,
		},
		{
			icon: Globe2,
			iconName: "globe",
			label: "Hent",
			placeholder: prompt,
			source: "crawl" as const,
		},
		{
			icon: ShoppingBag,
			iconName: "shopping-bag",
			label: "Produkter",
			placeholder: "Lim inn en produktside …",
			source: "produkter" as const,
		},
	];
	const activeSource = sources.find((item) => item.source === source) ?? sources[1];
	const InputIcon = activeSource.icon;

	return (
		<div className={cn("grid w-full gap-3", className)} data-composer-mode="crawl">
			<div
				aria-label="Velg innholdstype"
				className="relative isolate flex w-fit items-center gap-1 rounded-full bg-black/[0.04] p-[3px]"
				data-composer-crawl-controls
				role="group"
			>
				{sources.map((item) => {
					const isActive = item.source === source;
					const SourceIcon = item.icon;
					const animateLink =
						ambientMotion && !hasManualSourceSelection && item.source === "lenke" && !isActive;

					return (
						<button
							aria-pressed={isActive}
							className={cn(
								"relative inline-flex items-center gap-1.5 rounded-full px-3 py-[5px] font-geist text-[12.5px] font-semibold leading-none transition-[color,background-color,box-shadow] duration-200 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-verevon-coral/70",
								isActive
									? "z-10 bg-white text-[#222] shadow-sm"
									: "text-[#999] hover:bg-white/65 hover:text-[#555]",
								animateLink &&
									"motion-safe:animate-[verevon-composer-crawl-link_3.8s_ease-in-out_infinite]",
							)}
							data-composer-crawl-control={item.source}
							data-composer-crawl-link={
								item.source === "lenke" && animateLink ? true : undefined
							}
							key={item.source}
							onClick={() => {
								setSource(item.source);
								setHasManualSourceSelection(true);
							}}
							type="button"
						>
							<SourceIcon className="size-3.5" />
							{item.label}
							{item.source === "lenke" && animateLink ? (
								<MousePointer2
									aria-hidden="true"
									className="pointer-events-none absolute left-1/2 top-1/2 z-20 size-5 fill-white text-[#171717] opacity-0 drop-shadow-[0_2px_3px_rgba(0,0,0,0.18)] motion-safe:animate-[verevon-composer-crawl-cursor_3.8s_cubic-bezier(0.16,1,0.3,1)_infinite]"
									data-composer-crawl-cursor
								/>
							) : null}
						</button>
					);
				})}
			</div>
			<div className="flex min-w-0 items-center gap-2 rounded-[18px] border border-black/[0.07] bg-white/90 px-3 py-[10px] shadow-[0_10px_28px_rgba(0,0,0,0.07)] backdrop-blur-sm">
				<InputIcon
					className="size-4 shrink-0 text-[#9A9188]"
					data-composer-crawl-input-icon={activeSource.iconName}
				/>
				<span
					aria-live="polite"
					className="min-w-0 flex-1 truncate text-left font-geist text-[14px] font-medium text-[#777]"
					data-composer-crawl-placeholder
				>
					{activeSource.placeholder}
				</span>
				<span
					aria-hidden="true"
					className="grid size-8 shrink-0 place-items-center rounded-[12px] bg-[#171717] text-white shadow-[0_8px_22px_rgba(0,0,0,0.14)]"
					data-composer-crawl-submit
				>
					<ArrowRight className="size-4" />
				</span>
			</div>
		</div>
	);
}

function ChatComposerPreview({
	ambientMotion = false,
	animateCharacters = false,
	className,
	id = "verevon-preview-composer",
	prompt,
}: VerevonComposerPreviewProps) {
	const webSearchActive = false;
	const webSearchLabel = "Søk";

	return (
		<div
			aria-hidden="true"
			className={cn("w-full text-verevon-j-text", className)}
			data-composer-mode="chat"
		>
			<div className="mb-3 flex items-center justify-between px-1">
				<div className="flex min-w-0 items-center gap-2">
					<button
						aria-label="Valgt modell"
						className="flex h-[38px] max-w-[150px] items-center gap-2 rounded-[12px] border border-black/[0.07] bg-white/90 px-3 font-geist text-[13px] font-medium text-[#333] shadow-[0_8px_20px_rgba(0,0,0,0.05)] backdrop-blur-sm"
						tabIndex={-1}
						type="button"
					>
						<Zap className="size-4 shrink-0 text-[#12b76a]" />
						<span className="min-w-0 truncate whitespace-nowrap">Verevon Balance</span>
						<ChevronDown className="size-4 shrink-0 text-[#999]" />
					</button>

					<button
						className="flex h-[38px] items-center gap-2 rounded-[12px] bg-[#2a2a2a] px-3 font-geist text-[13px] font-medium text-white shadow-md"
						tabIndex={-1}
						type="button"
					>
						<Sparkles className="size-[13px] shrink-0" />
						<span className="hidden whitespace-nowrap sm:inline">Opprett agent</span>
					</button>
				</div>

				<div className="flex items-center gap-1.5">
					<IconChip label="Historikk">
						<Clock3 className="size-3.5" />
					</IconChip>
					<IconChip label="Innstillinger">
						<SlidersHorizontal className="size-3.5" />
					</IconChip>
				</div>
			</div>

			<form
				aria-label="Forhåndsvisning av Verevon-komponisten"
				className={cn(
					"relative overflow-hidden rounded-[14px] border border-black/[0.07] bg-white px-[14px] pb-3 pt-[14px] shadow-[0_20px_60px_rgba(20,21,24,0.11)]",
					ambientMotion && "motion-safe:animate-[verevon-composer-idle-shell_5.2s_ease-in-out_infinite]",
				)}
				data-composer-idle-shell={ambientMotion || undefined}
				onSubmit={(event) => event.preventDefault()}
			>
				<div className="relative px-4">
					<label className="sr-only" htmlFor={id}>
						Meld Verevon
					</label>
					<div
						aria-label={prompt}
						className="min-h-[80px] w-full resize-none bg-transparent px-1 font-geist text-[15px] font-medium leading-[21px] text-[#1a1a1a] outline-none"
						id={id}
						role="textbox"
					>
						<PromptPreview
							ambientMotion={ambientMotion}
							animateCharacters={animateCharacters}
							prompt={prompt}
						/>
					</div>
				</div>

				<div
					aria-hidden="true"
					className="mx-4 mb-2 h-px origin-left scale-x-0 overflow-hidden rounded-full bg-black/6 opacity-0"
					data-feature-processing
				>
					<div className="h-full w-full rounded-full bg-[linear-gradient(90deg,#ee7a50,#4f7df3,#ee7a50)]" />
				</div>

				<div className="flex flex-col gap-3 px-3 lg:flex-row lg:items-center lg:justify-between">
					<div className="flex min-w-0 flex-wrap items-center gap-1.5">
						<button
							aria-label="Legg til filer"
							className="group flex items-center gap-1.5 font-geist text-[13px] text-[#888]"
							tabIndex={-1}
							type="button"
						>
							<span className="flex size-8 items-center justify-center rounded-full bg-black/6">
								<CirclePlus className="size-4" />
							</span>
							<span>legg til filer</span>
						</button>
						<div className="mx-1 hidden h-4 w-px bg-black/10 sm:block" />
						<ToolbarChip label="Forslag">
							<Lightbulb className="size-4" />
						</ToolbarChip>
						<ToolbarChip active={false} label="Dypsøk">
							<Telescope className="size-4" />
						</ToolbarChip>
						<button
							aria-label="Søk på nettet"
							aria-pressed={webSearchActive}
							className={cn(
								"flex items-center gap-1.5 rounded-lg px-3 py-[7px] font-geist text-[13px] font-medium transition-colors",
								webSearchActive ? "bg-blue-50 text-blue-600" : "text-[#777]",
							)}
							tabIndex={-1}
							type="button"
						>
							<Globe2 className="size-4" />
							{webSearchLabel}
						</button>
						<button
							aria-label="Generer bilde"
							className="hidden items-center gap-1.5 rounded-lg px-3 py-[7px] font-geist text-[13px] font-medium text-[#777] md:flex"
							tabIndex={-1}
							type="button"
						>
							<ImagePlus className="size-4" />
							Bilde
						</button>
					</div>

					<div className="flex items-center justify-end gap-1.5">
						<div className="flex items-center gap-0.5 rounded-xl bg-black/6 p-[3px]">
							{responseModes.map((mode, index) => {
								const Icon = mode.icon;

								return (
									<ToolbarChip active={index === 0} key={mode.label} label={mode.label}>
										<Icon className="size-4" />
									</ToolbarChip>
								);
							})}
						</div>
						<ToolbarChip label="Talemodus">
							<AudioWaveform className="size-4" />
						</ToolbarChip>
						<ToolbarChip label="Taleinndata">
							<Mic className="size-4" />
						</ToolbarChip>
						<button
							aria-label="Send melding"
							className="relative grid size-10 place-items-center rounded-[12px] bg-[#111111] text-white shadow-[0_10px_26px_rgba(0,0,0,0.16)]"
							data-feature-send-button
							tabIndex={-1}
							title="Send melding"
							type="submit"
						>
							<span
								aria-hidden="true"
								className="absolute inset-[-5px] rounded-[18px] border border-verevon-coral/70 opacity-0"
								data-feature-send-ring
							/>
							{ambientMotion ? (
								<span
									aria-hidden="true"
									className="absolute inset-[-6px] rounded-[18px] border border-verevon-coral/45 opacity-0 motion-safe:animate-[verevon-composer-idle-send_2.8s_ease-out_infinite]"
									data-composer-idle-send
								/>
							) : null}
							<ArrowUp className="size-4" data-feature-send-arrow />
						</button>
					</div>
				</div>
			</form>
		</div>
	);
}
