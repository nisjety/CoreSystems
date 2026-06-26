import type { CSSProperties } from "react";

/**
 * StatusBadge — the honesty primitive for Velion's trust surfaces.
 *
 * Velion holds the controls but not (yet) the third-party certifications.
 * Every capability/control claim on the marketing site renders its real
 * maturity through this badge so nothing roadmap is ever shown as held.
 *
 *  - live     → shipped + engineering-confirmed   ("Live")
 *  - progress → being wired / partial             ("Under arbeid")
 *  - planned  → roadmap, not started in product   ("Planlagt")
 */
export type StatusLevel = "live" | "progress" | "planned";

const STATUS_COPY: Record<StatusLevel, string> = {
	live: "Live",
	progress: "Under arbeid",
	planned: "Planlagt",
};

const STATUS_STYLE: Record<StatusLevel, CSSProperties> = {
	live: {
		color: "var(--velion-status-live)",
		background: "var(--velion-status-live-soft)",
	},
	progress: {
		color: "var(--velion-coral-deep)",
		background: "var(--velion-status-progress-soft)",
	},
	planned: {
		color: "var(--velion-status-planned)",
		background: "var(--velion-status-planned-soft)",
	},
};

const DOT_COLOR: Record<StatusLevel, string> = {
	live: "var(--velion-status-live)",
	progress: "var(--velion-status-progress)",
	planned: "var(--velion-status-planned)",
};

type StatusBadgeProps = {
	level: StatusLevel;
	label?: string;
	className?: string;
};

export function StatusBadge({ level, label, className = "" }: StatusBadgeProps) {
	return (
		<span
			className={[
				"inline-flex w-fit items-center gap-2 whitespace-nowrap rounded-full px-3 py-1 font-protokoll text-[0.74rem] font-medium uppercase leading-none tracking-[0.08em]",
				className,
			]
				.filter(Boolean)
				.join(" ")}
			style={STATUS_STYLE[level]}
		>
			<span
				aria-hidden="true"
				className="size-[6px] rounded-full"
				style={{ background: DOT_COLOR[level] }}
			/>
			{label ?? STATUS_COPY[level]}
		</span>
	);
}

export default StatusBadge;
