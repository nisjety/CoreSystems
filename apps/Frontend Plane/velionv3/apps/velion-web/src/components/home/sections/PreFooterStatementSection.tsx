"use client";

import { useEffect, useRef } from "react";
import { ArrowButton } from "@/components/ui/ArrowButton";

export function PreFooterStatementSection() {
	const sectionRef = useRef<HTMLElement | null>(null);

	useEffect(() => {
		let ctx: { revert: () => void } | undefined;
		let disposed = false;

		async function setupAnimation() {
			const [{ default: gsap }, { ScrollTrigger }] = await Promise.all([
				import("gsap"),
				import("gsap/ScrollTrigger"),
			]);

			if (disposed || !sectionRef.current) {
				return;
			}

			gsap.registerPlugin(ScrollTrigger);

			ctx = gsap.context(() => {
				const section = sectionRef.current;

				if (!section) {
					return;
				}

				const reduceMotion = window.matchMedia(
					"(prefers-reduced-motion: reduce)",
				).matches;

				const footerBg = section.querySelector<HTMLElement>(
					"[data-prefooter-footer-bg]",
				);
				const firstPhase = section.querySelector<HTMLElement>(
					"[data-prefooter-first-phase]",
				);
				const secondPhase = section.querySelector<HTMLElement>(
					"[data-prefooter-second-phase]",
				);
				const secondPhaseButtons = section.querySelector<HTMLElement>(
					"[data-prefooter-second-buttons]",
				);
				const dots = section.querySelector<HTMLElement>("[data-prefooter-dots]");
				const cards = section.querySelectorAll<HTMLElement>(
					"[data-prefooter-card]",
				);

				if (!footerBg || !firstPhase || !secondPhase || !secondPhaseButtons) {
					return;
				}

				if (reduceMotion) {
					gsap.set(footerBg, { autoAlpha: 1 });
					gsap.set(firstPhase, { autoAlpha: 0 });
					gsap.set(secondPhase, { autoAlpha: 1, y: 0, scale: 1 });
					gsap.set(secondPhaseButtons, { y: 0 });
					gsap.set(dots, { autoAlpha: 0.16 });
					gsap.set(cards, { autoAlpha: 1, y: 0, scale: 1, rotate: 0 });
					return;
				}

				gsap.set(footerBg, { autoAlpha: 0 });
				gsap.set(firstPhase, { autoAlpha: 1, y: 0, scale: 1 });
				gsap.set(secondPhase, { autoAlpha: 0, y: 42, scale: 0.985 });
				gsap.set(secondPhaseButtons, { y: 0 });
				gsap.set(dots, { autoAlpha: 0.12, scale: 1.02 });

				gsap.set(cards, {
					autoAlpha: 0,
					y: 48,
					scale: 0.94,
					rotate: (index) => [-5, 4, -3, 5, -4][index] ?? 0,
				});

				gsap
					.timeline({
						scrollTrigger: {
							trigger: section,
							start: "top top",
							end: "bottom 72%",
							scrub: 0.45,
							id: "velion-prefooter-two-phase",
							invalidateOnRefresh: true,
						},
					})
					.to(
						cards,
						{
							autoAlpha: 1,
							y: 0,
							scale: 1,
							stagger: 0.035,
							ease: "none",
							duration: 0.3,
						},
						0,
					)
					.to(
						dots,
						{
							autoAlpha: 0.24,
							scale: 1,
							ease: "none",
							duration: 0.3,
						},
						0,
					)
					.to(
						firstPhase,
						{
							autoAlpha: 0,
							y: -36,
							scale: 0.985,
							ease: "none",
							duration: 0.28,
						},
						0.36,
					)
					.to(
						footerBg,
						{
							autoAlpha: 1,
							ease: "none",
							duration: 0.34,
						},
						0.4,
					)
					.to(
						secondPhase,
						{
							autoAlpha: 1,
							y: 0,
							scale: 1,
							ease: "none",
							duration: 0.36,
						},
						0.5,
					)
					.to(
						secondPhaseButtons,
						{
							y: () => Math.min(window.innerHeight * 0.28, 260),
							ease: "none",
							duration: 0.34,
						},
						0.66,
					);
			}, sectionRef);

			window.requestAnimationFrame(() => ScrollTrigger.refresh());
		}

		setupAnimation();

		return () => {
			disposed = true;
			ctx?.revert();
		};
	}, []);

	return (
		<section
			ref={sectionRef}
			aria-label="Velion avsluttende oppfordring"
			className="relative min-h-[145svh] overflow-clip bg-velion-footer-bg text-velion-j-text"
			data-prefooter-scroll
		>
			<div className="sticky top-0 z-[1] h-[72svh] min-h-[560px] overflow-hidden bg-background">
				<div
					aria-hidden="true"
					className="absolute inset-0 z-0 bg-velion-footer-bg opacity-0"
					data-prefooter-footer-bg
				/>

				<svg
					aria-hidden="true"
					className="pointer-events-none absolute inset-0 z-[1] h-full w-full opacity-0"
					data-prefooter-dots
				>
					<defs>
						<pattern
							id="velion-prefooter-dot-grid"
							width="180"
							height="180"
							patternUnits="userSpaceOnUse"
							x="40"
							y="40"
						>
							<rect
								width="4"
								height="4"
								fill="var(--velion-j-text)"
								opacity="0.16"
							/>
						</pattern>
					</defs>

					<rect
						width="100%"
						height="100%"
						fill="url(#velion-prefooter-dot-grid)"
					/>
				</svg>

				<div
					aria-hidden="true"
					className="pointer-events-none absolute inset-0 z-[2]"
				>
					<div
						className="absolute -left-[6%] top-[12%] aspect-square w-20 overflow-hidden rounded-[2px] bg-velion-bg-mist shadow-[0_10px_28px_rgba(23,23,23,0.035)] md:left-[2%] md:top-[10%] md:w-[100px] lg:w-[10%]"
						data-prefooter-card
					>
						<div className="absolute inset-0 bg-[radial-gradient(circle_at_34%_28%,rgba(238,122,80,0.32),transparent_28%),linear-gradient(135deg,#f8f8f7,#d9e3e4)]" />
						<div className="absolute inset-x-[18%] bottom-[24%] h-px bg-velion-j-text/25" />
						<div className="absolute left-[28%] top-[26%] size-7 rounded-full border border-velion-j-text/20" />
					</div>

					<div
						className="absolute left-[34%] top-[2%] aspect-[0.91] w-[72px] overflow-hidden rounded-[2px] bg-velion-bg-mist shadow-[0_10px_28px_rgba(23,23,23,0.035)] md:left-[32%] md:top-0 md:w-[88px] lg:w-[8%]"
						data-prefooter-card
					>
						<div className="absolute inset-0 bg-[linear-gradient(90deg,rgba(23,23,23,0.08)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.08)_1px,transparent_1px),linear-gradient(145deg,#ffffff,#ecebea)] bg-[length:24px_24px,24px_24px,100%_100%]" />
						<div className="absolute bottom-[18%] left-[18%] h-[42%] w-[22%] bg-velion-j-text/20 blur-[1px]" />
						<div className="absolute bottom-[18%] right-[24%] h-[34%] w-[18%] bg-velion-j-text/16 blur-[1px]" />
					</div>

					<div
						className="absolute -right-[4%] top-[4%] aspect-square w-20 overflow-hidden rounded-full bg-velion-coral-soft shadow-[0_10px_28px_rgba(23,23,23,0.035)] md:right-[14%] md:top-[2%] md:w-[88px] lg:w-[8%]"
						data-prefooter-card
					>
						<div className="absolute inset-0 bg-[radial-gradient(circle_at_50%_45%,rgba(238,122,80,0.44),transparent_30%),radial-gradient(circle_at_46%_52%,rgba(121,56,25,0.18),transparent_46%)]" />
						<div className="absolute left-1/2 top-[18%] h-[62%] w-px -translate-x-1/2 bg-velion-a-earth/30" />
						<div className="absolute left-[28%] top-[42%] h-px w-[44%] rotate-[-28deg] bg-velion-a-earth/30" />
					</div>

					<div
						className="absolute bottom-[6%] left-[4%] aspect-[1.08] w-24 overflow-hidden rounded-[2px] bg-velion-coral shadow-[0_10px_28px_rgba(23,23,23,0.035)] md:bottom-[8%] md:left-[18%] md:w-[100px] lg:w-[8%]"
						data-prefooter-card
					>
						<div className="absolute inset-0 bg-[linear-gradient(145deg,rgba(238,122,80,0.94),rgba(247,200,168,0.88))]" />
						<div className="absolute left-[28%] top-[34%] size-2 rounded-full bg-velion-j-text/55" />
						<div className="absolute right-[28%] top-[34%] size-2 rounded-full bg-velion-j-text/55" />
						<div className="absolute left-[30%] top-[58%] h-px w-[40%] rounded-full bg-velion-j-text/55" />
					</div>

					<div
						className="absolute top-[64%] right-[16%] aspect-square w-[100px] overflow-hidden rounded-[2px] bg-velion-bg-mist shadow-[0_10px_28px_rgba(23,23,23,0.035)] md:bottom-[3%] md:right-[26%] md:w-[110px] lg:w-[10%]"
						data-prefooter-card
					>
						<div className="absolute inset-0 bg-[linear-gradient(180deg,#eef4ff,#f8f8f7)]" />
						<div className="absolute bottom-[12%] left-1/2 h-[70%] w-px -translate-x-1/2 bg-velion-h-teal-deep/35" />
						<div className="absolute left-[20%] top-[28%] h-px w-[48%] rotate-[28deg] bg-velion-h-teal-deep/35" />
						<div className="absolute right-[18%] top-[38%] h-px w-[42%] rotate-[-18deg] bg-velion-h-teal-deep/35" />
					</div>
				</div>

				<div className="absolute inset-0 z-[3] grid place-items-center overflow-hidden px-[clamp(24px,4vw,72px)]">
					<div
						className="absolute left-1/2 top-1/2 grid w-[min(94vw,1280px)] -translate-x-1/2 -translate-y-1/2 justify-items-center text-center"
						data-prefooter-first-phase
					>
						<h2 className="m-0 max-w-none whitespace-nowrap font-arbeit text-[clamp(1.5rem,4vw,5.5rem)] font-light leading-[0.88] tracking-[-0.08em] text-velion-j-text max-[760px]:whitespace-normal">
							Hver kunde trenger riktig svar.
						</h2>
					</div>

					<div
						className="absolute left-1/2 top-1/2 grid w-[min(94vw,1280px)] -translate-x-1/2 -translate-y-1/2 justify-items-center text-center opacity-0"
						data-prefooter-second-phase
					>
						<h2 className="m-0 max-w-none whitespace-nowrap font-arbeit text-[clamp(1.5rem,4vw,5.5rem)] font-light leading-[0.88] tracking-[-0.08em] text-velion-j-text max-[760px]:whitespace-normal">
							Vi gjør svaret klart.
						</h2>

						<div
							className="mt-[clamp(32px,4vw,58px)] flex flex-wrap items-center justify-center gap-5"
							data-prefooter-second-buttons
						>
							<ArrowButton href="#produkt" variant="muted">
								Se produktet
							</ArrowButton>

							<ArrowButton href="#kontakt" variant="dark">
								Snakk med oss
							</ArrowButton>
						</div>
					</div>
				</div>
			</div>

			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-x-0 bottom-0 z-0 h-[42svh] bg-[linear-gradient(180deg,transparent_0%,rgba(246,246,244,0.18)_24%,rgba(246,246,244,0.64)_58%,var(--velion-footer-bg)_100%)]"
			/>
		</section>
	);
}

export default PreFooterStatementSection;
