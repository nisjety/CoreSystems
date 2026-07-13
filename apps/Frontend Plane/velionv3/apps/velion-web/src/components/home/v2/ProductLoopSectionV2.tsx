"use client";

import type { CSSProperties } from "react";
import { useRef } from "react";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { EditorialGrid } from "../extras/EditorialGrid";
import { Reveal } from "../sections/Reveal";
import { useEditorialParallax } from "../sections/useEditorialParallax";

const loopStates = ["Spør", "Utkast", "Godkjenn", "Revider"];

/**
 * ProductLoopSectionV2 — the work loop, carried on the shared editorial grid.
 * The dashboard recording stays ambient with a small media parallax only.
 */
export function ProductLoopSectionV2() {
	const sectionRef = useRef<HTMLElement>(null);

	useEditorialParallax(sectionRef, {
		id: "velion-product-loop",
		refreshPriority: 5,
	});

	return (
		<section
			className="relative isolate overflow-hidden border-b border-velion-j-text/8 bg-background text-velion-j-text"
			id="flyt"
			ref={sectionRef}
		>
			<EditorialGrid className="z-[6]" />

			<div className="relative z-[4] grid min-h-[min(840px,92svh)] grid-cols-4 px-[var(--velion-edge)] max-[1100px]:grid-cols-1 max-[760px]:px-[var(--velion-page-pad)]">
				<Reveal className="col-span-2 grid max-w-[620px] content-center py-[clamp(80px,8.4vw,124px)] pr-[clamp(34px,5.2vw,96px)] max-[1100px]:max-w-none max-[1100px]:pr-0">
					<p className="velion-eyebrow text-[color-mix(in_srgb,var(--velion-a-earth)_78%,var(--velion-j-text))]">
						Tidlig tilgang
					</p>

					<h2
						className="fade-out-top m-0 mt-[clamp(24px,3vw,40px)] max-w-[540px] font-arbeit text-[clamp(2.8rem,5vw,6.4rem)] font-light leading-[0.92] tracking-[-0.06em] text-velion-j-text text-balance"
						data-fade-out-top
					>
						Kjør kundearbeid på autopilot.
					</h2>

					<p
						className="velion-body-lg fade-out-top mt-[clamp(24px,2.6vw,38px)] max-w-[520px]"
						data-fade-out-top
					>
						Spør på naturlig språk. Velion kan skrive utkast, bygge
						arbeidsflyter,{" "}
						<span className="text-[color-mix(in_srgb,var(--velion-coral)_72%,var(--velion-j-text))]">
							stoppe for godkjenning
						</span>{" "}
						og holde kildesporet synlig før noen handling når en
						kunde.
					</p>

					<div className="mt-[clamp(34px,4vw,56px)]">
						<ArrowButton href="#plattform" variant="coral">
							Følg arbeidssløyfen
						</ArrowButton>
					</div>
				</Reveal>

				<Reveal className="relative col-span-2 min-h-[min(840px,92svh)] min-w-0 overflow-hidden border-l border-velion-j-text/8 bg-[linear-gradient(90deg,rgba(255,255,255,0.24),rgba(255,255,255,0)),color-mix(in_srgb,var(--velion-bg-soft)_92%,var(--velion-a-earth))] max-[1100px]:min-h-[560px] max-[1100px]:border-l-0 max-[1100px]:border-t">
					<div className="absolute inset-0 bg-[radial-gradient(circle_at_72%_22%,rgba(238,122,80,0.08),transparent_25%),linear-gradient(180deg,rgba(248,248,247,0),rgba(248,248,247,0.32))]" />

					<div className="absolute inset-[clamp(56px,7vw,110px)_clamp(42px,4.5vw,72px)_clamp(48px,6vw,90px)_clamp(42px,4.5vw,72px)] grid grid-rows-[minmax(0,1fr)_auto] content-center gap-[clamp(14px,1.4vw,20px)] max-[1100px]:inset-[clamp(34px,6vw,72px)_clamp(24px,4vw,56px)]">
						<div
							className="relative aspect-[1.818] w-full max-w-[1040px] self-center justify-self-end overflow-hidden rounded-l-[18px] border border-[rgba(31,31,29,0.1)] bg-white/85 shadow-[var(--velion-shadow-lg)] will-change-transform max-[1100px]:justify-self-center max-[1100px]:rounded-[18px]"
							data-editorial-media=""
						>
							<video
								aria-label="Opptak av Velion-dashbordet der norske instruksjoner skrives inn."
								autoPlay
								className="block size-full object-cover object-center"
								loop
								muted
								playsInline
								poster="/velion-product-shots/dashboard-expanded-prompt.png"
								preload="none"
							>
								<source
									src="/velion-product-shots/velion-dashboard-typing.mp4"
									type="video/mp4"
								/>
							</video>
						</div>

						<div className="grid w-full max-w-[760px] grid-cols-4 gap-[clamp(8px,0.8vw,12px)] justify-self-end max-[1100px]:justify-self-center max-[640px]:grid-cols-2">
							{loopStates.map((state, index) => (
								<div
									className="relative overflow-hidden border border-velion-j-text/10 bg-white/48 px-4 py-3 backdrop-blur-[14px]"
									key={state}
									style={
										{
											"--state-index": index,
										} as CSSProperties
									}
								>
									<span
										aria-hidden="true"
										className="absolute inset-y-0 left-0 w-[3px] bg-velion-coral/70 opacity-60"
									/>
									<span className="block font-arbeit text-[0.76rem] font-normal uppercase leading-none tracking-[0.14em] text-velion-j-text/42">
										{String(index + 1).padStart(2, "0")}
									</span>
									<span className="mt-2 block font-protokoll text-[var(--text-body-sm)] font-light leading-none text-velion-j-text/76">
										{state}
									</span>
								</div>
							))}
						</div>
					</div>
				</Reveal>
			</div>
		</section>
	);
}

export default ProductLoopSectionV2;
