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
	Search,
	SlidersHorizontal,
	Sparkles,
	ShoppingBag,
	Telescope,
	Zap,
} from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export type VerevonComposerMode = "chat" | "crawl" | "search";

type VerevonComposerPreviewProps = {
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

function PromptPreview({
	animateCharacters,
	prompt,
}: {
	animateCharacters: boolean;
	prompt: string;
}) {
	if (!animateCharacters) {
		return <>{prompt}</>;
	}

	return (
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
	animateCharacters = false,
	className,
	id = "verevon-preview-composer",
	mode,
	prompt,
}: VerevonComposerPreviewProps) {
	if (mode === "search") {
		return <SearchBarPreview className={className} prompt={prompt} />;
	}

	if (mode === "crawl") {
		return <CrawlBarPreview className={className} prompt={prompt} />;
	}

	return (
		<ChatComposerPreview
			animateCharacters={animateCharacters}
			className={className}
			id={id}
			prompt={prompt}
		/>
	);
}

function SearchBarPreview({
	className,
	prompt,
}: {
	className?: string;
	prompt: string;
}) {
	return (
		<div aria-hidden="true" className={cn("flex w-full items-center gap-2", className)} data-composer-mode="search">
			<button
				aria-label="Send søkekontekst til chat"
				className="grid size-11 shrink-0 place-items-center rounded-full border border-black/[0.07] bg-white/90 text-[#888] shadow-[0_8px_22px_rgba(0,0,0,0.06)] backdrop-blur-sm"
				tabIndex={-1}
				type="button"
			>
				<CirclePlus className="size-4" />
			</button>
			<div className="flex h-12 min-w-0 flex-1 items-center gap-2 rounded-full border border-black/[0.07] bg-white/90 px-4 shadow-[0_10px_28px_rgba(0,0,0,0.07)] backdrop-blur-sm">
				<Search className="size-4 shrink-0 text-[#9A9188]" />
				<span className="min-w-0 flex-1 truncate text-left font-protokoll text-[14px] font-medium text-[#777]">
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
	);
}

function CrawlBarPreview({
	className,
	prompt,
}: {
	className?: string;
	prompt: string;
}) {
	return (
		<div aria-hidden="true" className={cn("grid w-full gap-3", className)} data-composer-mode="crawl">
			<div className="flex w-fit items-center gap-1 rounded-full bg-black/[0.04] p-[3px]">
				<span className="inline-flex items-center gap-1.5 rounded-full px-3 py-[5px] font-protokoll text-[12.5px] font-semibold leading-none text-[#999]">
					<Link2 className="size-3.5" /> Lenke
				</span>
				<span className="inline-flex items-center gap-1.5 rounded-full bg-white px-3 py-[5px] font-protokoll text-[12.5px] font-semibold leading-none text-[#222] shadow-sm">
					<Globe2 className="size-3.5" /> Crawl
				</span>
				<span className="inline-flex items-center gap-1.5 rounded-full px-3 py-[5px] font-protokoll text-[12.5px] font-semibold leading-none text-[#999]">
					<ShoppingBag className="size-3.5" /> Produkter
				</span>
			</div>
			<div className="flex min-w-0 items-center gap-2 rounded-[18px] border border-black/[0.07] bg-white/90 px-3 py-[10px] shadow-[0_10px_28px_rgba(0,0,0,0.07)] backdrop-blur-sm">
				<Link2 className="size-4 shrink-0 text-[#9A9188]" />
				<span className="min-w-0 flex-1 truncate text-left font-protokoll text-[14px] font-medium text-[#777]">
					{prompt}
				</span>
				<button
					aria-label="Start crawl"
					className="grid size-8 shrink-0 place-items-center rounded-[12px] bg-[#171717] text-white shadow-[0_8px_22px_rgba(0,0,0,0.14)]"
					tabIndex={-1}
					type="button"
				>
					<ArrowRight className="size-4" />
				</button>
			</div>
		</div>
	);
}

function ChatComposerPreview({
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
						className="flex h-[38px] max-w-[150px] items-center gap-2 rounded-[12px] border border-black/[0.07] bg-white/90 px-3 font-protokoll text-[13px] font-medium text-[#333] shadow-[0_8px_20px_rgba(0,0,0,0.05)] backdrop-blur-sm"
						tabIndex={-1}
						type="button"
					>
						<Zap className="size-4 shrink-0 text-[#12b76a]" />
						<span className="min-w-0 truncate whitespace-nowrap">Verevon Balance</span>
						<ChevronDown className="size-4 shrink-0 text-[#999]" />
					</button>

					<button
						className="flex h-[38px] items-center gap-2 rounded-[12px] bg-[#2a2a2a] px-3 font-protokoll text-[13px] font-medium text-white shadow-md"
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
				className="overflow-hidden rounded-[14px] border border-black/[0.07] bg-white px-[14px] pb-3 pt-[14px] shadow-[0_20px_60px_rgba(20,21,24,0.11)]"
				onSubmit={(event) => event.preventDefault()}
			>
				<div className="relative px-4">
					<label className="sr-only" htmlFor={id}>
						Meld Verevon
					</label>
					<div
						aria-label={prompt}
						className="min-h-[80px] w-full resize-none bg-transparent px-1 font-protokoll text-[15px] font-medium leading-[21px] text-[#1a1a1a] outline-none"
						id={id}
						role="textbox"
					>
						<PromptPreview animateCharacters={animateCharacters} prompt={prompt} />
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
							className="group flex items-center gap-1.5 font-protokoll text-[13px] text-[#888]"
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
								"flex items-center gap-1.5 rounded-lg px-3 py-[7px] font-protokoll text-[13px] font-medium transition-colors",
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
							className="hidden items-center gap-1.5 rounded-lg px-3 py-[7px] font-protokoll text-[13px] font-medium text-[#777] md:flex"
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
							<ArrowUp className="size-4" data-feature-send-arrow />
						</button>
					</div>
				</div>
			</form>
		</div>
	);
}
