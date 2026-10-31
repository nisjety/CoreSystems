"use client";

import { useEffect, useRef } from "react";
import { FLAG_DURATION_MS, FLAG_LOOP_MS, flagPaths, REST_PATHS, type FlagVariant } from "./flag-motion";
import styles from "./LogoMotionStudies.module.css";

export function FlagMark({ variant, paused, speed = 1 }: { variant: FlagVariant; paused: boolean; speed?: number }) {
	const svgRef = useRef<SVGSVGElement>(null);
	const elapsedRef = useRef(0);

	useEffect(() => {
		const svg = svgRef.current;
		if (!svg) return;
		const paths = Array.from(svg.querySelectorAll<SVGPathElement>("[data-flag-contour]"));
		let frame: number | null = null;
		let previousTime: number | null = null;
		let visible = typeof IntersectionObserver === "undefined";
		const draw = (progress: number) => flagPaths(progress, variant).forEach(
			(d, index) => paths[index].setAttribute("d", d),
		);
		const stop = () => {
			if (frame !== null) cancelAnimationFrame(frame);
			frame = null;
			previousTime = null;
		};
		const tick = (timestamp: number) => {
			if (previousTime !== null) elapsedRef.current += Math.min(timestamp - previousTime, 64) * speed;
			previousTime = timestamp;
			draw(Math.min(1, (elapsedRef.current % FLAG_LOOP_MS) / FLAG_DURATION_MS));
			frame = requestAnimationFrame(tick);
		};
		const sync = () => {
			// The root layout overrides matchMedia. This native CSS media value
			// still reflects the OS preference; CSS also swaps to a static mark.
			const reduced = getComputedStyle(svg).getPropertyValue("--flag-motion").trim() === "reduce";
			if (reduced || paused || !visible || document.hidden) {
				stop();
				if (reduced) { elapsedRef.current = 0; draw(0); }
			} else if (frame === null) frame = requestAnimationFrame(tick);
		};
		const observer = typeof IntersectionObserver === "undefined" ? null
			: new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; sync(); });
		observer?.observe(svg);
		document.addEventListener("visibilitychange", sync);
		// Also detect a live OS preference change despite the site's override.
		const preferenceCheck = window.setInterval(sync, 400);
		sync();
		return () => {
			stop();
			observer?.disconnect();
			window.clearInterval(preferenceCheck);
			document.removeEventListener("visibilitychange", sync);
		};
	}, [variant, paused, speed]);

	return (
		<svg aria-hidden="true" className={styles.mark} data-motion-mark={variant}
			ref={svgRef} viewBox="20 26 82 68" fill="currentColor">
			<g className={styles.animatedMark}>
				{REST_PATHS.map((d, index) => <path d={d} data-flag-contour={index} key={index} />)}
			</g>
			<g className={styles.staticMark}>
				{REST_PATHS.map((d, index) => <path d={d} key={index} />)}
			</g>
		</svg>
	);
}
