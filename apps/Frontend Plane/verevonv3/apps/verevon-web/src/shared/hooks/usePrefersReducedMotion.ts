"use client";

import { useEffect, useState } from "react";

/**
 * The site can explicitly opt into full motion with `data-motion="full"` on
 * the root element. Otherwise, the visitor's system preference is respected.
 */
export function usePrefersReducedMotion() {
	const [prefersReducedMotion, setPrefersReducedMotion] = useState(false);

	useEffect(() => {
		const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
		const updatePreference = () => {
			const fullMotionIsEnabled =
				document.documentElement.dataset.motion === "full";

			setPrefersReducedMotion(!fullMotionIsEnabled && mediaQuery.matches);
		};

		updatePreference();
		mediaQuery.addEventListener("change", updatePreference);

		return () => mediaQuery.removeEventListener("change", updatePreference);
	}, []);

	return prefersReducedMotion;
}
