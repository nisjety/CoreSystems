"use client";

import type { CSSProperties } from "react";

type SensesGuidelineProps = {
	className?: string;
	delaySeconds?: number;
	orientation?: "horizontal" | "vertical";
};

/**
 * SensesGuidelines — continuous marching-dash + traveling-glow shimmer,
 * matching titangatequity.com's .svg-guideline treatment. Position, length,
 * and thickness all still come from the same Tailwind classes the original
 * plain span used (passed in via className) — this only changes the line's
 * rendering, from a flat solid line to this animated dashed one.
 */
export function SensesGuidelines({
	className = "",
	delaySeconds = 0,
	orientation = "vertical",
}: SensesGuidelineProps) {
	return (
		<div
			aria-hidden="true"
			className={`verevon-senses-guideline verevon-senses-guideline--${orientation} ${className}`}
		>
			<svg
				className="verevon-senses-guideline__svg"
				preserveAspectRatio="none"
				style={
					{
						"--verevon-senses-guideline-delay": `${delaySeconds}s`,
					} as CSSProperties
				}
				viewBox={orientation === "vertical" ? "0 0 1 100" : "0 0 100 1"}
			>
				<path
					className="verevon-senses-guideline__path"
					d={orientation === "vertical" ? "M.5 0v100" : "M0 .5h100"}
				/>
			</svg>
		</div>
	);
}

export default SensesGuidelines;
