"use client";

import Image from "next/image";
import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import {
	Minus,
	Plus,
} from "lucide-react";
import { usePrefersReducedMotion } from "@/shared/hooks/usePrefersReducedMotion";

type LayerId =
	| "management"
	| "business-context"
	| "orchestration"
	| "infrastructure";

type LayerDefinition = {
	capabilities: readonly string[];
	description: string;
	story: string;
	framerLayer: 1 | 2 | 3 | 4;
	id: LayerId;
	image: string;
	/** Where this layer sits in the stack — spoken, so the copy reads the visual. */
	position: string;
	title: string;
};

// Each illustration is a complete, deliberately composed surface rather than
// a single thin plate. Selection gives the stack a clear focal point while
// keeping the remaining layers visibly connected beneath it.
const layers: LayerDefinition[] = [
	{
		id: "management",
		title: "Kontroll",
		position: "Øverste lag",
		description: "Regler, tilgang og godkjenning rundt arbeidet.",
		story: "Dere bestemmer hva AI kan se, foreslå og gjøre. Arbeid som faller utenfor rammene, eller krever en vurdering, går til riktig person. Alt kan følges og etterprøves.",
		capabilities: ["Policy", "Tilgang", "Godkjenning", "Revisjon"],
		framerLayer: 1,
		image: "/verevon-layers/kontroll-transparent.png",
	},
	{
		id: "orchestration",
		title: "Agentarbeid",
		position: "Andre lag",
		description: "AI-arbeid innenfor rammene dere setter.",
		story: "AI henter relevant kontekst, gjør forberedelser og utfører godkjente handlinger på tvers av systemene. Mennesker tar beslutningene der det trengs.",
		capabilities: ["E-post", "Teams", "Telefon", "Mikrofon", "Nett"],
		framerLayer: 2,
		image: "/verevon-layers/agentarbeid-transparent.png",
	},
	{
		id: "business-context",
		title: "Kunnskap",
		position: "Tredje lag",
		description: "Virksomhetens kunnskap, samlet i kontekst.",
		story: "Dokumenter, regler, tidligere beslutninger og arbeidsmåter knyttes til saken. Det gjør grunnlaget synlig for både mennesker og AI.",
		capabilities: ["MCP", "Skills", "Instruksjoner", "Integrasjoner"],
		framerLayer: 3,
		image: "/verevon-layers/kunnskap-transparent.png",
	},
	{
		id: "infrastructure",
		title: "Kilder",
		position: "Nederste lag",
		description: "Systemene og kildene dere allerede bruker.",
		story: "Verevon kobler seg til avtalte systemer, dokumenter og datakilder. Fagsystemene blir stående, mens informasjonen blir tilgjengelig der arbeidet skjer.",
		capabilities: ["Webhuset", "Telenor", "Bineric", "Interne kilder"],
		framerLayer: 4,
		image: "/verevon-layers/kilder-transparent.png",
	},
];

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

const springTransition = {
	type: "spring",
	mass: 0.95,
	stiffness: 82,
	damping: 21,
} as const;

type StackFrame = {
	glowOpacity: number;
	scale: number;
	shadowOpacity: number;
	y: number;
};

const layerOrder: readonly LayerId[] = [
	"management",
	"orchestration",
	"business-context",
	"infrastructure",
];

const physicalLayerZIndex: Record<LayerId, number> = {
	management: 40,
	orchestration: 30,
	"business-context": 20,
	infrastructure: 10,
};

// This preserves the original vertical flow: Kontroll is clearly lifted above
// the other three plates, while the stack remains visibly continuous below it.
// Selecting a lower layer moves only the necessary plates, rather than turning
// the whole visual into a single front-facing card.
const stackLayouts: Record<LayerId, Record<LayerId, StackFrame>> = {
	management: {
		management: { y: -220, scale: 1.06, glowOpacity: 1, shadowOpacity: 0.18 },
		orchestration: { y: 68, scale: 0.9, glowOpacity: 0.2, shadowOpacity: 0.08 },
		"business-context": { y: 112, scale: 0.865, glowOpacity: 0.06, shadowOpacity: 0.055 },
		infrastructure: { y: 152, scale: 0.845, glowOpacity: 0.05, shadowOpacity: 0.06 },
	},
	orchestration: {
		management: { y: -300, scale: 0.9, glowOpacity: 0.22, shadowOpacity: 0.095 },
		orchestration: { y: 44, scale: 1.06, glowOpacity: 0.96, shadowOpacity: 0.17 },
		"business-context": { y: 330, scale: 0.86, glowOpacity: 0.06, shadowOpacity: 0.06 },
		infrastructure: { y: 378, scale: 0.84, glowOpacity: 0.045, shadowOpacity: 0.055 },
	},
	"business-context": {
		management: { y: -300, scale: 0.9, glowOpacity: 0.22, shadowOpacity: 0.095 },
		orchestration: { y: -250, scale: 0.86, glowOpacity: 0.045, shadowOpacity: 0.055 },
		"business-context": { y: 92, scale: 1.06, glowOpacity: 0.94, shadowOpacity: 0.17 },
		infrastructure: { y: 350, scale: 0.86, glowOpacity: 0.06, shadowOpacity: 0.06 },
	},
	infrastructure: {
		management: { y: -218, scale: 0.92, glowOpacity: 0.24, shadowOpacity: 0.1 },
		orchestration: { y: -168, scale: 0.875, glowOpacity: 0.045, shadowOpacity: 0.055 },
		"business-context": { y: -122, scale: 0.855, glowOpacity: 0.05, shadowOpacity: 0.055 },
		infrastructure: { y: 210, scale: 1.08, glowOpacity: 0.68, shadowOpacity: 0.17 },
	},
};

function getStackState(layerId: LayerId, activeLayerId: LayerId) {
	const position = layerOrder.indexOf(layerId) - layerOrder.indexOf(activeLayerId);

	if (position === 0) return "front";
	return position > 0 ? `under-${position}` : `passed-${Math.abs(position)}`;
}

function useLayerOffsetScale() {
	const [scale, setScale] = useState(0.74);

	useEffect(() => {
		const update = () => {
			if (window.innerWidth < 640) return setScale(0.5);
			if (window.innerWidth < 900) return setScale(0.62);
			if (window.innerWidth < 1280) return setScale(0.68);
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
	prefersReducedMotion,
}: {
	active: boolean;
	layer: LayerDefinition;
	onSelect: (id: LayerId) => void;
	prefersReducedMotion: boolean;
}) {
	const panelId = `platform-layer-panel-${layer.id}`;
	const transition = prefersReducedMotion
		? { duration: 0 }
		: { duration: 0.36, ease: [0.22, 1, 0.36, 1] as const };

	return (
		<div
			className="relative border-t border-verevon-j-text/15 last:border-b"
			data-layer-disclosure="intric"
		>
			<button
				aria-controls={panelId}
				aria-expanded={active}
				className="group grid w-full cursor-pointer grid-cols-[20px_minmax(0,1fr)] items-start gap-x-5 bg-transparent py-[clamp(18px,1.8vw,24px)] text-left outline-none focus-visible:rounded-[2px] focus-visible:ring-2 focus-visible:ring-verevon-coral/40"
				data-layer-row="intric"
				onClick={() => onSelect(layer.id)}
				type="button"
			>
				<span
					aria-hidden="true"
					className="mt-[2px] flex h-5 w-5 items-center justify-center text-verevon-j-text/75 transition-colors duration-200 group-hover:text-verevon-j-text"
					data-layer-toggle
				>
					{active ? <Minus size={15} strokeWidth={1.35} /> : <Plus size={15} strokeWidth={1.35} />}
				</span>
				<span className="min-w-0">
					<span className="mb-2.5 block font-geist text-[0.66rem] font-normal uppercase leading-none tracking-[0.065em] text-verevon-j-text/58" data-layer-position>
						{layer.position}
					</span>
					<motion.span
						animate={{ opacity: active ? 1 : 0.74 }}
						className="block font-geist text-[clamp(0.94rem,1vw,1.04rem)] font-medium leading-[1.15] tracking-[-0.02em] text-verevon-j-text"
						data-layer-title
						initial={false}
						transition={transition}
					>
						{layer.title}
					</motion.span>
				</span>
			</button>
			<div aria-hidden={!active} id={panelId}>
				<motion.div
					animate={{ height: active ? "auto" : 0, opacity: active ? 1 : 0 }}
					className="overflow-hidden"
					initial={false}
					transition={transition}
				>
					<div className="pb-6 pl-10">
						<p className="max-w-[440px] font-manrope text-[clamp(0.96rem,0.95vw,1.04rem)] font-light leading-[1.5] tracking-[-0.015em] text-verevon-j-text/75">
							{layer.description}
						</p>
						<p className="max-w-[440px] pt-3 font-manrope text-[clamp(0.96rem,0.95vw,1.04rem)] font-light leading-[1.5] tracking-[-0.015em] text-verevon-j-text/75">
							{layer.story}
						</p>
						<div className="flex flex-wrap gap-2 pt-5">
							{layer.capabilities.map((capability) => (
								<span className="border-t border-verevon-j-text/18 pt-2 font-geist text-[0.65rem] font-normal uppercase leading-none tracking-[0.065em] text-verevon-j-text/68" key={capability}>
									{capability}
								</span>
							))}
						</div>
					</div>
				</motion.div>
			</div>
		</div>
	);
}

export function LayerSection() {
	const [activeLayerId, setActiveLayerId] = useState<LayerId>("management");
	// The browser preference resolves after hydration. Initial state has no entry
	// motion; transitions only happen after a visitor selects another layer.
	const prefersReducedMotion = usePrefersReducedMotion();
	const offsetScale = useLayerOffsetScale();
	const activeLayout = stackLayouts[activeLayerId];

	return (
		<section
			className="verevon-platform-section relative isolate overflow-visible py-24 text-verevon-j-text lg:py-32"
			id="plattform"
		>
			<div className="relative z-[1] mx-auto w-full px-[var(--verevon-edge)] max-[760px]:px-[var(--verevon-page-pad)]">
				<header className="grid grid-cols-1 gap-x-8 gap-y-6 md:grid-cols-3">
					<div className="md:col-span-2 md:col-start-2">
						<h2 className="verevon-home-heading text-verevon-j-text">
							Alt i sammenheng. Kontroll hele veien.
						</h2>

						<p className="mt-4 max-w-[522px] font-manrope text-[clamp(1rem,1.05vw,1.125rem)] font-light leading-[1.5] tracking-[-0.015em] text-verevon-text-muted">
							Systemene deres blir der de er. Verevon samler informasjon, kunnskap,
							AI-arbeid og kontroll i én felles arbeidsflate — fra første signal til
							oppfølging. Velg et lag for å se hvordan.
						</p>
					</div>
				</header>

				<div className="mb-16 md:mb-24" />

				<div className="grid grid-cols-1 items-start gap-y-10 md:grid-cols-[minmax(0,1.07fr)_minmax(320px,0.93fr)] md:gap-x-[clamp(42px,6vw,100px)]">
					<div
						className="verevon-platform-stage relative w-full overflow-visible p-[clamp(24px,3vw,52px)]"
						data-platform-stage=""
						style={{ aspectRatio: "3/4" }}
					>
						<span className="relative z-[2] block max-w-[280px] font-geist text-[0.7rem] font-medium uppercase leading-[1.4] tracking-[0.08em] text-verevon-text-muted/70">
							Kilder, kunnskap, agentarbeid, kontroll
						</span>

						<div className="absolute inset-x-[clamp(12px,3vw,44px)] bottom-[clamp(16px,3vw,48px)] top-[clamp(64px,8vw,112px)]">
							{layers.map((layer) => {
								const isActive = layer.id === activeLayerId;
								const frame = activeLayout[layer.id];

								return (
									<motion.div
										aria-hidden="true"
										animate={{
										opacity: 1,
										scale: frame.scale,
										y: frame.y * offsetScale,
										}}
										className="pointer-events-none absolute inset-0 flex items-center justify-center"
										data-active={isActive}
										data-framer-layer={layer.framerLayer}
										data-layer-id={layer.id}
									data-stack-offset={frame.y}
									data-stack-state={getStackState(layer.id, activeLayerId)}
										data-stack-visible="true"
										initial={false}
										key={layer.id}
										style={{
										filter: `drop-shadow(0 ${isActive ? 28 : 15}px ${
											isActive ? 42 : 22
										}px rgba(31, 31, 29, ${frame.shadowOpacity}))`,
										zIndex: physicalLayerZIndex[layer.id],
										}}
										transition={
											prefersReducedMotion
												? { duration: 0 }
												: springTransition
										}
									>
										<motion.div
										animate={{ opacity: frame.glowOpacity }}
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

										<div className="relative aspect-[2752/1536] w-[min(88vw,680px)] md:w-[min(50vw,680px)]">
											<Image
												alt=""
												className="select-none object-contain"
												draggable={false}
												fill
												sizes="(max-width: 720px) 88vw, (max-width: 1280px) 50vw, 680px"
												src={layer.image}
											/>
										</div>
									</motion.div>
								);
							})}
						</div>
					</div>

					<nav
						aria-label="Velg lag i plattformen"
						className="grid content-start pt-1"
					>
							{layers.map((layer) => (
								<LayerCallout
									active={layer.id === activeLayerId}
									key={layer.id}
									layer={layer}
									onSelect={setActiveLayerId}
									prefersReducedMotion={prefersReducedMotion}
								/>
							))}
						</nav>
					</div>
				</div>
		</section>
	);
}

export default LayerSection;
