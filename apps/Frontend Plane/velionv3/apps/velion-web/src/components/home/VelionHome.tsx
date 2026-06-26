"use client";

import { useEffect, useRef, useState } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { Footer } from "@/components/core/footer/Footer";
import { Navbar } from "@/components/core/navbar/Navbar";
import { MenuModal } from "@/components/ui/MenuModal";
import { HeroSection } from "./sections/HeroSection";
import { BrandLogosSection } from "./sections/BrandLogosSection";
import { TrustScrollSection } from "./sections/trust-scroll";
import { LayerSection } from "./sections/layer-section";
import { ProductLoopSection } from "./sections/ProductLoopSection";
import { PromptSection } from "./sections/PromptSection";
import { SensesSection } from "./sections/SensesSection";
import { PreFooterStatementSection } from "./sections/PreFooterStatementSection";

gsap.registerPlugin(ScrollTrigger);

export function VelionHome() {
	const homeRef = useRef<HTMLDivElement>(null);
	const [isMenuOpen, setIsMenuOpen] = useState(false);
	const [isScrolled, setIsScrolled] = useState(false);

	useEffect(() => {
		let frame: number | null = null;

		const updateScrollState = () => {
			setIsScrolled((current) => {
				const next = window.scrollY > window.innerHeight * 0.86;
				return current === next ? current : next;
			});

			frame = null;
		};

		const onScrollOrResize = () => {
			if (frame !== null) {
				return;
			}

			frame = window.requestAnimationFrame(updateScrollState);
		};

		updateScrollState();

		window.addEventListener("scroll", onScrollOrResize, { passive: true });
		window.addEventListener("resize", onScrollOrResize);

		return () => {
			if (frame !== null) {
				window.cancelAnimationFrame(frame);
			}

			window.removeEventListener("scroll", onScrollOrResize);
			window.removeEventListener("resize", onScrollOrResize);
		};
	}, []);

	useEffect(() => {
		let animationContext: { revert: () => void } | null = null;
		if (!homeRef.current) {
			return;
		}

		animationContext = gsap.context(() => {
			const reduceMotion = window.matchMedia(
				"(prefers-reduced-motion: reduce)",
			).matches;
			const nav =
				homeRef.current?.querySelector<HTMLElement>("[data-site-nav]");

			const hero = homeRef.current?.querySelector<HTMLElement>(
				"[data-hero-parallax]",
			);
			const heroMedia = homeRef.current?.querySelector<HTMLElement>(
				"[data-hero-parallax-media]",
			);
			const heroContent = homeRef.current?.querySelector<HTMLElement>(
				"[data-hero-parallax-content]",
			);

			if (hero && heroMedia && heroContent) {
				gsap.set([heroMedia, heroContent], {
					yPercent: 0,
					force3D: true,
				});

				const loadFrame = hero.querySelector<HTMLElement>(
					"[data-hero-load-frame]",
				);
				const introVideo = hero.querySelector<HTMLVideoElement>(
					"[data-hero-load-intro-video]",
				);
				const finalMedia = hero.querySelector<HTMLElement>(
					"[data-hero-load-final-media]",
				);
				const finalImage = hero.querySelector<HTMLElement>(
					"[data-hero-load-final-image]",
				);
				const finalOverlays = hero.querySelectorAll<HTMLElement>(
					"[data-hero-load-final-overlay]",
				);
				const copyItems = hero.querySelectorAll<HTMLElement>(
					"[data-hero-load-copy-item]",
				);

				if (
					loadFrame &&
					introVideo &&
					finalMedia &&
					finalImage &&
					copyItems.length > 0
				) {
					const initialFrameClip =
						"inset(67% 35% 11% 35% round 22px)";
					const settleFrameClip = "inset(52% 27% 22% 27% round 24px)";
					const bridgeFrameClip = "inset(40% 22% 26% 22% round 26px)";
					const nearFullFrameClip = "inset(8% 4% 6% 4% round 28px)";

					if (reduceMotion) {
						introVideo.pause();
						introVideo.currentTime = 0;

						gsap.set(loadFrame, {
							autoAlpha: 1,
							clipPath: "inset(0 0 0 0 round 0)",
							yPercent: 0,
						});
						gsap.set(introVideo, {
							autoAlpha: 0,
							yPercent: 0,
							scale: 1,
						});
						gsap.set(finalMedia, { autoAlpha: 1 });
						gsap.set(finalImage, { yPercent: 0, scale: 1 });
						gsap.set(finalOverlays, { autoAlpha: 1 });
						gsap.set(copyItems, { autoAlpha: 1, y: 0 });
						if (nav) {
							gsap.set(nav, { autoAlpha: 1, y: 0 });
						}
					} else {
						introVideo.pause();
						introVideo.currentTime = 0;

						gsap.set(loadFrame, {
							autoAlpha: 0,
							clipPath: initialFrameClip,
							yPercent: 18,
							force3D: true,
						});
						gsap.set(introVideo, {
							autoAlpha: 1,
							yPercent: 22,
							scale: 1.14,
							force3D: true,
							transformOrigin: "50% 50%",
						});
						gsap.set(finalMedia, { autoAlpha: 0 });
						gsap.set(finalImage, {
							yPercent: 22,
							scale: 1.05,
							force3D: true,
							transformOrigin: "50% 50%",
						});
						gsap.set(finalOverlays, { autoAlpha: 0 });
						gsap.set(copyItems, {
							autoAlpha: 0,
							y: 28,
							force3D: true,
						});
						if (nav) {
							gsap.set(nav, {
								autoAlpha: 0,
								y: -18,
								force3D: true,
							});
						}

						const introTimeline = gsap.timeline({
							defaults: {
								ease: "power3.inOut",
							},
						});

						introTimeline
							.add(() => {
								introVideo.currentTime = 0;

								const playPromise = introVideo.play();
								if (
									playPromise &&
									typeof playPromise.catch === "function"
								) {
									playPromise.catch(() => {});
								}
							}, 0.36)
							.to(
								loadFrame,
								{ autoAlpha: 1, duration: 0.01 },
								0.36,
							)
							.to(
								loadFrame,
								{
									clipPath: settleFrameClip,
									yPercent: 0,
									duration: 0.7,
									ease: "power3.out",
								},
								0.36,
							)
							.to(
								introVideo,
								{
									yPercent: 8,
									scale: 1.045,
									duration: 0.7,
									ease: "power2.out",
								},
								0.36,
							)
							.to(
								loadFrame,
								{
									clipPath: bridgeFrameClip,
									duration: 0.42,
									ease: "power2.inOut",
								},
								1.08,
							)
							.to(
								introVideo,
								{
									yPercent: 2,
									scale: 1.01,
									duration: 0.42,
									ease: "power2.inOut",
								},
								1.08,
							)
							.to(
								loadFrame,
								{
									clipPath: nearFullFrameClip,
									duration: 0.82,
									ease: "power4.inOut",
								},
								1.38,
							)
							.to(
								introVideo,
								{
									yPercent: 0,
									scale: 1,
									duration: 0.82,
									ease: "power3.inOut",
								},
								1.38,
							)
							.to(
								finalMedia,
								{
									autoAlpha: 1,
									duration: 0.48,
									ease: "power2.out",
								},
								1.84,
							)
							.to(
								finalImage,
								{
									yPercent: 0,
									scale: 1,
									duration: 0.92,
									ease: "power3.out",
								},
								1.84,
							)
							.to(
								finalOverlays,
								{
									autoAlpha: 1,
									duration: 0.56,
									stagger: 0.06,
									ease: "power2.out",
								},
								1.94,
							)
							.to(
								introVideo,
								{
									autoAlpha: 0,
									duration: 0.42,
									ease: "power2.out",
								},
								2.02,
							)
							.to(
								loadFrame,
								{
									clipPath: "inset(0 0 0 0 round 0)",
									duration: 0.64,
									ease: "power4.inOut",
								},
								1.74,
							)
							.to(
								copyItems,
								{
									autoAlpha: 1,
									y: 0,
									duration: 0.6,
									stagger: 0.08,
									ease: "power3.out",
								},
								2.34,
							)
							.add(() => {
								introVideo.pause();
							}, 2.92);

						if (nav) {
							introTimeline.to(
								nav,
								{
									autoAlpha: 1,
									y: 0,
									duration: 0.56,
									ease: "power3.out",
								},
								2.2,
							);
						}
					}
				}

				if (!reduceMotion) {
					const heroScrollTrigger = {
						trigger: hero,
						start: "top top",
						end: "bottom top",
						scrub: true,
						invalidateOnRefresh: true,
					} as const;

					gsap.to(heroMedia, {
						yPercent: 80,
						ease: "none",
						overwrite: "auto",
						scrollTrigger: {
							...heroScrollTrigger,
							id: "velion-hero-media-parallax",
						},
					});

					gsap.to(heroContent, {
						yPercent: 40,
						ease: "none",
						overwrite: "auto",
						scrollTrigger: {
							...heroScrollTrigger,
							id: "velion-hero-content-parallax",
						},
					});
				}
			}

			const footer = homeRef.current?.querySelector<HTMLElement>(
				"[data-footer-parallax]",
			);
			const footerMedia = homeRef.current?.querySelector<HTMLElement>(
				"[data-footer-parallax-media]",
			);

			if (footer && footerMedia) {
				gsap.set(footerMedia, {
					yPercent: reduceMotion ? 0 : -8,
					force3D: true,
				});

				if (!reduceMotion) {
					gsap.to(footerMedia, {
						yPercent: 0,
						ease: "none",
						scrollTrigger: {
							trigger: footer,
							start: "top bottom",
							end: "bottom bottom",
							scrub: true,
							id: "velion-footer-parallax",
							invalidateOnRefresh: true,
						},
					});
				}
			}

			window.requestAnimationFrame(() => ScrollTrigger.refresh());
		}, homeRef.current);

		return () => {
			animationContext?.revert();
		};
	}, []);

	useEffect(() => {
		let frame: number | null = null;

		const updateTextFades = () => {
			document
				.querySelectorAll<HTMLElement>("[data-fade-out-top]")
				.forEach((element) => {
					const rect = element.getBoundingClientRect();
					const maskY = -rect.top + 90;
					const position = `0px ${maskY.toFixed(3)}px`;

					element.style.setProperty("mask-position", position);
					element.style.setProperty(
						"-webkit-mask-position",
						position,
					);
				});

			frame = null;
		};

		const schedule = () => {
			if (frame !== null) {
				return;
			}

			frame = window.requestAnimationFrame(updateTextFades);
		};

		updateTextFades();

		window.addEventListener("scroll", schedule, { passive: true });
		window.addEventListener("resize", schedule);

		return () => {
			if (frame !== null) {
				window.cancelAnimationFrame(frame);
			}

			window.removeEventListener("scroll", schedule);
			window.removeEventListener("resize", schedule);
		};
	}, []);

	useEffect(() => {
		document.body.classList.toggle("velion-menu-open", isMenuOpen);

		const onKeyDown = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				setIsMenuOpen(false);
			}
		};

		window.addEventListener("keydown", onKeyDown);

		return () => {
			document.body.classList.remove("velion-menu-open");
			window.removeEventListener("keydown", onKeyDown);
		};
	}, [isMenuOpen]);

	useEffect(() => {
		const onDocumentClick = (event: MouseEvent) => {
			const target = event.target;

			if (!(target instanceof Element)) {
				return;
			}

			const anchor = target.closest<HTMLAnchorElement>('a[href^="#"]');
			const hash = anchor?.getAttribute("href");

			if (!anchor || !hash || hash === "#") {
				return;
			}

			const destination = document.querySelector(hash);

			if (!destination) {
				return;
			}

			event.preventDefault();
			destination.scrollIntoView({ behavior: "smooth", block: "start" });
			window.history.replaceState(null, "", hash);
		};

		document.addEventListener("click", onDocumentClick);

		return () => {
			document.removeEventListener("click", onDocumentClick);
		};
	}, []);

	return (
		<div
			className="min-h-screen bg-background text-velion-text [--velion-edge:clamp(56px,5.55vw,208px)] [--velion-page-pad:clamp(24px,4vw,56px)] [--velion-section-gap:clamp(86px,8.9vw,330px)] [--velion-section-vpad:clamp(96px,15vh,180px)]"
			ref={homeRef}
		>
			<Navbar
				isMenuOpen={isMenuOpen}
				isOnDark={false}
				isScrolled={isScrolled}
				onOpen={() => setIsMenuOpen(true)}
			/>

			<MenuModal onClose={() => setIsMenuOpen(false)} open={isMenuOpen} />

			<main className="relative z-[2] bg-background">
				<HeroSection />
				<BrandLogosSection />
				<TrustScrollSection />
				<SensesSection />
				<PromptSection />
				<LayerSection />
				<PreFooterStatementSection />
			</main>

			<div className="relative z-[1] overflow-clip bg-velion-footer-bg">
				<Footer />
			</div>
		</div>
	);
}

export default VelionHome;
