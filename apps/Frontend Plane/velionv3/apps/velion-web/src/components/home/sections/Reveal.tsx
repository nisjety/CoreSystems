"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";

type RevealProps = {
	children: ReactNode;
	className?: string;
	/** Optional stagger delay in ms. */
	delay?: number;
};

/**
 * Reveal — the single, calm motion primitive for the V2 homepage.
 *
 * One ambient reveal (fade + small rise) on first scroll-in, via
 * IntersectionObserver. No GSAP, no pin, no scrub, no parallax — the whole
 * point of V2 is restraint. Honors prefers-reduced-motion (shows instantly).
 * Backed by the token-driven `.velion-reveal` class in globals.css.
 */
export function Reveal({ children, className = "", delay = 0 }: RevealProps) {
	const ref = useRef<HTMLDivElement>(null);
	const [revealed, setRevealed] = useState(false);

	useEffect(() => {
		const element = ref.current;

		if (!element) {
			return;
		}

		// Under reduced motion the `.velion-reveal` CSS already forces the
		// element visible, so we only need the observer for the animated path.
		const observer = new IntersectionObserver(
			(entries) => {
				for (const entry of entries) {
					if (entry.isIntersecting) {
						setRevealed(true);
						observer.disconnect();
						break;
					}
				}
			},
			{ threshold: 0.16, rootMargin: "0px 0px -8% 0px" },
		);

		observer.observe(element);

		return () => observer.disconnect();
	}, []);

	return (
		<div
			className={["velion-reveal", className].filter(Boolean).join(" ")}
			data-revealed={revealed ? "true" : undefined}
			ref={ref}
			style={delay ? { transitionDelay: `${delay}ms` } : undefined}
		>
			{children}
		</div>
	);
}

export default Reveal;
