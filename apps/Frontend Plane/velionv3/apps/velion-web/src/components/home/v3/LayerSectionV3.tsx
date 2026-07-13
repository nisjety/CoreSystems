"use client";

import Image from "next/image";
import { useEffect, useRef, useState } from "react";
import { motion, useInView, useReducedMotion } from "framer-motion";

type LayerId =
	| "management"
	| "business-context"
	| "orchestration"
	| "infrastructure";

type LayerDefinition = {
	description: string;
	framerLayer: 1 | 2 | 3 | 4;
	id: LayerId;
	image: string;
	title: string;
};

const layers: LayerDefinition[] = [
	{
		id: "management",
		title: "Oversikt",
		description:
			"Se hva som venter på godkjenning, hva som er avgjort, og hvor arbeidet stopper opp.",
		framerLayer: 1,
		image: "/velion-layers/management-layer.png",
	},
	{
		id: "orchestration",
		title: "Arbeidsflyt",
		description:
			"Velion flytter saken fra chat og innboks til utkast, godkjenning og neste steg.",
		framerLayer: 2,
		image: "/velion-layers/orchestration-layer.png",
	},
	{
		id: "business-context",
		title: "Kontekst",
		description:
			"Kilder, regler og tidligere svar følger saken, synlige og til å ettergå.",
		framerLayer: 3,
		image: "/velion-layers/business-context-layer.png",
	},
	{
		id: "infrastructure",
		title: "Infrastruktur",
		description:
			"Modeller, koblinger, godkjenning og revisjonsspor kjøres med EU som standard, ikke et tillegg.",
		framerLayer: 4,
		image: "/velion-layers/infrastructure-layer.png",
	},
];

type StackFrame = {
	glowOpacity: number;
	scale: number;
	shadowOpacity: number;
	y: number;
};

/**
 * Physical z-order must stay constant.
 * This prevents lower visual layers from drawing above upper layers.
 *
 * Layer 1 = Management / Styringslag
 * Layer 2 = Orchestration / Orkestrering
 * Layer 3 = Business Context / Bedriftskontekst
 * Layer 4 = Infrastructure / Infrastruktur
 */
const physicalLayerZIndex: Record<LayerId, number> = {
	management: 40,
	orchestration: 30,
	"business-context": 20,
	infrastructure: 10,
};

const layerGlows: Record<LayerId, string> = {
	management:
		"radial-gradient(circle at 52% 36%, rgba(250, 165, 121, 0.22), transparent 56%)",
	orchestration:
		"radial-gradient(circle at 50% 50%, rgba(121, 137, 210, 0.15), transparent 60%)",
	"business-context":
		"radial-gradient(circle at 48% 46%, rgba(250, 165, 121, 0.16), transparent 60%)",
	infrastructure:
		"radial-gradient(circle at 50% 60%, rgba(57, 57, 57, 0.08), transparent 62%)",
};

/**
 * Visual layer order:
 *
 * Layer 1 = Management / Styringslag
 * Top operational surface with monitoring, evaluation and optimization.
 *
 * Layer 2 = Orchestration / Orkestrering
 * Agent routing and workflow coordination across channels and interfaces.
 *
 * Layer 3 = Business Context / Bedriftskontekst
 * Reusable instructions, knowledge, skills and process memory.
 *
 * Layer 4 = Infrastructure / Infrastruktur
 * Secure execution, models, connectors, approvals and audit trails.
 *
 * Important:
 * These layouts only control position, scale, glow and shadow.
 * The physical z-index stays fixed elsewhere so lower layers never render above upper layers.
 */
const stackLayouts: Record<LayerId, Record<LayerId, StackFrame>> = {
	management: {
		management: {
			y: -220,
			scale: 1.06,
			glowOpacity: 1,
			shadowOpacity: 0.18,
		},
		orchestration: {
			y: 68,
			scale: 0.9,
			glowOpacity: 0.2,
			shadowOpacity: 0.08,
		},
		"business-context": {
			y: 112,
			scale: 0.865,
			glowOpacity: 0.06,
			shadowOpacity: 0.055,
		},
		infrastructure: {
			y: 152,
			scale: 0.845,
			glowOpacity: 0.05,
			shadowOpacity: 0.06,
		},
	},

	orchestration: {
		management: {
			y: -300,
			scale: 0.9,
			glowOpacity: 0.22,
			shadowOpacity: 0.095,
		},
		orchestration: {
			y: 44,
			scale: 1.06,
			glowOpacity: 0.96,
			shadowOpacity: 0.17,
		},
		"business-context": {
			y: 330,
			scale: 0.86,
			glowOpacity: 0.06,
			shadowOpacity: 0.06,
		},
		infrastructure: {
			y: 378,
			scale: 0.84,
			glowOpacity: 0.045,
			shadowOpacity: 0.055,
		},
	},

	"business-context": {
		management: {
			y: -300,
			scale: 0.9,
			glowOpacity: 0.22,
			shadowOpacity: 0.095,
		},
		orchestration: {
			y: -250,
			scale: 0.86,
			glowOpacity: 0.045,
			shadowOpacity: 0.055,
		},
		"business-context": {
			y: 92,
			scale: 1.06,
			glowOpacity: 0.94,
			shadowOpacity: 0.17,
		},
		infrastructure: {
			y: 350,
			scale: 0.86,
			glowOpacity: 0.06,
			shadowOpacity: 0.06,
		},
	},

	infrastructure: {
		management: {
			y: -218,
			scale: 0.92,
			glowOpacity: 0.24,
			shadowOpacity: 0.1,
		},
		orchestration: {
			y: -168,
			scale: 0.875,
			glowOpacity: 0.045,
			shadowOpacity: 0.055,
		},
		"business-context": {
			y: -122,
			scale: 0.855,
			glowOpacity: 0.05,
			shadowOpacity: 0.055,
		},
		infrastructure: {
			y: 210,
			scale: 1.08,
			glowOpacity: 0.68,
			shadowOpacity: 0.17,
		},
	},
};

const springTransition = {
	type: "spring",
	mass: 0.95,
	stiffness: 82,
	damping: 21,
} as const;

const entryTransition = {
	type: "spring",
	mass: 1,
	stiffness: 74,
	damping: 18,
} as const;

/**
 * Scales all vertical layer movement so the full section fits in one viewport.
 * The stackLayouts keep their design values, while this function compresses
 * the movement based on screen width.
 */
function useLayerOffsetScale() {
	const [scale, setScale] = useState(0.74);

	useEffect(() => {
		const update = () => {
			const width = window.innerWidth;

			if (width < 640) {
				setScale(0.5);
				return;
			}

			if (width < 900) {
				setScale(0.62);
				return;
			}

			if (width < 1280) {
				setScale(0.68);
				return;
			}

			setScale(0.74);
		};

		update();

		window.addEventListener("resize", update);
		return () => window.removeEventListener("resize", update);
	}, []);

	return scale;
}

function LayerCallout({
	active,
	layer,
	onSelect,
}: {
	active: boolean;
	layer: LayerDefinition;
	onSelect: (id: LayerId) => void;
}) {
	return (
		<button
			aria-pressed={active}
			className="w-full cursor-pointer bg-transparent p-0 text-left outline-none focus:outline-none focus-visible:rounded-[10px] focus-visible:ring-2 focus-visible:ring-velion-coral/30"
			onClick={() => onSelect(layer.id)}
			type="button"
		>
			<motion.span
				aria-hidden="true"
				animate={{
					opacity: active ? 1 : 0,
					width: active ? 34 : 22,
				}}
				className="mb-4 block h-[3px] bg-velion-coral"
				initial={false}
				transition={{ duration: 0.28, ease: [0.22, 1, 0.36, 1] }}
			/>

			<motion.h3
				animate={{ opacity: active ? 1 : 0.4 }}
				className="font-arbeit text-[clamp(1.12rem,0.96vw,1.35rem)] font-light leading-[1.08] tracking-[-0.05em] text-velion-j-text"
				initial={false}
				transition={{ duration: 0.28, ease: [0.22, 1, 0.36, 1] }}
			>
				{layer.title}
			</motion.h3>

			<motion.p
				animate={{ opacity: active ? 0.78 : 0.4 }}
				className="mt-3 max-w-[310px] font-protokoll text-[clamp(0.9rem,0.78vw,1rem)] font-light leading-[1.42] tracking-[-0.02em] text-velion-text-muted"
				initial={false}
				transition={{ duration: 0.28, ease: [0.22, 1, 0.36, 1] }}
			>
				{layer.description}
			</motion.p>
		</button>
	);
}

export function LayerSectionV3() {
	const sectionRef = useRef<HTMLElement | null>(null);
	const sectionInView = useInView(sectionRef, {
		amount: 0.34,
		once: true,
	});

	const [activeLayerId, setActiveLayerId] = useState<LayerId>("management");
	const prefersReducedMotion = useReducedMotion();
	const offsetScale = useLayerOffsetScale();

	const activeLayout = stackLayouts[activeLayerId];

	return (
		<section
			ref={sectionRef}
			className="relative isolate overflow-hidden bg-background py-24 text-velion-j-text lg:py-32"
			id="plattform"
		>
			<div
				aria-hidden="true"
				className="absolute inset-0 bg-[radial-gradient(circle_at_50%_16%,rgba(23,23,23,0.04),transparent_34%),linear-gradient(90deg,rgba(23,23,23,0.022)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.02)_1px,transparent_1px)] bg-[length:100%_100%,108px_108px,108px_108px]"
			/>

			<div
				aria-hidden="true"
				className="absolute inset-0 bg-[radial-gradient(circle_at_50%_54%,rgba(255,255,255,0.94),rgba(248,248,247,0.54)_35%,transparent_72%)]"
			/>

			<div className="relative z-[1] mx-auto w-full max-w-[1540px] px-[clamp(24px,4.2vw,112px)]">
				<header className="grid grid-cols-1 gap-x-8 gap-y-6 md:grid-cols-3">
					<p className="pt-1 font-protokoll text-[12px] font-light leading-none text-velion-text-muted">
						Slik henger det sammen
					</p>

					<div className="md:col-span-2">
						<h2 className="font-arbeit text-[clamp(2.25rem,2.9vw,3.8rem)] font-light leading-[0.96] tracking-[-0.068em] text-velion-j-text">
							Én flate for kunnskap, agentarbeid og kontroll.
						</h2>

						<p className="mt-4 max-w-[580px] font-protokoll text-[clamp(1rem,1vw,1.16rem)] font-light leading-[1.42] text-velion-text-muted">
							Samme sløyfe, samme kilder, samme spor: hver kobling gjør
							saksflyten mer nyttig, uten å bryte det som allerede er avgjort.
						</p>
					</div>
				</header>

				<div className="mb-16 md:mb-24" />

				<div
					className="grid grid-cols-1 items-start gap-y-10 md:grid-cols-[5fr_2fr]"
					style={{ columnGap: "30%" }}
				>
					<div
						className="relative w-full overflow-hidden border border-velion-j-text/[0.08] bg-background p-[clamp(24px,3vw,52px)] shadow-[0_24px_90px_rgba(31,31,29,0.08)]"
						style={{ aspectRatio: "3/4" }}
					>
						<div
							aria-hidden="true"
							className="absolute inset-0 bg-[radial-gradient(circle_at_50%_20%,rgba(255,255,255,0.92),rgba(248,248,247,0.62)_36%,transparent_74%),linear-gradient(90deg,rgba(23,23,23,0.026)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.022)_1px,transparent_1px)] bg-[length:100%_100%,92px_92px,92px_92px]"
						/>

						<div
							aria-hidden="true"
							className="absolute inset-x-[8%] top-[16%] h-[64%] rounded-full bg-[radial-gradient(circle_at_50%_50%,rgba(250,165,121,0.13),rgba(121,137,210,0.075)_38%,transparent_72%)] blur-[72px]"
						/>

						<span className="relative z-[2] block max-w-[280px] font-protokoll text-[10px] font-light uppercase leading-[1.4] tracking-[0.25em] text-velion-text-muted/70">
							Oversikt, flyt, kontekst, infrastruktur
						</span>

						<div className="absolute inset-x-[clamp(12px,3vw,44px)] bottom-[clamp(16px,3vw,48px)] top-[clamp(64px,8vw,112px)]">
							{layers.map((layer) => {
								const frame = activeLayout[layer.id];
								const isActive = layer.id === activeLayerId;
								const shouldPlayEntry =
									layer.id === "management" &&
									!sectionInView &&
									!prefersReducedMotion;

								return (
									<motion.div
										aria-hidden="true"
										animate={{
											opacity: shouldPlayEntry ? 0 : 1,
											scale: shouldPlayEntry
												? frame.scale * 0.72
												: frame.scale,
											y: shouldPlayEntry
												? frame.y * offsetScale + 44
												: frame.y * offsetScale,
										}}
										className="pointer-events-none absolute inset-0 flex items-center justify-center"
										data-framer-layer={layer.framerLayer}
										initial={false}
										key={layer.id}
										style={{
											filter: `drop-shadow(0 ${
												isActive ? 28 : 15
											}px ${isActive ? 42 : 22}px rgba(31, 31, 29, ${
												frame.shadowOpacity
											}))`,
											zIndex: physicalLayerZIndex[
												layer.id
											],
										}}
										transition={
											prefersReducedMotion
												? { duration: 0 }
												: layer.id === "management"
													? entryTransition
													: springTransition
										}
									>
										<motion.div
											animate={{
												opacity: shouldPlayEntry
													? 0
													: frame.glowOpacity,
											}}
											className="absolute inset-[12%_10%_8%]"
											initial={false}
											style={{
												backgroundImage:
													layerGlows[layer.id],
											}}
											transition={
												prefersReducedMotion
													? { duration: 0 }
													: {
															duration: 0.55,
															ease: [
																0.22, 1, 0.36,
																1,
															],
														}
											}
										/>

										<div className="relative aspect-[1832/939] w-[min(88vw,680px)] md:w-[min(50vw,680px)]">
											<Image
												alt=""
												className="select-none object-contain"
												draggable={false}
												fill
												priority={
													layer.id === "management"
												}
												sizes="(max-width: 720px) 88vw, (max-width: 1280px) 50vw, 680px"
												src={layer.image}
											/>
										</div>
									</motion.div>
								);
							})}
						</div>
					</div>

					<div className="grid content-start gap-[clamp(30px,4vw,52px)] pt-1 sm:grid-cols-2 md:flex md:flex-col">
						{layers.map((layer) => (
							<LayerCallout
								active={layer.id === activeLayerId}
								key={layer.id}
								layer={layer}
								onSelect={setActiveLayerId}
							/>
						))}
					</div>
				</div>
			</div>
		</section>
	);
}

export default LayerSectionV3;
