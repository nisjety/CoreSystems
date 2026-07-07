"use client";

import Image from "next/image";
import { type CSSProperties, useEffect, useRef, useState } from "react";
import { PromptCycle } from "./PromptCycle";

type Card = {
	kicker: string;
	title: string;
	text: string;
	image: string;
};

const cards: Card[] = [
	{
		kicker: "Svar",
		title: "Skriver utkast",
		text: "Lager utkast til e-post, chat og meldinger med synlige kildespor før noe sendes.",
		image: "/velion-product-shots/chat-draft-answer-top.png",
	},
	{
		kicker: "Kunnskap",
		title: "Bygger kunnskap",
		text: "Kobler til nettsider, dokumenter og integrasjoner, og bygger en arbeidsminne mennesker kan inspisere.",
		image: "/velion-product-shots/dashboard-composer-prompt-state.png",
	},
	{
		kicker: "Styr",
		title: "Ruter risiko",
		text: "Finner trege saker, klassifiserer hastegrad og spør riktig menneske før eskalering.",
		image: "/velion-product-shots/chat-agent-steps.png",
	},
	{
		kicker: "Spor",
		title: "Reviderer alt",
		text: "Registrerer godkjenninger, policy-sjekker, koblingsstatus og tilbakerulling for hver arbeidsflyt.",
		image: "/velion-product-shots/inbox-empty-workspace.png",
	},
];

// Horizontal spread each card converges from (trust-scroll "fly into place").
const flyX = ["44%", "16%", "-16%", "-44%"];

export function FeatureCardsSectionV2() {
	const stageRef = useRef<HTMLDivElement>(null);
	const [inView, setInView] = useState(false);

	useEffect(() => {
		const stage = stageRef.current;

		if (!stage) {
			return;
		}

		const observer = new IntersectionObserver(
			(entries) => {
				for (const entry of entries) {
					if (entry.isIntersecting) {
						setInView(true);
						observer.disconnect();
						break;
					}
				}
			},
			{ threshold: 0.2, rootMargin: "0px 0px -10% 0px" },
		);

		observer.observe(stage);
		return () => observer.disconnect();
	}, []);

	return (
		<section
			aria-label="Forankret handling"
			className="relative isolate overflow-hidden border-b border-velion-j-text/8 bg-background px-[var(--velion-edge)] py-[var(--velion-section-vpad)] text-velion-j-text max-[760px]:px-[var(--velion-page-pad)]"
			id="produkt"
		>
			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-0 z-0 bg-[radial-gradient(circle_at_50%_30%,rgba(238,122,80,0.05),transparent_30%)]"
			/>

			<div className="relative z-[1] mx-auto max-w-[1680px]">
				<div className="mx-auto flex max-w-[660px] flex-col items-center gap-7 text-center">
					<p className="velion-eyebrow">Forankret handling</p>

					<h2
						className="fade-out-top m-0 font-arbeit text-[clamp(2.6rem,5vw,6.4rem)] font-light leading-[0.94] tracking-[-0.07em] text-velion-j-text text-balance"
						data-fade-out-top
					>
						Tenking, satt i arbeid
					</h2>

					<p className="velion-body-lg max-w-[540px] text-pretty">
						Ett kundesignal kan bli et svar, en rute, en policy-sjekk og en
						revidert handling — alt med kildene synlige.
					</p>

					<PromptCycle />
				</div>

				<div
					className="velion-fly-stage relative mt-[clamp(52px,6.5vw,96px)]"
					data-in={inView ? "true" : undefined}
					ref={stageRef}
				>
					<div
						aria-hidden="true"
						className="pointer-events-none absolute -inset-x-[clamp(8px,2vw,40px)] -inset-y-[clamp(16px,3vw,48px)] z-0 bg-[linear-gradient(90deg,rgba(23,23,23,0.04)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.035)_1px,transparent_1px)] bg-[length:calc(100%/4)_100%,100%_50%] opacity-60"
					/>

					<div className="relative z-[1] grid gap-x-[clamp(16px,1.6vw,28px)] gap-y-[clamp(28px,3vw,40px)] sm:grid-cols-2 xl:grid-cols-4">
						{cards.map((card, index) => (
							<article
								className="velion-fly-card group flex min-w-0 flex-col gap-3"
								key={card.title}
								style={
									{
										"--fly-x": flyX[index],
										"--fly-i": index,
									} as CSSProperties
								}
							>
								<div className="flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-velion-j-text/42">
									<span className="flex items-center gap-1.5">
										<span className="size-[5px] rounded-full bg-velion-coral/70" />
										{card.kicker}
									</span>
									<span>Velion-resultat</span>
								</div>

								<div className="relative aspect-[3/4] overflow-hidden rounded-[4px] border border-velion-j-text/8 bg-white/55 shadow-[var(--velion-shadow-sm)]">
									<Image
										alt=""
										className="object-cover opacity-[0.8] saturate-[0.78] transition-transform duration-700 group-hover:scale-[1.04]"
										fill
										sizes="(max-width: 640px) 90vw, (max-width: 1280px) 45vw, 23vw"
										src={card.image}
									/>
									<div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(248,248,247,0.06),rgba(248,248,247,0.7))]" />
									<h3 className="absolute bottom-4 left-4 right-4 m-0 font-arbeit text-[clamp(1.45rem,1.7vw,2rem)] font-light leading-[1.04] tracking-[-0.05em] text-velion-j-text">
										{card.title}
									</h3>
								</div>

								<p className="m-0 font-protokoll text-[var(--text-body-sm)] font-light leading-[1.4] text-velion-text-muted/90">
									{card.text}
								</p>
							</article>
						))}
					</div>

					{["left-0 top-0", "right-0 top-0", "left-0 bottom-0", "right-0 bottom-0"].map(
						(corner) => (
							<span
								aria-hidden="true"
								className={`pointer-events-none absolute z-[2] size-[6px] rounded-full bg-velion-j-text/20 ${corner}`}
								key={corner}
							/>
						),
					)}
				</div>
			</div>
		</section>
	);
}

export default FeatureCardsSectionV2;
