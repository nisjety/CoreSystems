export type VisualPanelVariant =
	| "craft"
	| "surface"
	| "mark"
	| "icon"
	| "open"
	| "detail"
	| "award"
	| "heritage";

type VisualPanelProps = {
	className?: string;
	label: string;
	variant: VisualPanelVariant;
};

const variantBackgrounds: Record<VisualPanelVariant, string> = {
	craft:
		"bg-[linear-gradient(120deg,rgba(79,125,243,0.15),transparent_38%),linear-gradient(145deg,#f9f8f5,#e7eceb)]",
	surface:
		"bg-[linear-gradient(135deg,#171717,var(--velion-h-teal-deep))] text-velion-c-white",
	mark:
		"bg-[linear-gradient(90deg,transparent_0_45%,rgba(121,56,25,0.2)_45%_45.5%,transparent_45.5%_100%),linear-gradient(145deg,#ffffff,#ebe8e1)]",
	icon:
		"bg-[linear-gradient(90deg,rgba(26,26,26,0.06)_1px,transparent_1px),linear-gradient(180deg,rgba(26,26,26,0.06)_1px,transparent_1px),linear-gradient(145deg,#fbfaf7,#dfe6e4)] bg-[length:72px_72px,72px_72px,100%_100%]",
	open:
		"bg-[linear-gradient(90deg,rgba(26,26,26,0.06)_1px,transparent_1px),linear-gradient(180deg,rgba(26,26,26,0.06)_1px,transparent_1px),linear-gradient(145deg,#fbfaf7,#dfe6e4)] bg-[length:72px_72px,72px_72px,100%_100%]",
	detail:
		"bg-[linear-gradient(90deg,rgba(26,26,26,0.06)_1px,transparent_1px),linear-gradient(180deg,rgba(26,26,26,0.06)_1px,transparent_1px),linear-gradient(145deg,#fbfaf7,#dfe6e4)] bg-[length:72px_72px,72px_72px,100%_100%]",
	award:
		"bg-[radial-gradient(circle_at_72%_26%,rgba(238,122,80,0.14),transparent_20%),linear-gradient(145deg,#ffffff,#f1eee8)]",
	heritage:
		"bg-[radial-gradient(circle_at_72%_26%,rgba(79,125,243,0.18),transparent_18%),linear-gradient(90deg,rgba(26,26,26,0.08)_1px,transparent_1px),linear-gradient(180deg,rgba(26,26,26,0.08)_1px,transparent_1px),linear-gradient(145deg,#ffffff,#f0eee8)] bg-[length:100%_100%,86px_86px,86px_86px,100%_100%]",
};

const assetBackgrounds: Partial<Record<VisualPanelVariant, string>> = {
	craft:
		"bg-[linear-gradient(180deg,rgba(248,248,247,0.02),rgba(248,248,247,0.08)),url('/mist-bloom.png')]",
	surface:
		"bg-[linear-gradient(180deg,rgba(13,15,17,0.18),rgba(13,15,17,0.36)),url('/signal-ridge.png')]",
	mark:
		"bg-[linear-gradient(180deg,rgba(248,248,247,0.04),rgba(248,248,247,0.14)),url('/glass-edge.png')]",
	icon:
		"bg-[linear-gradient(180deg,rgba(248,248,247,0),rgba(248,248,247,0.08)),url('/warm-flight.png')]",
	open:
		"bg-[linear-gradient(180deg,rgba(248,248,247,0.04),rgba(248,248,247,0.12)),url('/mist-bloom.png')]",
	detail:
		"bg-[linear-gradient(180deg,rgba(248,248,247,0.02),rgba(248,248,247,0.1)),url('/soft-orb.png')]",
	heritage:
		"bg-[linear-gradient(180deg,rgba(248,248,247,0),rgba(248,248,247,0.16)),url('/signal-ridge.png')]",
};

function getVariantTone(variant: VisualPanelVariant) {
	if (variant === "surface") {
		return {
			line: "bg-velion-c-white/25",
			node:
				"border-velion-c-white/40 bg-velion-c-white/80 shadow-[0_0_0_6px_rgba(247,244,238,0.16)]",
			panel: "border-velion-c-white/20 bg-velion-c-white/10",
			card: "border-velion-c-white/20 bg-velion-c-white/10",
		};
	}

	return {
		line: "bg-velion-j-text/15",
		node:
			"border-velion-j-text/25 bg-background shadow-[0_0_0_6px_rgba(248,248,247,0.64)]",
		panel: "border-velion-j-text/10 bg-white/45",
		card: "border-velion-j-text/12 bg-white/20",
	};
}

export function VisualPanel({
	className = "",
	label,
	variant,
}: VisualPanelProps) {
	const tone = getVariantTone(variant);
	const assetBackground = assetBackgrounds[variant];

	return (
		<div
			aria-label={label}
			className={[
				"relative isolate min-h-0 w-full overflow-hidden rounded-[2px] shadow-[inset_0_0_0_1px_rgba(26,26,26,0.05)]",
				"aspect-[1.82]",
				variantBackgrounds[variant],
				className,
			]
				.filter(Boolean)
				.join(" ")}
			role="img"
		>
			{assetBackground ? (
				<span
					aria-hidden="true"
					className={[
						"absolute inset-0 z-0 block scale-[1.02] bg-cover bg-center opacity-90 saturate-[0.85] contrast-[1.03]",
						assetBackground,
					].join(" ")}
				/>
			) : null}

			<span
				aria-hidden="true"
				className={[
					"absolute inset-x-[10%] top-[12%] z-[2] h-[34%] border backdrop-blur-[18px]",
					"bg-[linear-gradient(90deg,rgba(238,122,80,0.42)_0_4px,transparent_4px_100%),rgba(255,255,255,0.2)]",
					tone.card,
				].join(" ")}
			/>

			<span
				aria-hidden="true"
				className={[
					"absolute bottom-[12%] right-[10%] z-[2] h-[28%] w-[48%] border backdrop-blur-[16px]",
					tone.panel,
				].join(" ")}
			/>

			<span
				aria-hidden="true"
				className={[
					"absolute left-[11%] top-[55%] z-[2] h-px w-[72%]",
					tone.line,
				].join(" ")}
			/>
			<span
				aria-hidden="true"
				className={[
					"absolute left-[20%] top-[42%] z-[2] h-px w-[48%] rotate-[-18deg]",
					tone.line,
				].join(" ")}
			/>
			<span
				aria-hidden="true"
				className={[
					"absolute left-[67%] top-[18%] z-[2] h-[64%] w-px",
					tone.line,
				].join(" ")}
			/>

			<span
				aria-hidden="true"
				className={[
					"absolute left-[18%] top-[39%] z-[3] size-[13px] rounded-full border backdrop-blur-sm",
					tone.node,
				].join(" ")}
			/>
			<span
				aria-hidden="true"
				className={[
					"absolute left-[66%] top-[29%] z-[3] size-[13px] rounded-full border backdrop-blur-sm",
					tone.node,
				].join(" ")}
			/>
			<span
				aria-hidden="true"
				className={[
					"absolute left-[52%] top-[67%] z-[3] size-[13px] rounded-full border backdrop-blur-sm",
					variant === "surface"
						? tone.node
						: "border-velion-a-earth/40 bg-background shadow-[0_0_0_6px_rgba(248,248,247,0.64)]",
				].join(" ")}
			/>

			<span
				aria-hidden="true"
				className={[
					"absolute right-[12%] top-[18%] z-[1] h-[16%] w-[28%] border bg-[repeating-linear-gradient(0deg,transparent_0_13px,rgba(26,26,26,0.08)_13px_14px),rgba(255,255,255,0.48)]",
					tone.panel,
				].join(" ")}
			/>

			<span
				aria-hidden="true"
				className={[
					"absolute bottom-[18%] left-[12%] z-[1] h-[18%] w-[32%] border bg-[repeating-linear-gradient(0deg,transparent_0_13px,rgba(26,26,26,0.08)_13px_14px),rgba(255,255,255,0.48)]",
					tone.panel,
				].join(" ")}
			/>
		</div>
	);
}