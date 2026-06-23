"use client";

import {
	ArrowUp,
	AudioWaveform,
	CirclePlus,
	Clock3,
	Globe2,
	ImagePlus,
	Lightbulb,
	Mic,
	SlidersHorizontal,
	Sparkles,
	Telescope,
	Zap,
} from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

type VelionComposerPreviewProps = {
	animateCharacters?: boolean;
	className?: string;
	prompt: string;
};

const responseModes = [
	{ icon: Sparkles, label: "Agent mode" },
	{ icon: Zap, label: "Fast action" },
	{ icon: Lightbulb, label: "Explain" },
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
				"grid size-9 place-items-center rounded-[12px] border border-black/[0.07] bg-white/80 text-[#888] shadow-sm backdrop-blur-sm transition-colors",
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

export function VelionComposerPreview({
	animateCharacters = false,
	className,
	prompt,
}: VelionComposerPreviewProps) {
	return (
		<div className={cn("w-full text-velion-j-text", className)}>
			<div className="mb-3 flex items-center justify-between px-1">
				<div className="flex min-w-0 items-center gap-2">
					<button
						aria-label="Selected model"
						className="flex h-10 max-w-[172px] items-center gap-2 rounded-[12px] border border-black/[0.07] bg-white/90 px-3 font-protokoll text-[13px] font-medium text-[#333] shadow-[0_8px_20px_rgba(0,0,0,0.05)] backdrop-blur-sm sm:px-4"
						tabIndex={-1}
						type="button"
					>
						<Zap className="size-4 shrink-0 text-[#12b76a]" />
						<span className="min-w-0 truncate whitespace-nowrap">Velion Balance</span>
					</button>

					<button
						className="flex h-10 items-center gap-2 rounded-[12px] bg-[#2a2a2a] px-3 font-protokoll text-[13px] font-medium text-white shadow-md sm:px-4"
						tabIndex={-1}
						type="button"
					>
						<Sparkles className="size-[13px] shrink-0" />
						<span className="hidden whitespace-nowrap sm:inline">Create agent</span>
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
				aria-label="Velion composer preview"
				className="overflow-hidden rounded-[26px] bg-white px-3 pb-3 pt-[14px] shadow-[0_20px_60px_rgba(20,21,24,0.11),0_0_0_1px_rgba(0,0,0,0.03)]"
				onSubmit={(event) => event.preventDefault()}
			>
				<div className="relative px-4">
					<label className="sr-only" htmlFor="velion-preview-composer">
						Message Velion
					</label>
					<div
						aria-label={prompt}
						className="min-h-[66px] w-full resize-none bg-transparent px-1 font-protokoll text-[15px] font-light leading-[21px] text-[#1a1a1a] outline-none"
						id="velion-preview-composer"
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
							aria-label="Add files"
							className="group flex items-center gap-1.5 font-protokoll text-[13px] text-[#888]"
							tabIndex={-1}
							type="button"
						>
							<span className="flex size-8 items-center justify-center rounded-full bg-black/6">
								<CirclePlus className="size-4" />
							</span>
							<span>add files</span>
						</button>
						<div className="mx-1 hidden h-4 w-px bg-black/10 sm:block" />
						<ToolbarChip label="Suggestions">
							<Lightbulb className="size-4" />
						</ToolbarChip>
						<ToolbarChip active label="Deep search">
							<Telescope className="size-4" />
						</ToolbarChip>
						<button
							aria-label="Browse web"
							aria-pressed="true"
							className="flex items-center gap-1.5 rounded-lg bg-blue-50 px-3 py-[7px] font-protokoll text-[13px] font-medium text-blue-600"
							tabIndex={-1}
							type="button"
						>
							<Globe2 className="size-4" />
							Search
						</button>
						<button
							aria-label="Generate image"
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
						<ToolbarChip label="Voice mode">
							<AudioWaveform className="size-4" />
						</ToolbarChip>
						<ToolbarChip label="Voice input">
							<Mic className="size-4" />
						</ToolbarChip>
						<button
							aria-label="Send message"
							className="relative grid size-11 place-items-center rounded-[14px] bg-[#111111] text-white shadow-[0_10px_26px_rgba(0,0,0,0.16)]"
							data-feature-send-button
							tabIndex={-1}
							title="Send message"
							type="submit"
						>
							<span
								aria-hidden="true"
								className="absolute inset-[-5px] rounded-[18px] border border-velion-coral/70 opacity-0"
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
