"use client";

import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, MutableRefObject } from "react";

const SIGNAL_BLOB_D =
	"M55.1257 34C40.6277 34 34 36.52 34 44.5C34 54.16 38.9708 55 55.1257 55C78.4606 55 121.817 45.34 120.988 44.5C119.817 43.312 78.4606 34 55.1257 34Z";

const SIGNAL_BLOB_ORIGIN = {
	x: 77.49985885620117,
	y: 44.5,
};

const SIGNAL_BLOB_STYLE: CSSProperties = {
	opacity: 1,
	rotate: "none",
	scale: "none",
	transformOrigin: "0px 0px",
	translate: "none",
};

type SignalMatrix = readonly [
	number,
	number,
	number,
	number,
	number,
	number,
];

type SignalRoute = {
	blobMatrix: SignalMatrix;
	d: string;
	duration: number;
	emphasize?: boolean;
	fullDash: string;
	initialProgress?: number;
	signalDash: string;
	signalOffset: number;
};

const HERO_SIGNAL_ROUTES: SignalRoute[] = [
	{
		blobMatrix: [-1, 0, 0, -1, 731.5028, 226.01374],
		d: "M 768,0 L 768,131.5 A 50,50 0 0 1 718,181.5 L 530,181.5 L 242,181.5 A 50,50 0 0 0 192,231.5 L 192,590.5 A 28.34,28.34 0 0 1 142.4,609.24 L 0,447.7",
		duration: 9.2,
		fullDash: "1407.51px, 0.1px",
		signalDash: "14.0751px, 1393.53px",
		signalOffset: -271.284,
	},
	{
		blobMatrix: [1, 0, 0, 1, 1754.28264, 500],
		d: "M 1152,0 L 1152,228.3 A 50,50 0 0 0 1202,278.3 L 1678,278.3 A 50,50 0 0 1 1728,328.3 L 1728,494.5 A 50,50 0 0 0 1778,544.5 L 1920,544.5",
		duration: 8.4,
		emphasize: true,
		fullDash: "1248.15px, 0.1px",
		signalDash: "12.4815px, 1235.77px",
		signalOffset: -1148.32,
	},
	{
		blobMatrix: [-0.72671, -0.68695, 0.68695, -0.72671, 1783.85851, 900.54266],
		d: "M 1920,968 L 1742.53,800.24 A 50.27,50.27 0 0 0 1708,786.5 L 1298,786.5 A 50,50 0 0 0 1248,836.5 L 1248,1210",
		duration: 8.9,
		fullDash: "1144.33px, 0.1px",
		signalDash: "11.4433px, 1132.99px",
		signalOffset: -220.559,
	},
	{
		blobMatrix: [-0.67171, 0.74081, -0.74081, -0.67171, 325.94488, 1128.52442],
		d: "M 0,786.5 L 456,786.5 A 53.18,53.18 0 0 1 495.39,875.4 L 192,1210",
		duration: 7.8,
		fullDash: "1030.36px, 0.1px",
		signalDash: "10.3036px, 1020.16px",
		signalOffset: -947.953,
	},
];

// These paths begin exactly where the hero routes leave its canvas: the two
// side exits continue from the side, and the two bottom exits continue from
// the top of the next section. Their geometry intentionally changes so the
// problem section reads as the next chapter rather than a repeated backdrop.
type ProblemRouteTargets = {
	left: number;
	middle: number;
	right: number;
};

const DEFAULT_PROBLEM_ROUTE_TARGETS: ProblemRouteTargets = {
	left: 703,
	middle: 914,
	right: 1171,
};

function createProblemSignalRoutes({
	left,
	middle,
	right,
}: ProblemRouteTargets): SignalRoute[] {
	return [
		{
			blobMatrix: [1, 0, 0, 1, 0, 0],
			d: `M 0,447.7 L 132,579.7 A 50,50 0 0 0 167.5,594.4 L ${left - 50},594.4 A 50,50 0 0 1 ${left},644.4 L ${left},1210`,
			duration: 10.4,
			fullDash: "1398px, 0.1px",
			initialProgress: 0,
			signalDash: "13.98px, 1384.02px",
			signalOffset: 0,
		},
		{
			blobMatrix: [-1, 0, 0, -1, 0, 0],
			d: `M 1920,544.5 L 1764,700.5 A 50,50 0 0 1 1728.6,715.1 L ${right + 50},715.1 A 50,50 0 0 0 ${right},765.1 L ${right},1210`,
			duration: 9.7,
			fullDash: "1322px, 0.1px",
			initialProgress: 0,
			signalDash: "13.22px, 1308.78px",
			signalOffset: 0,
		},
		{
			blobMatrix: [0.72671, 0.68695, -0.68695, 0.72671, 0, 0],
			d: `M 1248,0 L 1248,168 A 50,50 0 0 1 1198,218 L ${middle + 50},218 A 50,50 0 0 0 ${middle},268 L ${middle},1210`,
			duration: 10.1,
			fullDash: "1270px, 0.1px",
			initialProgress: 0,
			signalDash: "12.7px, 1257.3px",
			signalOffset: 0,
		},
	];
}

function signalCenterFromMatrix(matrix: SignalMatrix) {
	const [a, b, c, d, e, f] = matrix;

	return {
		x: a * SIGNAL_BLOB_ORIGIN.x + c * SIGNAL_BLOB_ORIGIN.y + e,
		y: b * SIGNAL_BLOB_ORIGIN.x + d * SIGNAL_BLOB_ORIGIN.y + f,
	};
}

function signalMatrixAtPoint(matrix: SignalMatrix, x: number, y: number) {
	const [a, b, c, d] = matrix;
	const e = x - (a * SIGNAL_BLOB_ORIGIN.x + c * SIGNAL_BLOB_ORIGIN.y);
	const f = y - (b * SIGNAL_BLOB_ORIGIN.x + d * SIGNAL_BLOB_ORIGIN.y);

	return `matrix(${a},${b},${c},${d},${e.toFixed(5)},${f.toFixed(5)})`;
}

function closestPathProgress(
	path: SVGPathElement,
	target: { x: number; y: number },
) {
	const length = path.getTotalLength();
	const samples = 260;
	let closestDistance = Number.POSITIVE_INFINITY;
	let closestProgress = 0;

	for (let index = 0; index <= samples; index += 1) {
		const progress = index / samples;
		const point = path.getPointAtLength(progress * length);
		const distance = Math.hypot(point.x - target.x, point.y - target.y);

		if (distance < closestDistance) {
			closestDistance = distance;
			closestProgress = progress;
		}
	}

	return closestProgress;
}

function setSvgRef<T extends SVGElement>(
	refs: MutableRefObject<T[]>,
	index: number,
	node: T | null,
) {
	if (node) {
		refs.current[index] = node;
	}
}

/**
 * SignalPathLayer — Terminal-style ambient SVG route pattern.
 *
 * Geometry, stroke styling, dash arrays, and starting transforms mirror the
 * supplied Terminal footer SVG. GSAP only advances each dash offset and matching
 * blurred signal shape, so the static SVG remains valid at first paint.
 */
type SignalPathLayerProps = {
	variant: "hero" | "problem";
};

export function SignalPathLayer({ variant }: SignalPathLayerProps) {
	const baseStrokeOpacity = variant === "hero" ? 0.13 : 0.1;
	const baseStrokeWidth = variant === "hero" ? 0.78 : 1;
	const signalStrokeOpacity = variant === "hero" ? 1 : 0.26;
	const signalStrokeWidth = variant === "hero" ? 1.08 : 1;
	const emphasisOpacityBoost = variant === "hero" ? 0.08 : 0.16;
	const emphasisWidthBoost = variant === "hero" ? 0.14 : 0.3;
	const rootRef = useRef<HTMLDivElement>(null);
	const pathRefs = useRef<SVGPathElement[]>([]);
	const signalRefs = useRef<SVGPathElement[]>([]);
	const blobRefs = useRef<SVGPathElement[]>([]);
	const filterId = `verevon-signal-blur-${useId().replace(/:/g, "")}`;
	const [problemRouteTargets, setProblemRouteTargets] = useState(
		DEFAULT_PROBLEM_ROUTE_TARGETS,
	);
	const routes = useMemo(
		() =>
			variant === "hero"
				? HERO_SIGNAL_ROUTES
				: createProblemSignalRoutes(problemRouteTargets),
		[problemRouteTargets, variant],
	);

	useLayoutEffect(() => {
		if (variant !== "problem") {
			return;
		}

		if (!window.matchMedia("(min-width: 761px)").matches) {
			return;
		}

		const root = rootRef.current;
		const section = root?.closest<HTMLElement>("#problemet");
		const svg = root?.querySelector<SVGSVGElement>("svg");
		const tabs = section
			? Array.from(
					section.querySelectorAll<HTMLElement>("[data-problem-tab]"),
				)
			: [];

		if (!root || !section || !svg || tabs.length !== 3) {
			return;
		}

		let frame: number | null = null;

		const updateTargets = () => {
			const svgRect = svg.getBoundingClientRect();
			const scale = Math.max(svgRect.width / 1920, svgRect.height / 1210);

			if (scale === 0) {
				return;
			}
			const offsetX = svgRect.left + (svgRect.width - 1920 * scale) / 2;

			const [leftTab, middleTab, rightTab] = tabs;
			const toSvgX = (element: HTMLElement) => {
				const rect = element.getBoundingClientRect();
				return (rect.left + rect.width / 2 - offsetX) / scale;
			};
			const nextTargets = {
				left: toSvgX(leftTab),
				middle: toSvgX(middleTab),
				right: toSvgX(rightTab),
			};

			setProblemRouteTargets((current) => {
				const changed = Object.entries(nextTargets).some(
					([key, value]) =>
						Math.abs(current[key as keyof ProblemRouteTargets] - value) > 0.5,
				);

				return changed ? nextTargets : current;
			});
		};

		const scheduleUpdate = () => {
			if (frame !== null) {
				return;
			}

			frame = window.requestAnimationFrame(() => {
				frame = null;
				updateTargets();
			});
		};

		updateTargets();
		const observer = new ResizeObserver(scheduleUpdate);
		observer.observe(section);
		tabs.forEach((tab) => observer.observe(tab));
		document.fonts?.ready.then(scheduleUpdate);

		return () => {
			if (frame !== null) {
				window.cancelAnimationFrame(frame);
			}

			observer.disconnect();
		};
	}, [variant]);

	useEffect(() => {
		const root = rootRef.current;

		if (
			!root ||
			!window.matchMedia(
				"(min-width: 761px) and (prefers-reduced-motion: no-preference)",
			).matches
		) {
			return;
		}

		let disposed = false;
		let isVisible = false;
		let setupStarted = false;
		let cleanupContext: { revert: () => void } | undefined;
		let startSignals: (() => void) | undefined;
		let pauseSignals: (() => void) | undefined;
		const problemRevealHasStarted = () => {
			const activeRoot = rootRef.current;

			if (!activeRoot) {
				return false;
			}

			const rect = activeRoot.getBoundingClientRect();
			return rect.top <= window.innerHeight * 0.78 && rect.bottom >= 0;
		};

		async function setup() {
			const root = rootRef.current;
			const ready = routes.every(
				(_, index) =>
					pathRefs.current[index] &&
					signalRefs.current[index] &&
					blobRefs.current[index],
			);

			if (!root || !ready) {
				return;
			}

			const [{ default: gsap }, { ScrollTrigger }] = await Promise.all([
				import("gsap"),
				import("gsap/ScrollTrigger"),
			]);

			if (disposed || !isVisible) {
				setupStarted = false;
				return;
			}

			gsap.registerPlugin(ScrollTrigger);

			cleanupContext = gsap.context(() => {
				const signalTweens: Array<{
					pause: () => void;
					play: () => void;
				}> = [];
				const waitForReveal = variant === "problem";

				for (const [index, route] of routes.entries()) {
					const path = pathRefs.current[index];
					const signal = signalRefs.current[index];
					const blob = blobRefs.current[index];
					const length = path.getTotalLength();

					gsap.set(signal, {
						strokeDasharray: route.signalDash,
						strokeDashoffset: route.signalOffset,
					});
					const initialProgress =
						route.initialProgress ??
						closestPathProgress(
							path,
							signalCenterFromMatrix(route.blobMatrix),
						);
					const initialPoint = path.getPointAtLength(initialProgress * length);
					gsap.set(blob, {
						attr: {
							transform: signalMatrixAtPoint(
								route.blobMatrix,
								initialPoint.x,
								initialPoint.y,
							),
						},
					});

					const travel = { progress: initialProgress };

					const updateBlob = () => {
						const progress = travel.progress % 1;
						const point = path.getPointAtLength(progress * length);
						gsap.set(blob, {
							attr: {
								transform: signalMatrixAtPoint(
									route.blobMatrix,
									point.x,
									point.y,
								),
							},
						});
					};

					updateBlob();

					signalTweens.push(gsap.to(signal, {
						strokeDashoffset: route.signalOffset - length,
						duration: route.duration,
						ease: "none",
						paused: waitForReveal,
						repeat: -1,
					}));

					signalTweens.push(gsap.to(travel, {
						progress: travel.progress + 1,
						duration: route.duration,
						ease: "none",
						onUpdate: updateBlob,
						paused: waitForReveal,
						repeat: -1,
					}));
				}

				startSignals = () => {
					for (const tween of signalTweens) {
						tween.play();
					}
				};
				pauseSignals = () => {
					for (const tween of signalTweens) {
						tween.pause();
					}
				};

				if (variant === "hero") {
					if (isVisible) {
						startSignals();
					}
					return;
				}

				gsap.fromTo(
					root,
					{ autoAlpha: 0, yPercent: 0 },
					{
						autoAlpha: 1,
						yPercent: 0,
						ease: "none",
						scrollTrigger: {
							trigger: root,
							start: "top 78%",
							end: "top 34%",
							scrub: true,
							id: "verevon-problem-signal-routes",
							onEnter: () => startSignals?.(),
							onEnterBack: () => startSignals?.(),
						},
					},
				);

				if (isVisible && problemRevealHasStarted()) {
					startSignals();
				}
			});
		}

		const visibilityObserver = new IntersectionObserver(
			([entry]) => {
				isVisible = entry.isIntersecting;

				if (!isVisible) {
					pauseSignals?.();
					return;
				}

				if (!setupStarted) {
					setupStarted = true;
					void setup();
					return;
				}

				if (variant === "hero" || problemRevealHasStarted()) {
					startSignals?.();
				}
			},
			{ rootMargin: "300px 0px" },
		);

		visibilityObserver.observe(root);

		return () => {
			disposed = true;
			visibilityObserver.disconnect();
			cleanupContext?.revert();
		};
	}, [routes, variant]);

	return (
		<div
			aria-hidden="true"
			className={[
				"pointer-events-none absolute max-[760px]:hidden",
				variant === "hero"
					? "inset-0 z-[1]"
					: "inset-0 z-0",
				variant === "problem"
					? "opacity-0"
					: "opacity-100",
			].join(" ")}
			ref={rootRef}
			style={
				variant === "problem"
					? {
						maskImage:
							"linear-gradient(to bottom, black 0%, black 78%, transparent 100%)",
						WebkitMaskImage:
							"linear-gradient(to bottom, black 0%, black 78%, transparent 100%)",
					}
					: undefined
			}
		>
			<svg
				className="svg size-full"
				height="1210"
				preserveAspectRatio="xMidYMid slice"
				viewBox="0 0 1920 1210"
				width="1920"
			>
				<defs>
					<filter
						colorInterpolationFilters="sRGB"
						filterUnits="userSpaceOnUse"
						height="89"
						id={filterId}
						width="155"
						x="0"
						y="0"
					>
						<feGaussianBlur stdDeviation="17" />
					</filter>
				</defs>

				{routes.map((route, index) => (
					<g key={route.d}>
						<path
							d={SIGNAL_BLOB_D}
							data-svg-origin="77.49985885620117 44.5"
							fill="#A2A2A2"
							fillOpacity={0.025}
							filter={`url(#${filterId})`}
							ref={(node) => setSvgRef(blobRefs, index, node)}
							style={SIGNAL_BLOB_STYLE}
							transform={`matrix(${route.blobMatrix.join(",")})`}
						/>
						<path
							d={route.d}
							fill="none"
							ref={(node) => setSvgRef(pathRefs, index, node)}
							stroke={
								variant === "hero" ? "#D5D5D5" : "var(--verevon-j-text)"
							}
							strokeOpacity={
								route.emphasize
									? baseStrokeOpacity + emphasisOpacityBoost
									: baseStrokeOpacity
							}
							strokeWidth={
								route.emphasize
									? baseStrokeWidth + emphasisWidthBoost
									: baseStrokeWidth
							}
							vectorEffect={
								variant === "problem" ? "non-scaling-stroke" : undefined
							}
							style={{
								strokeDasharray: route.fullDash,
								strokeDashoffset: 0,
							}}
						/>
						<path
							d={route.d}
							fill="none"
							ref={(node) => setSvgRef(signalRefs, index, node)}
							stroke={
								variant === "hero" ? "#F0F0F0" : "var(--verevon-j-text)"
							}
							strokeLinecap="round"
							strokeOpacity={signalStrokeOpacity}
							strokeWidth={
								route.emphasize
									? signalStrokeWidth + emphasisWidthBoost
									: signalStrokeWidth
							}
							vectorEffect={
								variant === "problem" ? "non-scaling-stroke" : undefined
							}
							style={{
								opacity: 1,
								strokeDasharray: route.signalDash,
								strokeDashoffset: route.signalOffset,
							}}
						/>
					</g>
				))}
			</svg>
		</div>
	);
}

export default SignalPathLayer;
