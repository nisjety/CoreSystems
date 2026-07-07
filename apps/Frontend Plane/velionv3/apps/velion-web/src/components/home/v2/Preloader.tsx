"use client";

import { useEffect, useState } from "react";

type Phase = "intro" | "reveal" | "done";

/**
 * Preloader — branded wordmark + curtain reveal (maloothifibar pattern).
 *
 * Perf-safe by design:
 *  - The VELION wordmark is plain text painted at first paint, so it (not a
 *    spinner) is the LCP element and LCP stays low.
 *  - Time-boxed (~1.05s hold → 0.72s curtain), never gated on the hero video.
 *  - `prefers-reduced-motion` skips it entirely (hero shows immediately).
 *  - Shown once per session (sessionStorage), so back-navigation doesn't replay.
 */
export function Preloader() {
	const [phase, setPhase] = useState<Phase>("intro");

	useEffect(() => {
		const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
		let seen = false;
		try {
			seen = sessionStorage.getItem("velion-intro-seen") === "1";
		} catch {
			seen = false;
		}

		if (reduce || seen) {
			// Skip the intro — defer so we never setState synchronously in the
			// effect. (Reduced motion is also hidden via CSS regardless.)
			const skip = window.setTimeout(() => setPhase("done"), 0);
			return () => window.clearTimeout(skip);
		}

		try {
			sessionStorage.setItem("velion-intro-seen", "1");
		} catch {
			/* ignore */
		}

		document.body.style.overflow = "hidden";
		const toReveal = window.setTimeout(() => setPhase("reveal"), 1250);
		const toDone = window.setTimeout(() => {
			setPhase("done");
			document.body.style.overflow = "";
		}, 2080);

		return () => {
			window.clearTimeout(toReveal);
			window.clearTimeout(toDone);
			document.body.style.overflow = "";
		};
	}, []);

	if (phase === "done") {
		return null;
	}

	return (
		<div
			aria-hidden="true"
			className={["velion-preloader", phase === "reveal" ? "is-revealing" : ""]
				.filter(Boolean)
				.join(" ")}
		>
			<div className="velion-preloader__inner">
				<span className="velion-preloader__word">VELION</span>
				<span className="velion-preloader__bar">
					<span className="velion-preloader__bar-fill" />
				</span>
				<span className="velion-preloader__label">
					<span className="velion-preloader__label-no">01</span> — Norsk
					AI-arbeidsbenk
				</span>
			</div>
		</div>
	);
}

export default Preloader;
