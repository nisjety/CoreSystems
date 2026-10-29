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

		const fullMotionIsEnabled =
			document.documentElement.dataset.motion === "full";
		let refreshFrame: number | null = null;

		if (
			window.matchMedia("(prefers-reduced-motion: reduce)").matches &&
			!fullMotionIsEnabled
		) {
			gsap.set([media, copy], { clearProps: "transform" });
			return;
		}

		const context = gsap.context(() => {
			const scrollTrigger = {
				trigger: hero,
				start: "top top",
				end: "bottom top",
				scrub: true,
				id: "verevon-hero-parallax",
				invalidateOnRefresh: true,
			};

			// A restrained depth shift keeps the video alive without moving it out
			// from beneath the content as the hero leaves the viewport.
			gsap.fromTo(
				media,
				{ scale: 1.06, yPercent: 0 },
				{ scale: 1.12, yPercent: 10, ease: "none", force3D: true, scrollTrigger },
			);
			gsap.fromTo(
				copy,
				{ yPercent: 0 },
				{ yPercent: 6, ease: "none", force3D: true, scrollTrigger },
			);
		}, hero);

		refreshFrame = window.requestAnimationFrame(() => ScrollTrigger.refresh());

		return () => {
			if (refreshFrame !== null) {
				window.cancelAnimationFrame(refreshFrame);
			}

			context.revert();
		};
	}, []);

	return null;
}

export default HeroParallax;
