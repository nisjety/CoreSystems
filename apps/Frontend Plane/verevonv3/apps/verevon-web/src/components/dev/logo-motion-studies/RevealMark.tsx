"use client";

import { useEffect, useRef } from "react";
import { REST_PATHS } from "./flag-motion";
import { REVEAL_DURATION_MS, REVEAL_HOLD_MS, REVEAL_SETTLED_MS, revealPhase } from "./reveal-motion";
import { createRibbonRenderer, type RibbonRenderer } from "./ribbon-renderer";
import styles from "./LogoMotionStudies.module.css";

type RevealMarkProps = { paused: boolean; speed?: number; seekTime?: number };

function captureTime() {
	const query = new URLSearchParams(window.location.search);
	if (query.get("static") === "1") return REVEAL_DURATION_MS;
	const time = query.has("t") ? Number(query.get("t")) : undefined;
	return time !== undefined && Number.isFinite(time) ? Math.max(0, Math.min(REVEAL_DURATION_MS, time)) : undefined;
}

export function RevealMark({ paused, speed = 1, seekTime }: RevealMarkProps) {
	const svgRef = useRef<SVGSVGElement>(null);
	const elapsedRef = useRef(0);

	useEffect(() => {
		const svg = svgRef.current;
		if (!svg) return;
		const canonical = svg.querySelector<SVGGElement>("[data-canonical]")!;
		const surface = svg.querySelector<SVGForeignObjectElement>("[data-ribbon-surface]")!;
		const canvas = svg.querySelector("canvas")!;
		let renderer: RibbonRenderer | null = null;
		let failed = false;
		const selectedTime = seekTime ?? captureTime();
		if (selectedTime !== undefined) elapsedRef.current = selectedTime;
		let frame: number | null = null;
		let previousTime: number | null = null;
		let visible = typeof IntersectionObserver === "undefined";
		let preferenceCheck: number | undefined;
		const initialize = () => {
			renderer = createRibbonRenderer(canvas, getComputedStyle(svg).color, () => {
				failed = true; elapsedRef.current = REVEAL_DURATION_MS; draw(); stop();
			});
			if (!renderer) { failed = true; elapsedRef.current = REVEAL_DURATION_MS; }
			return renderer;
		};
		const draw = () => {
			let moving = !failed && elapsedRef.current > REVEAL_HOLD_MS && elapsedRef.current < REVEAL_SETTLED_MS;
			surface.style.display = moving ? "inline" : "none";
			if (moving && !renderer) {
				if (!initialize()) moving = false;
			}
			canonical.style.display = moving ? "none" : "inline";
			surface.style.display = moving ? "inline" : "none";
			if (moving) renderer?.draw(elapsedRef.current);
			svg.dataset.renderer = moving ? "depth-ribbon" : "canonical-svg";
			svg.dataset.time = String(Math.round(elapsedRef.current));
			const phase = revealPhase(elapsedRef.current);
			if (svg.dataset.phase !== phase) svg.dataset.phase = phase;
		};
		const stop = () => {
			if (frame !== null) cancelAnimationFrame(frame);
			frame = null;
			previousTime = null;
			if (elapsedRef.current >= REVEAL_DURATION_MS) {
				window.clearInterval(preferenceCheck);
				preferenceCheck = undefined;
			}
		};
		const tick = (timestamp: number) => {
			if (previousTime !== null) elapsedRef.current = Math.min(REVEAL_DURATION_MS,
				elapsedRef.current + Math.max(0, timestamp - previousTime) * speed);
			previousTime = timestamp;
			draw();
			if (elapsedRef.current < REVEAL_DURATION_MS) frame = requestAnimationFrame(tick);
			else stop();
		};
		const sync = () => {
			// Native CSS remains authoritative despite the site's matchMedia override.
			const reduced = getComputedStyle(svg).getPropertyValue("--flag-motion").trim() === "reduce";
			if (reduced) { elapsedRef.current = REVEAL_DURATION_MS; draw(); }
			// Compile and warm the first GPU draw during the quiet hold, before
			// starting the clock. Never allocate a context for reduced motion.
			if (!reduced && !paused && selectedTime === undefined && visible && !document.hidden
				&& elapsedRef.current <= REVEAL_HOLD_MS && !renderer && !failed) {
				initialize()?.draw(elapsedRef.current);
				if (failed) draw();
			}
			if (reduced || paused || selectedTime !== undefined || !visible || document.hidden || elapsedRef.current >= REVEAL_DURATION_MS) stop();
			else if (frame === null) frame = requestAnimationFrame(tick);
		};
		const observer = typeof IntersectionObserver === "undefined" ? null
			: new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; sync(); });
		observer?.observe(svg);
		document.addEventListener("visibilitychange", sync);
		preferenceCheck = window.setInterval(sync, 400);
		draw();
		sync();
		(window as Window & { __p2mReady?: boolean }).__p2mReady = true;
		return () => {
			stop();
			renderer?.dispose();
			observer?.disconnect();
			window.clearInterval(preferenceCheck);
			document.removeEventListener("visibilitychange", sync);
		};
	}, [paused, speed, seekTime]);

	return (
		<svg role="img" aria-label="Verevon-merket ruller rundt som ett bånd" id="logo-root"
			className={styles.mark} data-motion-mark="reveal" ref={svgRef} viewBox="20 26 82 68" fill="currentColor">
			<g className={styles.animatedMark}>
				<g data-canonical="">
					{REST_PATHS.map((d, index) => <path id={`reveal-${index === 0 ? "body" : "ribbon"}`}
						d={d} data-reveal-contour={index} key={index} />)}
				</g>
				<foreignObject x="20" y="26" width="82" height="68" data-ribbon-surface="" style={{ display: "none" }}>
					<canvas className={styles.ribbonCanvas} aria-hidden="true" />
				</foreignObject>
			</g>
			<g className={styles.staticMark}>
				{REST_PATHS.map((d, index) => <path d={d} key={index} />)}
			</g>
		</svg>
	);
}
