"use client";

import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Navbar } from "@/components/core/navbar/Navbar";
import { MenuModal } from "@/components/ui/MenuModal";

type HomeClientShellProps = {
	content: ReactNode;
	footer: ReactNode;
	hero: ReactNode;
};

/**
 * Owns the small amount of homepage-only browser state. Its slots are
 * server-rendered content, which keeps the homepage composition out of this
 * client boundary while preserving the existing DOM order.
 */
export function HomeClientShell({
	content,
	footer,
	hero,
}: HomeClientShellProps) {
	const homeRef = useRef<HTMLDivElement>(null);
	const menuTriggerRef = useRef<HTMLButtonElement>(null);
	const previousFocusedElementRef = useRef<HTMLElement | null>(null);
	const heroFogScrolledRef = useRef(false);
	const lastFogScrollY = useRef(0);
	const [isMenuOpen, setIsMenuOpen] = useState(false);
	const [isScrolled, setIsScrolled] = useState(false);
	const [isHeroFogScrolled, setIsHeroFogScrolled] = useState(false);

	useEffect(() => {
		let frame: number | null = null;
		lastFogScrollY.current = window.scrollY;

		const updateScrollState = () => {
			const nextScrollY = window.scrollY;
			const delta = nextScrollY - lastFogScrollY.current;
			const reduceMotion = window.matchMedia(
				"(prefers-reduced-motion: reduce)",
			).matches;
			const fogTrigger = Math.max(224, window.innerHeight * 0.6);
			let nextHeroFogScrolled = heroFogScrolledRef.current;

			if (reduceMotion) {
				nextHeroFogScrolled = false;
			} else if (Math.abs(delta) > 1) {
				if (delta > 0 && nextScrollY > fogTrigger) {
					nextHeroFogScrolled = true;
				}

				if (delta < 0) {
					nextHeroFogScrolled = false;
				}
			}

			heroFogScrolledRef.current = nextHeroFogScrolled;
			setIsHeroFogScrolled((current) =>
				current === nextHeroFogScrolled ? current : nextHeroFogScrolled,
			);
			setIsScrolled((current) => {
				const next = reduceMotion
					? nextScrollY > fogTrigger
					: nextHeroFogScrolled;
				return current === next ? current : next;
			});

			lastFogScrollY.current = nextScrollY;
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
		let animationContext: { revert: () => void } | undefined;
		let disposed = false;

		async function setupFooterParallax() {
			const root = homeRef.current;

			if (!root) {
				return;
			}

			const footerElement = root.querySelector<HTMLElement>(
				"[data-footer-parallax]",
			);
			const footerMedia = root.querySelector<HTMLElement>(
				"[data-footer-parallax-media]",
			);

			if (!footerElement || !footerMedia) {
				return;
			}

			const [{ default: gsap }, { ScrollTrigger }] = await Promise.all([
				import("gsap"),
				import("gsap/ScrollTrigger"),
			]);

			if (disposed) {
				return;
			}

			gsap.registerPlugin(ScrollTrigger);
			animationContext = gsap.context(() => {
				const reduceMotion = window.matchMedia(
					"(prefers-reduced-motion: reduce)",
				).matches;

				gsap.set(footerMedia, {
					yPercent: reduceMotion ? 0 : -8,
					force3D: true,
				});

				if (!reduceMotion) {
					gsap.to(footerMedia, {
						yPercent: 0,
						ease: "none",
						scrollTrigger: {
							trigger: footerElement,
							start: "top bottom",
							end: "bottom bottom",
							scrub: true,
							id: "verevon-footer-parallax",
							invalidateOnRefresh: true,
						},
					});
				}
			}, root);

			window.requestAnimationFrame(() => ScrollTrigger.refresh());
		}

		void setupFooterParallax();

		return () => {
			disposed = true;
			animationContext?.revert();
		};
	}, []);

	useEffect(() => {
		const root = homeRef.current;

		if (!root) {
			return;
		}

		const fadeTargets = Array.from(
			root.querySelectorAll<HTMLElement>("[data-fade-out-top]"),
		);

		if (fadeTargets.length === 0) {
			return;
		}

		let frame: number | null = null;

		const updateTextFades = () => {
			for (const element of fadeTargets) {
				const rect = element.getBoundingClientRect();
				const maskY = -rect.top + 90;
				const position = `0px ${maskY.toFixed(3)}px`;

				element.style.setProperty("mask-position", position);
				element.style.setProperty("-webkit-mask-position", position);
			}

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
		if (!isMenuOpen) {
			return;
		}

		document.body.classList.add("verevon-menu-open");

		return () => {
			document.body.classList.remove("verevon-menu-open");
			window.requestAnimationFrame(() => {
				previousFocusedElementRef.current?.focus();
			});
		};
	}, [isMenuOpen]);

	const openMenu = () => {
		previousFocusedElementRef.current =
			document.activeElement instanceof HTMLElement
				? document.activeElement
				: menuTriggerRef.current;
		setIsMenuOpen(true);
	};

	const closeMenu = () => setIsMenuOpen(false);

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
			const reduceMotion = window.matchMedia(
				"(prefers-reduced-motion: reduce)",
			).matches;

			destination.scrollIntoView({
				behavior: reduceMotion ? "auto" : "smooth",
				block: "start",
			});
			window.history.replaceState(null, "", hash);
		};

		document.addEventListener("click", onDocumentClick);

		return () => {
			document.removeEventListener("click", onDocumentClick);
		};
	}, []);

	return (
		<div
			className="min-h-screen bg-background text-verevon-text [--verevon-edge:clamp(56px,5.55vw,208px)] [--verevon-page-pad:clamp(24px,4vw,56px)] [--verevon-section-gap:clamp(86px,8.9vw,330px)] [--verevon-section-vpad:clamp(96px,15vh,180px)]"
			ref={homeRef}
		>
			<div aria-hidden={isMenuOpen || undefined} inert={isMenuOpen}>
				<Navbar
					isMenuOpen={isMenuOpen}
					isOnDark={false}
					isScrolled={isScrolled}
					menuButtonRef={menuTriggerRef}
					onOpen={openMenu}
				/>

				<main className="relative z-[2] bg-background">
					{hero}
					<div aria-hidden="true" className="pointer-events-none relative z-20 h-0">
						<div
							className={[
								"verevon-hima-fog",
								isHeroFogScrolled ? "scrolled" : "",
							]
								.filter(Boolean)
								.join(" ")}
						/>
					</div>
					{content}
				</main>

				<div className="relative z-[1] overflow-clip bg-verevon-footer-bg">
					{footer}
				</div>
			</div>

			<MenuModal onClose={closeMenu} open={isMenuOpen} />
		</div>
	);
}
