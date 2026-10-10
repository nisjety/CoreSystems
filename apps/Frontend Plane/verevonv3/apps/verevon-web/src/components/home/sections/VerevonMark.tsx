import type { CSSProperties } from "react";

type VerevonMarkProps = {
	className?: string;
	style?: CSSProperties;
	strokeWidth?: number;
	variant?: "filled" | "outline";
};

export function VerevonMark({
	className = "",
	style,
	strokeWidth = 1.8,
	variant = "filled",
}: VerevonMarkProps) {
	const isOutline = variant === "outline";

	return (
		<svg
			aria-hidden="true"
			className={className}
			style={style}
			viewBox="0 0 120 120"
			xmlns="http://www.w3.org/2000/svg"
			fill={isOutline ? "none" : "currentColor"}
		>
			{/* Main left / bottom V shape. Path data kept on ONE line: React
			    diffs the `d` attribute as a plain string, and multi-line
			    template whitespace was normalized differently on server vs
			    client, causing a hydration-mismatch warning on every load. */}
			<path
				d="M28 36 H42.7 L65.1 65.1 C66.8 67.3 70.2 67.5 72.4 65 L74.4 62.7 C77.5 66.2 77 70.2 74.7 73 L63.6 84 L28 36 Z"
				stroke={isOutline ? "currentColor" : "none"}
				strokeWidth={isOutline ? strokeWidth : undefined}
				strokeLinejoin="miter"
				strokeLinecap="butt"
				strokeMiterlimit={10}
				vectorEffect="non-scaling-stroke"
			/>

			{/* Right upper wing */}
			<path
				d="M78.9 36 H93.6 L74.4 61.7 L68.3 56.2 C65.9 54 66 50.7 67.8 48.5 L78.9 36 Z"
				stroke={isOutline ? "currentColor" : "none"}
				strokeWidth={isOutline ? strokeWidth : undefined}
				strokeLinejoin="miter"
				strokeLinecap="butt"
				strokeMiterlimit={10}
				vectorEffect="non-scaling-stroke"
			/>
		</svg>
	);
}

export function VerevonMarkFilled({ className = "" }: { className?: string }) {
	return <VerevonMark className={className} variant="filled" />;
}

export function VerevonMarkOutline({
	className = "",
	strokeWidth = 1.8,
}: {
	className?: string;
	strokeWidth?: number;
}) {
	return (
		<VerevonMark
			className={className}
			variant="outline"
			strokeWidth={strokeWidth}
		/>
	);
}

export default VerevonMark;
