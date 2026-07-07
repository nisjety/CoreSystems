"use client";

import type { CSSProperties } from "react";
import Link from "next/link";
import { useLayoutEffect, useRef } from "react";
import {
	ArrowUpRight,
	BarChart3,
	CalendarDays,
	CheckCircle2,
	CircleDot,
	FileText,
	KanbanSquare,
	LayoutDashboard,
	MessageSquareText,
	Settings,
	ShieldCheck,
	Sparkles,
	UserRound,
	Workflow,
} from "lucide-react";
import { gsap } from "gsap";
import styles from "./VelionShortShowcase.module.css";

const navigationItems = ["Product", "Workflows", "Learn"];

const sidebarNavItems = [
	{ icon: LayoutDashboard, isActive: true, label: "Dashboard" },
	{ icon: KanbanSquare, label: "Boards" },
	{ icon: CalendarDays, label: "Calendar" },
	{ icon: BarChart3, label: "Analytics" },
	{ icon: Settings, label: "Settings" },
];

const boardProfile = { name: "Kaja Lund", role: "Workspace lead" };

const taskCardTilt = [-3, 2.4, -3.6, 3, -2.2, 3.4];

const ribbonPalette = [
	"#b81f68",
	"#ee5fb0",
	"#5caec0",
	"#61254b",
	"#d24d6a",
	"#25465a",
	"#f3a0d8",
	"#277c93",
];

const ribbonSegments = Array.from({ length: 58 }, (_, index) => {
	const center = 28.5;
	const distance = Math.abs(index - center);
	const wave = Math.sin(index * 0.43) * 5.5;
	const ridge = Math.max(0, 18 - distance * 1.28);

	return {
		id: `ribbon-${index}`,
		background: ribbonPalette[index % ribbonPalette.length],
		x: -1 + index * 1.82,
		y: 60 + wave - ridge,
		rotate: -13 + (index % 9) * 3.35,
		scale: 0.82 + Math.max(0, 0.24 - distance * 0.006),
		z: index % 3 === 0 ? 42 : 14,
	};
});

const sourceTiles = [
	{ color: "blue", label: "ticket" },
	{ color: "coral", label: "mail" },
	{ color: "plum", label: "doc" },
];

const taskCards = [
	{
		accent: "blue",
		icon: MessageSquareText,
		owner: "Support",
		title: "Unanswered chat",
		meta: "4 sources",
	},
	{
		accent: "coral",
		icon: FileText,
		owner: "Sales",
		title: "Draft reply",
		meta: "Needs approval",
	},
	{
		accent: "plum",
		icon: ShieldCheck,
		owner: "Ops",
		title: "Policy check",
		meta: "Low risk",
	},
	{
		accent: "earth",
		icon: Workflow,
		owner: "Success",
		title: "Route workflow",
		meta: "2 actions",
	},
	{
		accent: "blue",
		icon: UserRound,
		owner: "Human",
		title: "Approve send",
		meta: "Waiting",
	},
	{
		accent: "coral",
		icon: CheckCircle2,
		owner: "Velion",
		title: "Audit trail",
		meta: "Saved",
	},
];

function useShowcaseTimeline(
	rootRef: React.RefObject<HTMLElement | null>,
	variant: "standalone" | "embedded" | "viewport",
) {
	useLayoutEffect(() => {
		const root = rootRef.current;
		const isDashboardOnly = variant === "viewport";

		if (!root) {
			return;
		}

		if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
			root.dataset.reducedMotion = "true";
			return;
		}

		const context = gsap.context(() => {
			const q = gsap.utils.selector(root);
			const overlay = q("[data-short-overlay]");
			const browser = q("[data-browser]");
			const topCaption = q("[data-top-caption]");
			const bottomCaption = q("[data-bottom-caption]");
			const heroScene = q("[data-hero-scene]");
			const heroCopy = q("[data-hero-copy]");
			const ribbon = q("[data-ribbon]");
			const ribbonSegments = q("[data-ribbon-segment]");
			const boardScene = q("[data-board-scene]");
			const camera = q("[data-camera]");
			const board = q("[data-board]");
			const measureLines = q("[data-measure-line]");
			const boardHeadings = q("[data-board-heading]");
			const sourceTiles = q("[data-source-tile]");
			const taskCards = q("[data-task-card]");
			const finalCopy = q("[data-final-copy]");
			const lightBands = q("[data-light-band]");

			gsap.set([topCaption, bottomCaption, browser], {
				autoAlpha: 0,
				force3D: true,
			});
			gsap.set(boardScene, { autoAlpha: 0 });
			gsap.set(finalCopy, { autoAlpha: 0, x: -28 });
			gsap.set(ribbonSegments, {
				autoAlpha: 0,
				rotateX: -64,
				scale: 0.72,
				y: 46,
				z: -40,
			});
			gsap.set(sourceTiles, {
				autoAlpha: 0,
				rotateX: -58,
				rotateZ: -7,
				scale: 0.86,
				x: -24,
				y: -150,
				z: 130,
			});
			gsap.set(taskCards, {
				autoAlpha: 0,
				rotateX: -72,
				rotateZ: 9,
				scale: 0.76,
				x: (index) => [-64, -28, 18, -42, 24, 52][index] ?? 0,
				y: -220,
				z: 160,
			});
			gsap.set(measureLines, { autoAlpha: 0, scaleX: 0.42 });
			gsap.set(boardHeadings, { autoAlpha: 0, y: 8 });
			gsap.set(camera, {
				force3D: true,
				rotateX: 67,
				rotateZ: -19,
				scale: 0.72,
				x: -32,
				y: 34,
			});

			if (isDashboardOnly) {
				gsap.set(overlay, { opacity: 0 });
				gsap.set([topCaption, bottomCaption, heroScene, finalCopy], {
					autoAlpha: 0,
				});
				gsap.set(browser, {
					autoAlpha: 1,
					filter: "none",
					scale: 1,
					y: 0,
				});
				gsap.set(boardScene, { autoAlpha: 1 });
				gsap.set(board, { autoAlpha: 1, filter: "blur(0px)" });
				gsap.set(lightBands, { autoAlpha: 0, x: 190 });
				gsap.set(camera, {
					force3D: true,
					rotateX: 66,
					rotateZ: -17,
					scale: 0.46,
					x: -12,
					y: 72,
				});

				const dashboardTimeline = gsap.timeline({
					defaults: { ease: "power3.out" },
					repeat: -1,
					repeatDelay: 0.55,
				});

				dashboardTimeline
					.to(
						camera,
						{
							duration: 0.95,
							rotateX: 62,
							rotateZ: -15,
							scale: 0.66,
							x: 4,
							y: 28,
						},
						0,
					)
					.to(
						measureLines,
						{
							autoAlpha: 1,
							duration: 0.5,
							scaleX: 1,
							stagger: 0.08,
						},
						0.18,
					)
					.to(
						boardHeadings,
						{
							autoAlpha: 1,
							duration: 0.5,
							stagger: 0.08,
							y: 0,
						},
						0.18,
					)
					.to(
						sourceTiles,
						{
							autoAlpha: 1,
							duration: 0.62,
							rotateX: 0,
							rotateZ: 0,
							scale: 1,
							stagger: 0.1,
							x: 0,
							y: 0,
							z: 0,
						},
						0.36,
					)
					.to(
						taskCards,
						{
							autoAlpha: 1,
							duration: 0.72,
							rotateX: 0,
							rotateZ: (index) => taskCardTilt[index] ?? 0,
							scale: 1,
							stagger: { amount: 0.58, from: "start" },
							x: 0,
							y: 0,
							z: 0,
						},
						0.62,
					)
					.to(
						camera,
						{
							duration: 1.55,
							ease: "sine.inOut",
							rotateX: 58,
							rotateZ: -12,
							scale: 1.12,
							x: 118,
							y: -38,
						},
						1.92,
					)
					.to(
						taskCards,
						{
							duration: 1.15,
							stagger: 0.04,
							x: (index) => (index % 2 === 0 ? -6 : 14),
							y: (index) => (index % 2 === 0 ? -14 : 10),
							z: (index) => 34 + (index % 3) * 12,
						},
						2.1,
					)
					.to(
						lightBands,
						{
							autoAlpha: 0.44,
							duration: 0.9,
							stagger: 0.08,
							x: -220,
						},
						2.25,
					)
					.to(
						camera,
						{
							duration: 1.1,
							ease: "sine.inOut",
							scale: 1.18,
							x: 132,
							y: -42,
						},
						3.35,
					)
					.to(
						[camera, taskCards],
						{
							duration: 1.15,
							ease: "sine.inOut",
							y: "+=4",
						},
						3.55,
					)
					.to(
						[measureLines, sourceTiles, taskCards, lightBands, boardHeadings],
						{
							autoAlpha: 0,
							duration: 0.45,
							stagger: { amount: 0.18, from: "end" },
						},
						5.1,
					);

				return;
			}

			const timeline = gsap.timeline({
				defaults: { ease: "power3.out" },
				repeat: -1,
				repeatDelay: 0.7,
			});

			timeline
				.fromTo(
					overlay,
					{ opacity: 0.78 },
					{ duration: 0.72, opacity: 0 },
					0,
				)
				.fromTo(
					topCaption,
					{ autoAlpha: 0, y: 14 },
					{ autoAlpha: 1, duration: 0.62, y: 0 },
					0.05,
				)
				.fromTo(
					bottomCaption,
					{ autoAlpha: 0, y: 18 },
					{ autoAlpha: 0.78, duration: 0.7, y: 0 },
					0.08,
				)
				.fromTo(
					browser,
					{ autoAlpha: 0, scale: 0.94, y: 26 },
					{ autoAlpha: 1, duration: 0.86, scale: 1, y: 0 },
					0.12,
				)
				.fromTo(
					heroCopy,
					{ autoAlpha: 0, y: 16 },
					{ autoAlpha: 1, duration: 0.66, y: 0 },
					0.28,
				)
				.to(
					ribbonSegments,
					{
						autoAlpha: 1,
						duration: 0.78,
						rotateX: 0,
						scale: 1,
						stagger: { amount: 0.58, from: "center" },
						y: 0,
						z: 0,
					},
					0.42,
				)
				.to(
					ribbon,
					{
						duration: 1.8,
						ease: "sine.inOut",
						scale: 1.045,
						x: -22,
					},
					0.9,
				)
				.to(
					lightBands,
					{
						autoAlpha: 0.42,
						duration: 0.9,
						stagger: 0.08,
						x: 80,
					},
					1.1,
				)
				.to(
					[heroCopy, heroScene],
					{
						autoAlpha: 0,
						duration: 0.45,
						y: -22,
					},
					2.35,
				)
				.to(
					ribbonSegments,
					{
						autoAlpha: 0,
						duration: 0.5,
						rotateX: 56,
						stagger: { amount: 0.26, from: "edges" },
						y: -34,
						z: 60,
					},
					2.35,
				)
				.to(boardScene, { autoAlpha: 1, duration: 0.25 }, 2.56)
				.fromTo(
					board,
					{ autoAlpha: 0, filter: "blur(8px)" },
					{ autoAlpha: 1, duration: 0.58, filter: "blur(0px)" },
					2.58,
				)
				.to(
					camera,
					{
						duration: 1.2,
						rotateX: 61,
						rotateZ: -13,
						scale: 0.86,
						x: 0,
						y: 6,
					},
					2.58,
				)
				.to(
					measureLines,
					{
						autoAlpha: 1,
						duration: 0.5,
						scaleX: 1,
						stagger: 0.08,
					},
					2.82,
				)
				.to(
					boardHeadings,
					{
						autoAlpha: 1,
						duration: 0.5,
						stagger: 0.08,
						y: 0,
					},
					2.82,
				)
				.to(
					sourceTiles,
					{
						autoAlpha: 1,
						duration: 0.62,
						rotateX: 0,
						stagger: 0.1,
						y: 0,
						z: 0,
					},
					3.03,
				)
				.to(
					taskCards,
					{
						autoAlpha: 1,
						duration: 0.72,
						rotateX: 0,
						rotateZ: (index) => taskCardTilt[index] ?? 0,
						scale: 1,
						stagger: { amount: 0.58, from: "start" },
						y: 0,
						z: 0,
					},
					3.34,
				)
				.to(
					camera,
					{
						duration: 1.45,
						ease: "sine.inOut",
						rotateX: 57,
						rotateZ: -13,
						scale: 1.24,
						x: 126,
						y: -34,
					},
					4.55,
				)
				.to(
					taskCards,
					{
						duration: 1.1,
						stagger: 0.04,
						y: (index) => (index % 2 === 0 ? -10 : 8),
						z: (index) => 24 + (index % 3) * 9,
					},
					4.72,
				)
				.to(
					finalCopy,
					{
						autoAlpha: 1,
						duration: 0.72,
						x: 0,
					},
					5.52,
				)
				.to(
					lightBands,
					{
						autoAlpha: 0.58,
						duration: 1,
						stagger: 0.08,
						x: -60,
					},
					5.56,
				)
				.to(
					[finalCopy, camera],
					{
						duration: 1.2,
						ease: "sine.inOut",
						y: "+=4",
					},
					6.52,
				)
				.to(browser, { duration: 0.55, filter: "brightness(0.42)" }, 8.35)
				.to([topCaption, bottomCaption], { autoAlpha: 0.2, duration: 0.55 }, 8.4)
				.to(overlay, { duration: 0.58, opacity: 0.82 }, 8.38);
		}, root);

		return () => context.revert();
	}, [rootRef, variant]);
}

type VelionShortShowcaseStageProps = {
	ariaLabel?: string;
	className?: string;
	variant?: "standalone" | "embedded" | "viewport";
};

export function VelionShortShowcaseStage({
	ariaLabel = "Velion vertical product animation for short-form recording",
	className = "",
	variant = "standalone",
}: VelionShortShowcaseStageProps) {
	const rootRef = useRef<HTMLElement>(null);

	useShowcaseTimeline(rootRef, variant);

	return (
		<section
			aria-label={ariaLabel}
			className={[
				styles.stage,
				variant === "embedded" ? styles.stageEmbedded : "",
				variant === "viewport" ? styles.stageViewport : "",
				className,
			]
				.filter(Boolean)
				.join(" ")}
			ref={rootRef}
		>
			<div aria-hidden="true" className={styles.backdropTexture} />
			<div aria-hidden="true" className={styles.shortOverlay} data-short-overlay="" />

			{variant === "standalone" ? (
				<h1 className={styles.topCaption} data-top-caption="">
					The Design
				</h1>
			) : (
				<p className={styles.topCaption} data-top-caption="">
					The Design
				</p>
			)}

			<div className={styles.browserFrame} data-browser="">
				<div className={styles.browserChrome}>
					<div className={styles.brandLockup}>
						<span className={styles.brandWord}>VELION</span>
						<span className={styles.brandTagline}>
							The control room for customer work
						</span>
					</div>

					<nav aria-label="Showcase navigation" className={styles.navigation}>
						{navigationItems.map((item) => (
							<span key={item}>{item}</span>
						))}
					</nav>

					<Link className={styles.chromeCta} href="/" tabIndex={-1}>
						[ Open flow ]
						<ArrowUpRight aria-hidden="true" size={12} strokeWidth={2.2} />
					</Link>
				</div>

				<div className={styles.browserViewport}>
					<div className={styles.lightBandOne} data-light-band="" />
					<div className={styles.lightBandTwo} data-light-band="" />

					<div className={styles.heroScene} data-hero-scene="">
						<div className={styles.heroCopy} data-hero-copy="">
							<p>work - flow</p>
							<h2>Your team&apos;s command center for customer work.</h2>
						</div>

						<p className={styles.heroRight}>
							Every signal turns into work your team can approve, trace, and
							ship from one calm surface.
						</p>

						<div className={styles.heroPrompt}>
							<p>Ready to turn every request into finished work?</p>
							<button tabIndex={-1} type="button">
								Open flow
							</button>
						</div>

						<div aria-hidden="true" className={styles.ribbon} data-ribbon="">
							{ribbonSegments.map((segment) => (
								<span
									className={styles.ribbonSegment}
									data-ribbon-segment=""
									key={segment.id}
									style={
										{
											"--segment-bg": segment.background,
											"--segment-rotate": `${segment.rotate}deg`,
											"--segment-scale": segment.scale,
											"--segment-x": `${segment.x}%`,
											"--segment-y": `${segment.y}%`,
											"--segment-z": `${segment.z}px`,
										} as CSSProperties
									}
								/>
							))}
						</div>
					</div>

					<div className={styles.boardScene} data-board-scene="">
						<div className={styles.camera} data-camera="">
							<div className={styles.board} data-board="">
								<div className={styles.boardGrid} />
								<div className={styles.measureLineTop} data-measure-line="" />
								<div className={styles.measureLineBottom} data-measure-line="" />
								<div className={styles.measureLineSide} data-measure-line="" />

								<p className={styles.boardHeading} data-board-heading="">
									Dashboard
								</p>

								<nav className={styles.boardSidebar}>
									{sidebarNavItems.map((item) => {
										const Icon = item.icon;

										return (
											<div
												className={[
													styles.boardSidebarItem,
													item.isActive ? styles.boardSidebarItemActive : "",
												]
													.filter(Boolean)
													.join(" ")}
												key={item.label}
											>
												<Icon aria-hidden="true" size={11} strokeWidth={2.4} />
												<span>{item.label}</span>
											</div>
										);
									})}
								</nav>

								<div className={styles.boardProfile}>
									<span aria-hidden="true" className={styles.boardProfileAvatar} />
									<span className={styles.boardProfileText}>
										<strong>{boardProfile.name}</strong>
										<em>{boardProfile.role}</em>
									</span>
								</div>

								<div className={styles.glassPlate}>
									<div className={styles.glassHighlight} />
									<div className={styles.statusRail}>
										<span />
										<span />
										<span />
										<span />
									</div>
								</div>

								<div className={styles.sourceTileRack}>
									{sourceTiles.map((tile) => (
										<div
											className={[
												styles.sourceTile,
												styles[`sourceTile${tile.color}`],
											].join(" ")}
											data-source-tile=""
											key={tile.label}
										>
											<CircleDot aria-hidden="true" size={12} strokeWidth={2.4} />
											<span>{tile.label}</span>
										</div>
									))}
								</div>

								<p className={styles.queueHeading} data-board-heading="">
									Work queue · 6 open
								</p>

								<div className={styles.taskGrid}>
									{taskCards.map((card, index) => {
										const Icon = card.icon;

										return (
											<article
												className={[
													styles.taskCard,
													styles[`taskCard${card.accent}`],
												].join(" ")}
												data-task-card=""
												key={`${card.title}-${index}`}
											>
												<header>
													<span className={styles.avatarStack}>
														<span />
														<span />
													</span>
													<Icon
														aria-hidden="true"
														className={styles.cardIcon}
														size={16}
														strokeWidth={2.2}
													/>
												</header>

												<strong>{card.title}</strong>
												<p>{card.owner}</p>
												<footer>
													<span>{card.meta}</span>
													<Sparkles aria-hidden="true" size={12} strokeWidth={2.3} />
												</footer>
											</article>
										);
									})}
								</div>
							</div>
						</div>
					</div>

					<div className={styles.finalCopy} data-final-copy="">
						<h2>Create drafts, set deadlines, assign owners.</h2>
						<p>
							Velion keeps signals, reviewers, and source evidence moving
							together.
						</p>
						<div className={styles.finalAction}>
							<span>Review every step</span>
							<button tabIndex={-1} type="button">
								Review flow
							</button>
						</div>
					</div>
				</div>
			</div>

			<p className={styles.bottomCaption} data-bottom-caption="">
				VELION FLOW
			</p>
		</section>
	);
}

export function VelionShortShowcase() {
	return (
		<main className={styles.page}>
			<VelionShortShowcaseStage />
		</main>
	);
}
