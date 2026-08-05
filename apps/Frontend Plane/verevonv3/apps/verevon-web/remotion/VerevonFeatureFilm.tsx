import { type ReactNode, useEffect, useState } from "react";
import {
	Easing,
	interpolate,
	useCurrentFrame,
	useDelayRender,
} from "remotion";
import {
	Eyebrow,
	FILM_STAGE,
	FilmSurface,
	Heading,
	IntegrationLogo,
	type PanelIndex,
	PanelFrame,
	UiCard,
	VerevonSymbol,
} from "./primitives";
import { easedSegment, getLoopOpacity } from "./timeline";
import { arbeit, COLORS, protokoll } from "./theme";

export type FeatureFilmKind =
	| "build"
	| "connect"
	| "ground"
	| "approve";

const panelMeta: Record<
	FeatureFilmKind,
	{ index: PanelIndex; label: string }
> = {
	build: { index: 0, label: "Agentoppsett" },
	connect: { index: 1, label: "Systemkobling" },
	ground: { index: 2, label: "Kildegrunnlag" },
	approve: { index: 3, label: "Godkjenning" },
};

const crispEase = Easing.bezier(0.16, 1, 0.3, 1);
const editorialEase = Easing.bezier(0.45, 0, 0.55, 1);

const motionProgress = (
	frame: number,
	start: number,
	end: number,
	easing = crispEase,
) =>
	interpolate(frame, [start, end], [0, 1], {
		easing,
		extrapolateLeft: "clamp",
		extrapolateRight: "clamp",
	});

// Each panel must look intentional even when it is not the active panel in
// the shared loop. `settled` retains a quiet, readable base state and raises
// the active state before easing it back at the loop reset.
const settled = (
	frame: number,
	start: number,
	end: number,
	base = 0.28,
) => {
	const active = easedSegment(frame, start, end);
	return base + (1 - base) * active;
};

type Point = { x: number; y: number };

type CubicRoute = {
	from: Point;
	controlA: Point;
	controlB: Point;
	to: Point;
};

const pointOnCubicRoute = (route: CubicRoute, progress: number): Point => {
	const t = Math.min(1, Math.max(0, progress));
	const inverse = 1 - t;

	return {
		x:
			inverse ** 3 * route.from.x +
			3 * inverse ** 2 * t * route.controlA.x +
			3 * inverse * t ** 2 * route.controlB.x +
			t ** 3 * route.to.x,
		y:
			inverse ** 3 * route.from.y +
			3 * inverse ** 2 * t * route.controlA.y +
			3 * inverse * t ** 2 * route.controlB.y +
			t ** 3 * route.to.y,
	};
};

const routePath = (route: CubicRoute) =>
	`M${route.from.x} ${route.from.y} C${route.controlA.x} ${route.controlA.y} ${route.controlB.x} ${route.controlB.y} ${route.to.x} ${route.to.y}`;

function CheckMark({ size = 30 }: { size?: number }) {
	return (
		<div
			style={{
				alignItems: "center",
				background: COLORS.success,
				borderRadius: 999,
				color: COLORS.white,
				display: "flex",
				fontSize: size * 0.58,
				height: size,
				justifyContent: "center",
				width: size,
				...protokoll,
			}}
		>
			✓
		</div>
	);
}

function SceneStage({
	children,
	frame,
}: {
	children: ReactNode;
	frame: number;
}) {
	return (
		<FilmSurface
			style={{
				height: FILM_STAGE.height,
				left: FILM_STAGE.left,
				opacity: getLoopOpacity(frame),
				top: FILM_STAGE.top,
				width: FILM_STAGE.width,
			}}
		>
			{children}
		</FilmSurface>
	);
}

// Paths and cards share the same stage coordinates. Every path finishes a few
// pixels inside its destination, so the destination card visually owns the
// join instead of leaving an anti-aliased gap.
function ConnectorPath({
	d,
	progress,
}: {
	d: string;
	progress: number;
}) {
	const draw = 0.24 + progress * 0.76;
	return (
		<path
			d={d}
			fill="none"
			pathLength={1}
			stroke={COLORS.coral}
			strokeDasharray="1"
			strokeDashoffset={1 - draw}
			strokeLinecap="round"
			strokeOpacity={0.18 + progress * 0.58}
			strokeWidth="3"
		/>
	);
}

function ConnectorLayer({ children }: { children: ReactNode }) {
	return (
		<svg
			aria-hidden="true"
			height={FILM_STAGE.height}
			style={{
				inset: 0,
				position: "absolute",
				overflow: "visible",
			}}
			viewBox={`0 0 ${FILM_STAGE.width} ${FILM_STAGE.height}`}
			width={FILM_STAGE.width}
		>
			{children}
		</svg>
	);
}

function RoutePulse({
	opacity = 1,
	progress,
	route,
}: {
	opacity?: number;
	progress: number;
	route: CubicRoute;
}) {
	const point = pointOnCubicRoute(route, progress);

	return (
		<>
			<circle
				cx={point.x}
				cy={point.y}
				fill="rgba(238,122,80,0.18)"
				opacity={opacity}
				r={18}
			/>
			<circle
				cx={point.x}
				cy={point.y}
				fill={COLORS.coral}
				opacity={opacity}
				r={6}
			/>
		</>
	);
}

function SmallNode({
	active,
	label,
	progress,
	xOffset = 0,
	yOffset = -48,
}: {
	active: number;
	label: string;
	progress: number;
	xOffset?: number;
	yOffset?: number;
}) {
	return (
		<UiCard
			style={{
				alignItems: "center",
				display: "flex",
				justifyContent: "center",
				opacity: progress,
				padding: "18px 18px",
				scale: 0.92 + active * 0.08,
				translate: `${(1 - active) * xOffset}px ${(1 - active) * yOffset}px`,
			}}
		>
			<div style={{ fontSize: 23, ...protokoll }}>{label}</div>
		</UiCard>
	);
}

function BuildFilm({ frame }: { frame: number }) {
	const taskActive = motionProgress(frame, 15, 42);
	const task = settled(frame, 15, 42, 0.48);
	const agentActive = motionProgress(frame, 45, 77);
	const agent = settled(frame, 45, 77, 0.34);
	const badgeActive = motionProgress(frame, 55, 72);
	const agentScale =
		agentActive < 0.74
			? interpolate(agentActive, [0, 0.74], [0.88, 1.035], {
					extrapolateLeft: "clamp",
					extrapolateRight: "clamp",
				})
			: interpolate(agentActive, [0.74, 1], [1.035, 1], {
					extrapolateLeft: "clamp",
					extrapolateRight: "clamp",
				});
	const nodeActive = [
		motionProgress(frame, 52, 68),
		motionProgress(frame, 59, 75),
		motionProgress(frame, 66, 82),
	];
	const nodeProgress = [
		settled(frame, 52, 68, 0.32),
		settled(frame, 59, 75, 0.32),
		settled(frame, 66, 82, 0.32),
	];
	const taskScan = motionProgress(frame, 23, 42, editorialEase);

	return (
		<SceneStage frame={frame}>
			<Eyebrow>Ny oppgave</Eyebrow>

			<div
				style={{
					left: 0,
					opacity: task,
					position: "absolute",
					top: 62,
					translate: `0 ${(1 - taskActive) * 20}px`,
					width: FILM_STAGE.width,
				}}
			>
				<UiCard
					style={{
						overflow: "hidden",
						padding: "24px 30px",
						position: "relative",
					}}
				>
					<div
						style={{
							background:
								"linear-gradient(90deg, transparent, rgba(238,122,80,0.17), transparent)",
							bottom: 0,
							left: `${taskScan * 115 - 15}%`,
							position: "absolute",
							top: 0,
							width: "18%",
						}}
					/>
					<div
						style={{
							alignItems: "center",
							display: "flex",
							justifyContent: "space-between",
						}}
					>
						<Heading size={46}>Følg opp leverandøravvik</Heading>
						<div
							style={{
								background: COLORS.coralSoft,
								borderRadius: 999,
								color: COLORS.coralDeep,
								fontSize: 19,
								padding: "11px 16px",
								...protokoll,
							}}
						>
							oppgave
						</div>
					</div>
				</UiCard>
			</div>

			<ConnectorLayer>
				<ConnectorPath
					d="M432 430 C432 494 120 500 120 568"
					progress={agentActive * nodeActive[0]}
				/>
				<ConnectorPath
					d="M432 430 C432 500 432 512 432 568"
					progress={agentActive * nodeActive[1]}
				/>
				<ConnectorPath
					d="M432 430 C432 494 744 500 744 568"
					progress={agentActive * nodeActive[2]}
				/>
			</ConnectorLayer>

			<div
				style={{
					left: 154,
					opacity: agent,
					position: "absolute",
					scale: agentScale,
					top: 272,
					translate: `0 ${(1 - agentActive) * 34}px`,
					width: 556,
				}}
			>
				<UiCard
					style={{
						background: COLORS.ink,
						borderColor: "rgba(23,23,23,0.92)",
						color: COLORS.white,
						padding: "30px 34px",
					}}
				>
					<div
						style={{ alignItems: "center", display: "flex", gap: 20 }}
					>
						<div
							style={{
								alignItems: "center",
								background: COLORS.coral,
								borderRadius: 999,
								display: "flex",
								fontSize: 34,
								height: 70,
								justifyContent: "center",
								opacity: 0.42 + badgeActive * 0.58,
								scale: 0.62 + badgeActive * 0.38,
								width: 70,
								...arbeit,
							}}
						>
							V
						</div>
						<div>
							<Heading size={48}>Avviksagent klar</Heading>
							<div
								style={{
									color: "rgba(255,255,255,0.62)",
									fontSize: 20,
									marginTop: 9,
									...protokoll,
								}}
							>
								mål · kunnskap · verktøy
							</div>
						</div>
					</div>
				</UiCard>
			</div>

			<div
				style={{
					display: "grid",
					gap: 34,
					gridTemplateColumns: "repeat(3, 1fr)",
					left: 0,
					position: "absolute",
					top: 560,
					width: FILM_STAGE.width,
				}}
			>
				<SmallNode
					active={nodeActive[0]}
					label="Mål"
					progress={nodeProgress[0]}
					xOffset={210}
				/>
				<SmallNode
					active={nodeActive[1]}
					label="Kunnskap"
					progress={nodeProgress[1]}
				/>
				<SmallNode
					active={nodeActive[2]}
					label="Verktøy"
					progress={nodeProgress[2]}
					xOffset={-210}
				/>
			</div>
		</SceneStage>
	);
}

function ConnectFilm({ frame }: { frame: number }) {
	const systems = [
		{ label: "Dokumenter", type: "archive" as const },
		{ label: "ERP", type: "database" as const },
		{ label: "CRM", type: "blocks" as const },
		{ label: "MCP", type: "nodes" as const },
	];
	const topRoutes: CubicRoute[] = [108, 324, 540, 756].map((x) => ({
		controlA: { x, y: 272 },
		controlB: { x: 432, y: 270 },
		from: { x, y: 190 },
		to: { x: 432, y: 348 },
	}));
	const bottomRoutes: CubicRoute[] = [168, 432, 696].map((x) => ({
		controlA: { x: 432, y: 508 },
		controlB: { x, y: 508 },
		from: { x: 432, y: 432 },
		to: { x, y: 582 },
	}));
	const systemActive = systems.map((_, index) =>
		motionProgress(frame, 84 + index * 6, 99 + index * 6),
	);
	const topRouteProgress = topRoutes.map((_, index) =>
		motionProgress(
			frame,
			91 + index * 6,
			118 + index * 3,
			editorialEase,
		),
	);
	const contextActive = motionProgress(frame, 105, 133);
	const context = settled(frame, 105, 133, 0.34);
	const integrationActive = bottomRoutes.map((_, index) =>
		motionProgress(frame, 120 + index * 7, 145 + index * 2, editorialEase),
	);
	const success = settled(frame, 137, 149, 0.24);

	return (
		<SceneStage frame={frame}>
			<Eyebrow>Systemer</Eyebrow>

			<ConnectorLayer>
				{topRoutes.map((route, index) => {
					const pulseOpacity =
						Math.sin(topRouteProgress[index] * Math.PI);
					return (
						<g key={`owned-${route.from.x}`}>
							<ConnectorPath
								d={routePath(route)}
								progress={topRouteProgress[index]}
							/>
							<RoutePulse
								opacity={pulseOpacity}
								progress={topRouteProgress[index]}
								route={route}
							/>
						</g>
					);
				})}
				{bottomRoutes.map((route, index) => {
					const pulseOpacity =
						Math.sin(integrationActive[index] * Math.PI);
					return (
						<g key={`trusted-${route.to.x}`}>
							<ConnectorPath
								d={routePath(route)}
								progress={integrationActive[index]}
							/>
							<RoutePulse
								opacity={pulseOpacity}
								progress={integrationActive[index]}
								route={route}
							/>
						</g>
					);
				})}
			</ConnectorLayer>

			<div
				style={{
					display: "grid",
					gridTemplateColumns: "repeat(4, 1fr)",
					left: 0,
					position: "absolute",
					top: 80,
					width: FILM_STAGE.width,
				}}
			>
				{systems.map((system, index) => (
					<div
						key={system.label}
						style={{
							filter: `grayscale(${1 - systemActive[index]})`,
							opacity: 0.38 + systemActive[index] * 0.62,
							scale: 0.9 + systemActive[index] * 0.1,
							translate: `0 ${(1 - systemActive[index]) * 15}px`,
						}}
					>
						<VerevonSymbol label={system.label} type={system.type} />
					</div>
				))}
			</div>

			<div
				style={{
					left: 140,
					opacity: context,
					position: "absolute",
					scale: 0.94 + contextActive * 0.06,
					top: 330,
					width: 584,
				}}
			>
				<UiCard
					style={{
						alignItems: "center",
						display: "flex",
						justifyContent: "center",
						padding: "25px 28px",
					}}
				>
					<div style={{ alignItems: "center", display: "flex", gap: 15 }}>
						<span
							style={{
								background: COLORS.coral,
								borderRadius: 999,
								boxShadow: `0 0 ${14 + contextActive * 24}px rgba(238,122,80,${0.1 + contextActive * 0.2})`,
								height: 15,
								width: 15,
							}}
						/>
						<Heading size={42}>Verevon-kontekst</Heading>
					</div>
				</UiCard>
			</div>

			<div
				style={{
					display: "grid",
					gridTemplateColumns: "repeat(3, 1fr)",
					left: 0,
					position: "absolute",
					top: 574,
					width: FILM_STAGE.width,
				}}
			>
				{(["outlook", "sharepoint", "bring"] as const).map(
					(name, index) => (
						<div
							key={name}
							style={{
								opacity: 0.42 + integrationActive[index] * 0.58,
								scale: 0.9 + integrationActive[index] * 0.1,
								translate: `0 ${(1 - integrationActive[index]) * 15}px`,
							}}
						>
							<IntegrationLogo
								connected={integrationActive[index]}
								name={name}
							/>
						</div>
					),
				)}
			</div>

			<div
				style={{
					alignItems: "center",
					color: COLORS.success,
					display: "flex",
					fontSize: 20,
					gap: 11,
					justifyContent: "center",
					left: 0,
					opacity: success,
					position: "absolute",
					top: 730,
					width: FILM_STAGE.width,
					...protokoll,
				}}
			>
				<CheckMark size={27} /> Systemene er koblet
			</div>
		</SceneStage>
	);
}

function GroundFilm({ frame }: { frame: number }) {
	const sourceActive = [
		motionProgress(frame, 150, 168, editorialEase),
		motionProgress(frame, 160, 178, editorialEase),
		motionProgress(frame, 170, 188, editorialEase),
	];
	const sourceProgress = [
		settled(frame, 150, 168, 0.36),
		settled(frame, 160, 178, 0.36),
		settled(frame, 170, 188, 0.36),
	];
	const stampActive = [
		motionProgress(frame, 159, 171),
		motionProgress(frame, 169, 181),
		motionProgress(frame, 179, 191),
	];
	const contextActive = motionProgress(frame, 186, 217, editorialEase);
	const context = settled(frame, 186, 217, 0.36);
	const sources = [
		{ label: "Policy 04", reference: "[01]", rotate: -2.4, x: 64 },
		{ label: "Avtalehistorikk", reference: "[02]", rotate: 0, x: 0 },
		{ label: "Ordre 52481", reference: "[03]", rotate: 2.4, x: -64 },
	];

	return (
		<SceneStage frame={frame}>
			<Eyebrow>Kildegrunnlag</Eyebrow>

			<ConnectorLayer>
				{[144, 432, 720].map((x, index) => (
					<ConnectorPath
						d={`M${x} 200 C${x} 294 432 292 432 390`}
						key={`source-${x}`}
						progress={sourceActive[index] * contextActive}
					/>
				))}
			</ConnectorLayer>

			<div
				style={{
					display: "grid",
					gap: 18,
					gridTemplateColumns: "repeat(3, 1fr)",
					left: 0,
					position: "absolute",
					top: 82,
					width: FILM_STAGE.width,
				}}
			>
				{sources.map((source, index) => {
					const progress = sourceProgress[index];
					const active = sourceActive[index];
					const stamp = stampActive[index];
					const stampScale =
						stamp < 0.72
							? interpolate(stamp, [0, 0.72], [0.64, 1.12], {
									extrapolateLeft: "clamp",
									extrapolateRight: "clamp",
								})
							: interpolate(stamp, [0.72, 1], [1.12, 1], {
									extrapolateLeft: "clamp",
									extrapolateRight: "clamp",
								});
					return (
						<UiCard
							key={source.reference}
							style={{
								opacity: progress,
								padding: "22px 20px",
								rotate: `${(1 - active) * source.rotate}deg`,
								scale: 0.94 + active * 0.06,
								translate: `${(1 - active) * source.x}px ${(1 - active) * 22}px`,
							}}
						>
							<div
								style={{
									alignItems: "center",
									border: `1px solid rgba(238,122,80,${0.14 + stamp * 0.34})`,
									borderRadius: 999,
									color: COLORS.coral,
									display: "flex",
									fontSize: 19,
									fontWeight: 500,
									height: 42,
									justifyContent: "center",
									scale: stampScale,
									width: 58,
									...protokoll,
								}}
							>
								{source.reference}
							</div>
							<div
								style={{
									fontSize: 28,
									lineHeight: 1.06,
									marginTop: 14,
									...arbeit,
								}}
							>
								{source.label}
							</div>
						</UiCard>
					);
				})}
			</div>

			<div
				style={{
					left: 62,
					opacity: context,
					position: "absolute",
					scale: `1 ${0.88 + contextActive * 0.12}`,
					top: 382,
					transformOrigin: "top center",
					width: 740,
				}}
			>
				<UiCard
					style={{
						borderColor: "rgba(238,122,80,0.34)",
						clipPath: `inset(0 0 ${(1 - contextActive) * 24}% 0 round 22px)`,
						padding: "32px 34px",
						position: "relative",
					}}
				>
					<div
						style={{
							background: COLORS.coral,
							height: 3,
							left: 0,
							position: "absolute",
							top: 0,
							width: `${contextActive * 100}%`,
						}}
					/>
					<div
						style={{
							alignItems: "center",
							display: "flex",
							justifyContent: "space-between",
						}}
					>
						<Eyebrow>Forankret svar</Eyebrow>
						<span
							style={{ color: COLORS.success, fontSize: 19, ...protokoll }}
						>
							3 kilder
						</span>
					</div>
					<div style={{ marginTop: 24 }}>
						<Heading size={52}>
							Krever godkjenning før status endres
						</Heading>
					</div>
					<div
						style={{
							color: COLORS.muted,
							fontSize: 19,
							marginTop: 24,
							...protokoll,
						}}
					>
						[01] · [02] · [03]
					</div>
				</UiCard>
			</div>
		</SceneStage>
	);
}

function ApproveFilm({ frame }: { frame: number }) {
	const actionActive = motionProgress(frame, 222, 249);
	const action = settled(frame, 222, 249, 0.48);
	const gateReach = motionProgress(frame, 232, 252, editorialEase);
	const waitingActive = motionProgress(frame, 252, 276);
	const waitingReveal = settled(frame, 252, 276, 0.36);
	const gateOpen = motionProgress(frame, 288, 300);
	const release = motionProgress(frame, 288, 310, editorialEase);
	const complete = motionProgress(frame, 291, 317);
	const waiting = waitingReveal * (1 - complete);
	const holdPulse =
		frame >= 252 && frame < 288
			? 0.5 + Math.sin((frame - 252) * 0.36) * 0.5
			: 0;
	const holdEnvelope = 1 - motionProgress(frame, 280, 288, editorialEase);
	const pulse = holdPulse * waitingActive * holdEnvelope;
	const signalY =
		frame < 288
			? interpolate(gateReach, [0, 1], [235, 358])
			: interpolate(release, [0, 1], [358, 430]);
	const signalOpacity = 0.28 + Math.max(gateReach, release) * 0.72;

	return (
		<SceneStage frame={frame}>
			<Eyebrow>Foreslått handling</Eyebrow>

			<ConnectorLayer>
				<ConnectorPath
					d="M432 235 C432 292 432 326 432 358"
					progress={actionActive * gateReach}
				/>
				<ConnectorPath
					d="M432 376 C432 394 432 408 432 430"
					progress={gateOpen * release}
				/>
				<circle
					cx={432}
					cy={signalY}
					fill="rgba(238,122,80,0.18)"
					opacity={signalOpacity}
					r={18 + pulse * 6}
				/>
				<circle
					cx={432}
					cy={signalY}
					fill={release > 0.88 ? COLORS.success : COLORS.coral}
					opacity={signalOpacity}
					r={7}
				/>
			</ConnectorLayer>

			<div
				style={{
					left: 0,
					opacity: action,
					position: "absolute",
					scale: 0.96 + actionActive * 0.04,
					top: 82,
					translate: `0 ${(1 - actionActive) * -18}px`,
					width: FILM_STAGE.width,
				}}
			>
				<UiCard style={{ padding: "31px 34px" }}>
					<div
						style={{
							alignItems: "flex-start",
							display: "flex",
							justifyContent: "space-between",
						}}
					>
						<div>
							<Heading size={52}>Oppdater ordrestatus</Heading>
							<div
								style={{
									color: COLORS.muted,
									fontSize: 20,
									marginTop: 14,
									...protokoll,
								}}
							>
								Ordre 52481
							</div>
						</div>
						<div
							style={{
								background: COLORS.successSoft,
								borderRadius: 999,
								color: COLORS.success,
								fontSize: 19,
								padding: "11px 16px",
								...protokoll,
							}}
						>
							Policy 04 ✓
						</div>
					</div>
				</UiCard>
			</div>

			<div
				style={{
					color: COLORS.muted,
					fontSize: 16,
					left: 0,
					letterSpacing: "0.13em",
					opacity: 0.34 + waitingActive * 0.66,
					position: "absolute",
					textAlign: "center",
					textTransform: "uppercase",
					top: 322,
					width: FILM_STAGE.width,
					...protokoll,
				}}
			>
				Kontrollpunkt
			</div>
			<div
				style={{
					background: COLORS.coral,
					height: 3,
					left: 376,
					opacity: 0.34 + waitingActive * 0.66,
					position: "absolute",
					top: 366,
					translate: `${gateOpen * -28}px 0`,
					width: 52,
				}}
			/>
			<div
				style={{
					background: COLORS.coral,
					height: 3,
					left: 436,
					opacity: 0.34 + waitingActive * 0.66,
					position: "absolute",
					top: 366,
					translate: `${gateOpen * 28}px 0`,
					width: 52,
				}}
			/>

			<div
				style={{
					left: 126,
					opacity: waiting,
					position: "absolute",
					scale: 0.98 + waitingActive * 0.02 + pulse * 0.018,
					top: 420,
					translate: `0 ${(1 - waitingActive) * 16}px`,
					width: 612,
				}}
			>
				<UiCard
					style={{
						alignItems: "center",
						borderColor: `rgba(238,122,80,${0.26 + waitingActive * 0.18 + pulse * 0.24})`,
						display: "flex",
						gap: 19,
						padding: "27px 31px",
					}}
				>
					<div
						style={{
							background: COLORS.coral,
							borderRadius: 999,
							boxShadow: `0 0 ${26 + pulse * 24}px rgba(238,122,80,${0.15 + pulse * 0.18})`,
							height: 17,
							width: 17,
						}}
					/>
					<Heading size={42}>Venter på godkjenning</Heading>
				</UiCard>
			</div>

			<div
				style={{
					left: 126,
					opacity: complete,
					position: "absolute",
					top: 420,
					translate: `0 ${(1 - complete) * 12}px`,
					width: 612,
				}}
			>
				<UiCard
					style={{
						background: COLORS.successSoft,
						borderColor: "rgba(54,117,90,0.25)",
						clipPath: `inset(0 ${(1 - complete) * 100}% 0 0 round 22px)`,
						padding: "28px 32px",
					}}
				>
					<div style={{ alignItems: "center", display: "flex", gap: 18 }}>
						<CheckMark size={48} />
						<div>
							<Heading size={48}>Utført</Heading>
							<div
								style={{
									color: COLORS.success,
									fontSize: 20,
									marginTop: 8,
									...protokoll,
								}}
							>
								Ida godkjente · revisjon lagret
							</div>
						</div>
					</div>
				</UiCard>
			</div>
		</SceneStage>
	);
}

function FilmContent({
	frame,
	kind,
}: {
	frame: number;
	kind: FeatureFilmKind;
}) {
	switch (kind) {
		case "build":
			return <BuildFilm frame={frame} />;
		case "connect":
			return <ConnectFilm frame={frame} />;
		case "ground":
			return <GroundFilm frame={frame} />;
		case "approve":
			return <ApproveFilm frame={frame} />;
		default: {
			const exhaustiveKind: never = kind;
			return exhaustiveKind;
		}
	}
}

export function VerevonFeatureFilm({ kind }: { kind: FeatureFilmKind }) {
	const frame = useCurrentFrame();
	const {
		continueRender: continueScopedRender,
		delayRender: delayScopedRender,
	} = useDelayRender();
	const [fontHandle] = useState(() =>
		delayScopedRender("Waiting for Verevon brand fonts"),
	);
	const { index, label } = panelMeta[kind];

	useEffect(() => {
		let active = true;
		const releaseRender = () => {
			if (!active) return;
			continueScopedRender(fontHandle);
		};

		document.fonts.ready.then(releaseRender, releaseRender);

		return () => {
			active = false;
		};
	}, [continueScopedRender, fontHandle]);

	return (
		<PanelFrame frame={frame} index={index} label={label}>
			<FilmContent frame={frame} kind={kind} />
		</PanelFrame>
	);
}
