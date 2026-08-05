import type { CSSProperties, ReactNode } from "react";
import { AbsoluteFill, Img, interpolate, staticFile } from "remotion";
import { getLoopOpacity, getSignalPosition } from "./timeline";
import {
	arbeit,
	cardStyle,
	COLORS,
	FONT_CSS,
	protokoll,
} from "./theme";

const clamp = (value: number) => Math.min(1, Math.max(0, value));

export type PanelIndex = 0 | 1 | 2 | 3;

// The panels are shown as small tiles in a four-up layout. This is the one
// shared stage for readable content; it deliberately has no visible frame.
export const FILM_STAGE = {
	height: 764,
	left: 48,
	top: 124,
	width: 864,
} as const;

const FILM_BACKGROUNDS: Record<PanelIndex, string> = {
	0: "feature-film-backgrounds/build-agent-pastel.jpg",
	1: "feature-film-backgrounds/connect-systems-pastel.jpg",
	2: "feature-film-backgrounds/ground-source-orange.jpg",
	3: "feature-film-backgrounds/approve-action-orange.jpg",
};

const FILM_BACKGROUND_MOTION: Record<
	PanelIndex,
	{
		x: [number, number];
		y: [number, number];
		scale: [number, number];
		imageOpacity: number;
		brightness: number;
		overlay: string;
	}
> = {
	0: {
		x: [-1.5, 1.5],
		y: [0, -1],
		scale: [1.08, 1.12],
		imageOpacity: 0.62,
		brightness: 1.2,
		overlay:
			"linear-gradient(180deg, rgba(255,254,250,0.38) 0%, rgba(255,254,250,0.27) 100%)",
	},
	1: {
		x: [1.5, -1.5],
		y: [0, 1],
		scale: [1.08, 1.14],
		imageOpacity: 0.7,
		brightness: 1.18,
		overlay:
			"linear-gradient(180deg, rgba(255,254,250,0.32) 0%, rgba(255,254,250,0.2) 68%, rgba(238,122,80,0.08) 100%)",
	},
	2: {
		x: [-1, 1],
		y: [1, -1],
		scale: [1.1, 1.15],
		imageOpacity: 0.62,
		brightness: 1.16,
		overlay:
			"linear-gradient(180deg, rgba(255,254,250,0.34) 0%, rgba(255,254,250,0.22) 100%)",
	},
	3: {
		x: [1, -1],
		y: [0, -1.5],
		scale: [1.08, 1.13],
		imageOpacity: 0.64,
		brightness: 1.12,
		overlay:
			"linear-gradient(180deg, rgba(255,254,250,0.3) 0%, rgba(255,254,250,0.18) 55%, rgba(255,241,231,0.1) 100%)",
	},
};

function FilmBackground({
	frame,
	index,
}: {
	frame: number;
	index: PanelIndex;
}) {
	const motion = FILM_BACKGROUND_MOTION[index];
	const progress = interpolate(frame, [0, 359], [0, 1], {
		extrapolateLeft: "clamp",
		extrapolateRight: "clamp",
	});
	const x = interpolate(progress, [0, 1], motion.x);
	const y = interpolate(progress, [0, 1], motion.y);
	const scale = interpolate(progress, [0, 1], motion.scale);

	return (
		<div
			aria-hidden="true"
			style={{
				inset: 0,
				overflow: "hidden",
				pointerEvents: "none",
				position: "absolute",
				zIndex: 0,
			}}
		>
			<Img
				src={staticFile(FILM_BACKGROUNDS[index])}
				style={{
					height: "calc(100% + 48px)",
					left: "calc(-24px - 1%)",
					objectFit: "cover",
					opacity: motion.imageOpacity * getLoopOpacity(frame),
					position: "absolute",
					top: "calc(-24px - 1%)",
					transform: `translate3d(${x}%, ${y}%, 0) scale(${scale})`,
					width: "calc(100% + 48px)",
					filter: `saturate(1.02) contrast(0.94) brightness(${motion.brightness}) blur(0.25px)`,
				}}
			/>
			<div
				style={{
					background: motion.overlay,
					inset: 0,
					position: "absolute",
				}}
			/>
			<div
				style={{
					background:
						"radial-gradient(circle at 50% 28%, rgba(255,255,255,0.05), transparent 46%), linear-gradient(180deg, rgba(255,254,250,0.035), rgba(255,254,250,0.025))",
					inset: 0,
					position: "absolute",
				}}
			/>
		</div>
	);
}

export function PanelFrame({
	children,
	frame,
	index,
	label,
}: {
	children: ReactNode;
	frame: number;
	index: PanelIndex;
	label: string;
}) {
	return (
		<AbsoluteFill
			style={{
				background: "#f7f6f2",
				color: COLORS.ink,
				overflow: "hidden",
			}}
		>
			<style>{FONT_CSS}</style>
			<FilmBackground frame={frame} index={index} />

			<div
				style={{
					position: "absolute",
					inset: 0,
					backgroundImage:
						"linear-gradient(90deg, rgba(23,23,23,0.045) 1px, transparent 1px), linear-gradient(rgba(23,23,23,0.04) 1px, transparent 1px)",
					backgroundSize: "120px 120px",
					opacity: 0.24,
				}}
			/>
			<div
				style={{
					position: "absolute",
					inset: 0,
					background:
						index === 3
						? "radial-gradient(circle at 72% 18%, rgba(238,122,80,0.12), transparent 31%), linear-gradient(180deg, rgba(255,255,255,0.08), transparent 52%)"
						: "radial-gradient(circle at 28% 16%, rgba(238,122,80,0.085), transparent 27%), linear-gradient(180deg, rgba(255,255,255,0.1), transparent 54%)",
				}}
			/>

			<header
				style={{
					alignItems: "center",
					borderBottom: `1px solid ${COLORS.line}`,
					display: "flex",
				height: 88,
					justifyContent: "space-between",
				left: 44,
					position: "absolute",
				right: 44,
					top: 0,
					...protokoll,
				}}
			>
				<div
					style={{
						alignItems: "center",
						display: "flex",
					fontSize: 20,
						fontWeight: 500,
					gap: 12,
						letterSpacing: "0.08em",
						textTransform: "uppercase",
					}}
				>
					<span
						style={{
							background: COLORS.coral,
							borderRadius: 999,
						height: 8,
						width: 8,
						}}
					/>
					Verevon
				</div>
				<div
					style={{
						color: COLORS.muted,
						fontSize: 17,
						letterSpacing: "0.13em",
						textTransform: "uppercase",
					}}
				>
					{label}
				</div>
			</header>

			{children}
			<SignalRail frame={frame} index={index} />
		</AbsoluteFill>
	);
}

export function SignalRail({
	frame,
	index,
}: {
	frame: number;
	index: PanelIndex;
}) {
	const signalPosition = getSignalPosition(frame);
	const loopOpacity = getLoopOpacity(frame);
	const panelStart = index * 0.25;
	const panelEnd = panelStart + 0.25;
	const localProgress = clamp((signalPosition - panelStart) / 0.25);
	const signalIsHere =
		signalPosition >= panelStart &&
		(index === 3 ? signalPosition <= panelEnd : signalPosition < panelEnd);
	const dotX = 44 + localProgress * 872;

	return (
		<div
			style={{
				height: 58,
				left: 0,
				opacity: loopOpacity,
				position: "absolute",
				right: 0,
				top: 926,
			}}
		>
			<div
				style={{
					background: "rgba(23,23,23,0.11)",
					height: 1,
					left: 0,
					position: "absolute",
					right: 0,
					top: 28,
				}}
			/>
			<div
				style={{
					background: "rgba(238,122,80,0.46)",
					height: 3,
					left: 0,
					position: "absolute",
					top: 27,
					width: dotX,
				}}
			/>
			{signalIsHere ? (
				<>
					<div
						style={{
							background: "rgba(238,122,80,0.17)",
							borderRadius: 999,
							filter: "blur(13px)",
						height: 54,
						left: dotX - 27,
							position: "absolute",
						top: 2,
						width: 54,
						}}
					/>
					<div
						style={{
							background: COLORS.coral,
						border: "4px solid rgba(255,255,255,0.96)",
							borderRadius: 999,
							boxShadow: "0 4px 18px rgba(238,122,80,0.38)",
						height: 19,
						left: dotX - 9.5,
							position: "absolute",
						top: 18.5,
						width: 19,
						}}
					/>
				</>
			) : null}
		</div>
	);
}

export function FilmSurface({
	children,
	style,
}: {
	children: ReactNode;
	style?: CSSProperties;
}) {
	return (
		<div
			style={{
				position: "absolute",
				// The panel frame is the only visible container. This reserves
				// scene space without creating a second card inside the card.
				overflow: "visible",
				...style,
			}}
		>
			{children}
		</div>
	);
}

export function UiCard({
	children,
	style,
}: {
	children: ReactNode;
	style?: CSSProperties;
}) {
	return (
		<div
			style={{
				borderRadius: 22,
				padding: "26px 28px",
				...cardStyle,
				...style,
			}}
		>
			{children}
		</div>
	);
}

export function Eyebrow({ children }: { children: ReactNode }) {
	return (
		<div
			style={{
				color: COLORS.muted,
				fontSize: 19,
				fontWeight: 500,
				letterSpacing: "0.13em",
				textTransform: "uppercase",
				...protokoll,
			}}
		>
			{children}
		</div>
	);
}

export function Heading({
	children,
		size = 44,
}: {
	children: ReactNode;
	size?: number;
}) {
	return (
		<div
			style={{
				fontSize: size,
				fontWeight: 300,
				letterSpacing: "-0.045em",
				lineHeight: 1.02,
				...arbeit,
			}}
		>
			{children}
		</div>
	);
}

export function VerevonSymbol({
	label,
	type,
}: {
	label: string;
	type: "archive" | "blocks" | "database" | "nodes" | "policy" | "tool";
}) {
	const glyphs = {
		archive: "□",
		blocks: "∷",
		database: "◎",
		nodes: "⌘",
		policy: "◇",
		tool: "↗",
	} as const;

	return (
		<div
			style={{
				alignItems: "center",
				display: "flex",
				flexDirection: "column",
				gap: 14,
				justifyContent: "center",
			}}
		>
			<div
				style={{
					alignItems: "center",
					background: COLORS.paper,
					border: `1px solid ${COLORS.lineStrong}`,
					borderRadius: 24,
					boxShadow: "0 12px 30px rgba(31,28,25,0.07)",
					display: "flex",
					fontSize: 38,
					height: 94,
					justifyContent: "center",
					width: 94,
					...arbeit,
				}}
			>
				{glyphs[type]}
			</div>
			<div
				style={{
					color: COLORS.muted,
					fontSize: 20,
					...protokoll,
				}}
			>
				{label}
			</div>
		</div>
	);
}

const INTEGRATIONS = {
	bring: { label: "Bring", path: "brand-logos/bring.svg" },
	outlook: { label: "Outlook", path: "brand-logos/outlook.svg" },
	sharepoint: { label: "SharePoint", path: "brand-logos/sharepoint.svg" },
} as const;

export type IntegrationName = keyof typeof INTEGRATIONS;

export function IntegrationLogo({
	connected,
	name,
}: {
	connected: number;
	name: IntegrationName;
}) {
	const integration = INTEGRATIONS[name];

	return (
		<div
			style={{
				alignItems: "center",
				display: "flex",
				flexDirection: "column",
				gap: 13,
				justifyContent: "center",
				opacity: 0.48 + connected * 0.52,
			}}
		>
			<div
				style={{
					alignItems: "center",
					background: COLORS.white,
					border: `1px solid ${COLORS.line}`,
					borderRadius: 24,
					boxShadow: "0 12px 30px rgba(31,28,25,0.07)",
					display: "flex",
					height: 94,
					justifyContent: "center",
					width: 94,
				}}
			>
				<Img
					src={staticFile(integration.path)}
					style={{
						filter: `grayscale(${1 - connected})`,
						height: 52,
						objectFit: "contain",
						width: 52,
					}}
				/>
			</div>
			<div
				style={{
					color: COLORS.muted,
					fontSize: 20,
					...protokoll,
				}}
			>
				{integration.label}
			</div>
		</div>
	);
}
