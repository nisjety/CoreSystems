import type { ReactNode } from "react";

type EyebrowProps = {
	children: ReactNode;
	className?: string;
	/** Show a leading terracotta tick — reserved for trust/governance contexts. */
	marker?: boolean;
};

/** Small uppercase kicker above a heading. Token-backed (.verevon-eyebrow). */
export function Eyebrow({ children, className = "", marker = false }: EyebrowProps) {
	return (
		<p
			className={["verevon-eyebrow inline-flex items-center gap-3", className]
				.filter(Boolean)
				.join(" ")}
		>
			{marker ? (
				<span
					aria-hidden="true"
					className="h-px w-8 bg-verevon-coral/70"
				/>
			) : null}
			{children}
		</p>
	);
}

type SectionHeadingProps = {
	eyebrow?: ReactNode;
	eyebrowMarker?: boolean;
	title: ReactNode;
	lede?: ReactNode;
	align?: "left" | "center";
	className?: string;
	titleClassName?: string;
};

/**
 * SectionHeading — consistent eyebrow + headline + lede block.
 * Used by new sections (Trust) and available for opportunistic migration.
 */
export function SectionHeading({
	eyebrow,
	eyebrowMarker = false,
	title,
	lede,
	align = "left",
	className = "",
	titleClassName = "",
}: SectionHeadingProps) {
	const isCenter = align === "center";

	return (
		<div
			className={[
				isCenter ? "mx-auto max-w-[920px] text-center" : "max-w-[760px]",
				className,
			]
				.filter(Boolean)
				.join(" ")}
		>
			{eyebrow ? (
				<div className={isCenter ? "flex justify-center" : ""}>
					<Eyebrow marker={eyebrowMarker}>{eyebrow}</Eyebrow>
				</div>
			) : null}

			<h2
				className={[
					"verevon-h2 mt-6 text-balance",
					titleClassName,
				]
					.filter(Boolean)
					.join(" ")}
			>
				{title}
			</h2>

			{lede ? (
				<p
					className={[
						"verevon-body-lg mt-7 text-pretty",
						isCenter ? "mx-auto max-w-[680px]" : "max-w-[600px]",
					].join(" ")}
				>
					{lede}
				</p>
			) : null}
		</div>
	);
}

export default SectionHeading;
