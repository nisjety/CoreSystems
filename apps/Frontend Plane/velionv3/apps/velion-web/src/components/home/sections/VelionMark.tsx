type VelionMarkProps = {
	className?: string;
	strokeWidth?: number;
	variant?: "filled" | "outline";
};

export function VelionMark({
	className = "",
	strokeWidth = 1.8,
	variant = "filled",
}: VelionMarkProps) {
	const isOutline = variant === "outline";

	return (
		<svg
			aria-hidden="true"
			className={className}
			viewBox="0 0 120 120"
			xmlns="http://www.w3.org/2000/svg"
			fill={isOutline ? "none" : "currentColor"}
		>
			{/* Main left / bottom V shape */}
			<path
				d="
					M28 36
					H42.7
					L65.1 65.1
					C66.8 67.3 70.2 67.5 72.4 65
					L74.4 62.7
					C77.5 66.2 77 70.2 74.7 73
					L63.6 84
					L28 36
					Z
				"
				stroke={isOutline ? "currentColor" : "none"}
				strokeWidth={isOutline ? strokeWidth : undefined}
				strokeLinejoin="miter"
				strokeLinecap="butt"
				strokeMiterlimit={10}
				vectorEffect="non-scaling-stroke"
			/>

			{/* Right upper wing */}
			<path
				d="
					M78.9 36
					H93.6
					L74.4 61.7
					L68.3 56.2
					C65.9 54 66 50.7 67.8 48.5
					L78.9 36
					Z
				"
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

export function VelionMarkFilled({ className = "" }: { className?: string }) {
	return <VelionMark className={className} variant="filled" />;
}

export function VelionMarkOutline({
	className = "",
	strokeWidth = 1.8,
}: {
	className?: string;
	strokeWidth?: number;
}) {
	return (
		<VelionMark
			className={className}
			variant="outline"
			strokeWidth={strokeWidth}
		/>
	);
}

export default VelionMark;
