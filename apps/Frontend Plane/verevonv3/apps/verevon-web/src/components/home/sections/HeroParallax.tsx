"use client";

import { useLayoutEffect } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";

gsap.registerPlugin(ScrollTrigger);

/**
 * Gives the hero the same exit-depth relationship as the reference site:
 * the visual layer moves faster than the copy while the hero leaves the
 * viewport. The section remains normal flow, so the following content does
 * not gain an artificial pin or scroll distance.
 */
export function HeroParallax() {
	useLayoutEffect(() => {
		const hero = document.querySelector<HTMLElement>(
			"[data-hero-parallax]",
		);
		const media = hero?.querySelector<HTMLElement>(
			"[data-hero-parallax-media]",
		);
		const copy = hero?.querySelector<HTMLElement>(
			"[data-hero-parallax-copy]",
		);

		if (!hero || !media || !copy) {
			return;
		}

		const matchMedia = gsap.matchMedia();
		const fullMotionIsEnabled =
			document.documentElement.dataset.motion === "full";
		let refreshFrame: number | null = null;

		matchMedia.add(
			{ reduceMotion: "(prefers-reduced-motion: reduce)" },
			(context) => {
				const { reduceMotion } = context.conditions as {
					reduceMotion: boolean;
				};

				if (reduceMotion && !fullMotionIsEnabled) {
					gsap.set([media, copy], { clearProps: "transform" });
					return;
				}

				const timeline = gsap.timeline({
					defaults: { ease: "none", force3D: true },
					scrollTrigger: {
						trigger: hero,
						start: "top top",
						end: "bottom top",
						scrub: true,
						id: "verevon-hero-parallax",
						invalidateOnRefresh: true,
					},
				});

				timeline
					.fromTo(media, { yPercent: 0 }, { yPercent: 80, duration: 1 }, 0)
					.fromTo(copy, { yPercent: 0 }, { yPercent: 40, duration: 1 }, 0);

				refreshFrame = window.requestAnimationFrame(() =>
					ScrollTrigger.refresh(),
				);
			},
		);

		return () => {
			if (refreshFrame !== null) {
				window.cancelAnimationFrame(refreshFrame);
			}

			matchMedia.revert();
		};
	}, []);

	return null;
}

export default HeroParallax;
