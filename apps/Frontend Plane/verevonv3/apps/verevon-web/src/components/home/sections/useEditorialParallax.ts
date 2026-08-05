"use client";

import { useLayoutEffect, type RefObject } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";

gsap.registerPlugin(ScrollTrigger);

type EditorialParallaxOptions = {
	id: string;
	refreshPriority?: number;
};

export function useEditorialParallax(
	sectionRef: RefObject<HTMLElement | null>,
	{ id, refreshPriority = 4 }: EditorialParallaxOptions,
) {
	useLayoutEffect(() => {
		const section = sectionRef.current;

		if (!section) {
			return;
		}

		const reduceMotion = window.matchMedia(
			"(prefers-reduced-motion: reduce)",
		).matches;
		const grid = section.querySelector<HTMLElement>("[data-editorial-grid]");
		const media = section.querySelector<HTMLElement>("[data-editorial-media]");

		if (reduceMotion) {
			gsap.set([grid, media].filter(Boolean), { clearProps: "transform" });
			return;
		}

		const context = gsap.context(() => {
			if (grid) {
				gsap.fromTo(
					grid,
					{ yPercent: -1.25 },
					{
						yPercent: 1.5,
						ease: "none",
						force3D: true,
						scrollTrigger: {
							trigger: section,
							start: "top bottom",
							end: "bottom top",
							scrub: 0.8,
							id: `${id}-grid-parallax`,
							invalidateOnRefresh: true,
							refreshPriority,
						},
					},
				);
			}

			if (media) {
				gsap.fromTo(
					media,
					{ yPercent: -3 },
					{
						yPercent: 3,
						ease: "none",
						force3D: true,
						scrollTrigger: {
							trigger: section,
							start: "top bottom",
							end: "bottom top",
							scrub: 0.65,
							id: `${id}-media-parallax`,
							invalidateOnRefresh: true,
							refreshPriority,
						},
					},
				);
			}
		}, section);

		window.requestAnimationFrame(() => ScrollTrigger.refresh());

		return () => context.revert();
	}, [id, refreshPriority, sectionRef]);
}
